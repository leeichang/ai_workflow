#!/usr/bin/env bash
#
# 建立完整的展示資料
#
# 與 seed.sh 的差別：
#   seed.sh      最小資料，讓 API 能跑起來
#   seed-demo.sh 完整情境，含多個表單、流程、多角色、多部門，供畫面展示
#
# 用法（順序不可顛倒）：
#   ./start_service.sh      # 1. 先起服務——本腳本會 curl 打 API
#   ./server/seed.sh        # 2. 基礎資料
#   ./server/seed-demo.sh   # 3. 展示資料（含流程定義）
#
# 服務沒起來時 curl 會回 HTTP 000，登入取不到 token 而中止。
set -euo pipefail

ADMIN_URL="${ADMIN_DATABASE_URL:-postgres://localhost:5432/workflow}"
API="${API_URL:-http://localhost:3001}"
TENANT_ID="11111111-1111-1111-1111-111111111111"
TENANT_CODE="demo"

echo "── 補充部門與使用者 ──"

psql "$ADMIN_URL" -q <<SQL
begin;
set local app.tenant_id = '$TENANT_ID';

-- 部門階層：總經理室下有業務、財務、製造
insert into department (tenant_id, code, name) values
  ('$TENANT_ID', 'exec',    '總經理室'),
  ('$TENANT_ID', 'qa',      '品保部'),
  ('$TENANT_ID', 'procure', '採購部')
on conflict (tenant_id, code) do nothing;

update department d set parent_id = e.id
from department e
where d.tenant_id = '$TENANT_ID' and e.tenant_id = '$TENANT_ID'
  and e.code = 'exec' and d.code in ('sales', 'finance', 'mfg', 'qa', 'procure');

-- 使用者。密碼統一為 demo1234。
insert into app_user (tenant_id, email, name, password_hash, department_id)
select '$TENANT_ID', u.email, u.name,
       (select password_hash from app_user
        where tenant_id = '$TENANT_ID' and email = 'admin@demo.local'),
       (select id from department where tenant_id = '$TENANT_ID' and code = u.dept)
from (values
  ('cfo@demo.local',      '張文華', 'exec'),
  ('finance@demo.local',  '李淑芬', 'finance'),
  ('qa@demo.local',       '黃志明', 'qa'),
  ('procure@demo.local',  '吳佩珊', 'procure'),
  ('sales2@demo.local',   '劉建國', 'sales')
) as u(email, name, dept)
on conflict (tenant_id, email) do nothing;

-- 主管鏈：業務員向業務主管報告，主管向總經理報告
update app_user u set manager_id = m.id
from app_user m
where u.tenant_id = '$TENANT_ID' and m.tenant_id = '$TENANT_ID'
  and m.email = 'designer@demo.local'
  and u.email in ('sales@demo.local', 'sales2@demo.local');

update app_user u set manager_id = m.id
from app_user m
where u.tenant_id = '$TENANT_ID' and m.tenant_id = '$TENANT_ID'
  and m.email = 'cfo@demo.local'
  and u.email in ('designer@demo.local', 'finance@demo.local');

-- 部門主管
update department d set manager_user_id = u.id
from app_user u
where d.tenant_id = '$TENANT_ID' and u.tenant_id = '$TENANT_ID'
  and ((d.code = 'sales'   and u.email = 'designer@demo.local')
    or (d.code = 'finance' and u.email = 'finance@demo.local')
    or (d.code = 'qa'      and u.email = 'qa@demo.local')
    or (d.code = 'procure' and u.email = 'procure@demo.local')
    or (d.code = 'exec'    and u.email = 'cfo@demo.local'));

-- 補充角色
insert into role (tenant_id, code, name, is_system) values
  ('$TENANT_ID', 'finance_manager', '財務主管',   false),
  ('$TENANT_ID', 'cfo',             '財務長',     false),
  ('$TENANT_ID', 'sales_director',  '業務總監',   false)
on conflict (tenant_id, code) do nothing;

-- 角色指派
insert into user_role (user_id, role_id, tenant_id)
select u.id, r.id, '$TENANT_ID'
from app_user u
join role r on r.tenant_id = '$TENANT_ID'
where u.tenant_id = '$TENANT_ID'
  and ((u.email = 'cfo@demo.local'     and r.code in ('cfo', 'approver'))
    or (u.email = 'finance@demo.local' and r.code in ('finance_manager', 'approver'))
    or (u.email = 'qa@demo.local'      and r.code = 'approver')
    or (u.email = 'procure@demo.local' and r.code = 'requester')
    or (u.email = 'sales2@demo.local'  and r.code = 'requester'))
on conflict do nothing;

commit;
SQL

echo "── 登入 ──"
TOKEN=$(curl -s -X POST "$API/auth/login" \
  -H 'Content-Type: application/json' \
  -d "{\"tenant_code\":\"$TENANT_CODE\",\"email\":\"designer@demo.local\",\"password\":\"demo1234\"}" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin).get("access_token",""))')

if [[ -z "$TOKEN" ]]; then
    echo "登入失敗。請先執行 ./server/seed.sh 並確認 API 已啟動" >&2
    exit 1
fi

create_form() {
    # macOS 的 bash 3.2：同一個 local 敘述不可引用前面剛宣告的變數
    local key="$1"; local name="$2"; local file="$3"
    printf '  %-22s' "$key"
    local code
    code=$(curl -s -o /tmp/seed-resp.json -w '%{http_code}' -X POST "$API/forms" \
        -H "Authorization: Bearer $TOKEN" \
        -H 'Content-Type: application/json' \
        -d @"$file")
    case "$code" in
        201) printf '已建立 ' ;;
        # 已存在時仍要往下嘗試發布：環境可能是在加上發布這一步之前
        # 建的，表單停在草稿。直接 return 會讓那些表單永遠鎖不到版本。
        409) printf '已存在 ' ;;
        *)   echo "失敗 HTTP $code"; head -c 200 /tmp/seed-resp.json; echo; return ;;
    esac

    # 建立只產生草稿，要發布才會有版本。
    #
    # 不發布的話 workflow_instance.form_version_id 永遠鎖不到東西——
    # 版本鎖定只認 PUBLISHED，草稿隨時會變，鎖它等於沒鎖。
    # 先前 seed 少了這一步，所有表單都停在草稿，
    # 畫面能運作只是因為讀取端會退回草稿。
    local pub
    pub=$(curl -s -o /tmp/seed-resp.json -w '%{http_code}' -X POST \
        "$API/forms/$key/draft/publish" \
        -H "Authorization: Bearer $TOKEN")

    case "$pub" in
        200) echo "並已發布" ;;
        # 沒有草稿可發布（已經是發布狀態）——冪等，不是錯誤
        409) echo "（已是發布狀態）" ;;
        *)   echo "發布失敗 HTTP $pub"; head -c 300 /tmp/seed-resp.json; echo ;;
    esac
}

echo "── 建立表單 ──"

# 採購申請單。欄位依決議 D-01b：人工填寫，金額門檻動態並簽。
cat > /tmp/form-purchase.json <<'JSON'
{
  "form_key": "purchase_request_form",
  "business_object": "purchase_request",
  "name": "採購申請單 (FM-PROC-PR01)",
  "content": {
    "form_key": "purchase_request_form",
    "version": 1,
    "business_object": "purchase_request",
    "name": "採購申請單",
    "sections": [
      { "key": "basic",    "title": "申請基本資料" },
      { "key": "supplier", "title": "供應商資訊" },
      { "key": "lines",    "title": "採購明細" },
      { "key": "amount",   "title": "金額彙總" },
      { "key": "internal", "title": "內部管控" }
    ],
    "fields": [
      {
        "key": "number",
        "section": "basic",
        "ui": { "component": "display", "label": "申請單號", "width": 4 },
        "data": { "path": "purchase_request.number", "type": "string" }
      },
      {
        "key": "need_by_date",
        "section": "basic",
        "ui": { "component": "date", "label": "需求日期", "width": 4 },
        "data": { "path": "purchase_request.need_by_date", "type": "date" },
        "workflow": { "required_when": "true",
                      "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "department",
        "section": "basic",
        "ui": { "component": "department_picker", "label": "申請部門", "width": 4 },
        "data": { "path": "purchase_request.department_id", "type": "uuid" },
        "workflow": { "required_when": "true",
                      "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "reason",
        "section": "basic",
        "ui": { "component": "textarea", "label": "申請事由", "width": 12,
                "placeholder": "說明採購用途與必要性" },
        "data": { "path": "purchase_request.reason", "type": "text", "max_length": 1000 },
        "workflow": { "required_when": "true",
                      "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "supplier",
        "section": "supplier",
        "ui": { "component": "reference", "label": "供應商", "width": 6,
                "help": "未指定時流程會退回要求補齊",
                "reference": { "source": "suppliers", "label_field": "name" } },
        "data": { "path": "purchase_request.supplier_party_id", "type": "uuid" },
        "workflow": { "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "currency",
        "section": "supplier",
        "ui": { "component": "select", "label": "幣別", "width": 3,
                "options": [
                  { "value": "TWD", "label": "新台幣" },
                  { "value": "USD", "label": "美金" },
                  { "value": "JPY", "label": "日圓" }
                ] },
        "data": { "path": "purchase_request.currency", "type": "string", "default": "TWD" },
        "workflow": { "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "lines",
        "section": "lines",
        "ui": {
          "component": "table",
          "label": "採購品項",
          "width": 12,
          "columns": [
            { "key": "product_ref", "label": "品號", "component": "reference", "width": "180px",
              "reference": { "source": "products", "label_field": "name" } },
            { "key": "name",        "label": "品名規格", "component": "input", "width": "auto" },
            { "key": "qty",         "label": "數量", "component": "number", "width": "90px",
              "align": "right", "precision": 2 },
            { "key": "unit",        "label": "單位", "component": "input", "width": "70px" },
            { "key": "unit_price",  "label": "單價", "component": "number", "width": "110px",
              "align": "right", "precision": 2 },
            { "key": "amount",      "label": "金額", "component": "number", "width": "120px",
              "align": "right", "precision": 2, "readonly": true,
              "formula": "row.qty * row.unit_price" },
            { "key": "note",        "label": "備註", "component": "input", "width": "140px" }
          ]
        },
        "data": { "path": "purchase_request.lines", "type": "array", "item": "purchase_line" },
        "workflow": { "required_when": "true",
                      "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "subtotal",
        "section": "amount",
        "ui": { "component": "display", "label": "小計", "width": 4, "precision": 2 },
        "data": { "path": "purchase_request.subtotal", "type": "decimal",
                  "computed": "sum(purchase_request.lines.amount)" }
      },
      {
        "key": "tax",
        "section": "amount",
        "ui": { "component": "display", "label": "稅額", "width": 4, "precision": 2 },
        "data": { "path": "purchase_request.tax", "type": "decimal",
                  "computed": "purchase_request.subtotal * 0.05" }
      },
      {
        "key": "total",
        "section": "amount",
        "ui": { "component": "display", "label": "總計", "width": 4, "precision": 2 },
        "data": { "path": "purchase_request.total", "type": "decimal",
                  "computed": "purchase_request.subtotal + purchase_request.tax" }
      },
      {
        "key": "budget_note",
        "section": "internal",
        "ui": { "component": "textarea", "label": "預算來源說明", "width": 12,
                "external_visible": false },
        "data": { "path": "purchase_request.budget_note", "type": "text" },
        "workflow": { "readable_roles": ["admin", "approver", "finance_manager", "cfo"] }
      },
      {
        "key": "attachments",
        "section": "internal",
        "ui": { "component": "upload", "label": "報價單與規格附件", "width": 12 },
        "data": { "path": "purchase_request.attachments", "type": "array", "item": "file" },
        "workflow": { "readonly_when": "node.id not in ['start','revise']" }
      }
    ]
  }
}
JSON
create_form "purchase_request_form" "採購申請單" /tmp/form-purchase.json

# 供應商評鑑表。展示不同業務物件與較簡單的結構。
cat > /tmp/form-supplier.json <<'JSON'
{
  "form_key": "supplier_review_form",
  "business_object": "supplier_review",
  "name": "供應商評鑑表 (FM-QA-SR01)",
  "content": {
    "form_key": "supplier_review_form",
    "version": 1,
    "business_object": "supplier_review",
    "name": "供應商評鑑表",
    "sections": [
      { "key": "target", "title": "評鑑對象" },
      { "key": "score",  "title": "評分項目" },
      { "key": "result", "title": "評鑑結果" }
    ],
    "fields": [
      {
        "key": "supplier",
        "section": "target",
        "ui": { "component": "reference", "label": "供應商", "width": 6,
                "reference": { "source": "suppliers", "label_field": "name" } },
        "data": { "path": "review.supplier_id", "type": "uuid" },
        "workflow": { "required_when": "true" }
      },
      {
        "key": "period",
        "section": "target",
        "ui": { "component": "select", "label": "評鑑期間", "width": 6,
                "options": [
                  { "value": "2026H1", "label": "2026 上半年" },
                  { "value": "2026H2", "label": "2026 下半年" }
                ] },
        "data": { "path": "review.period", "type": "string" },
        "workflow": { "required_when": "true" }
      },
      {
        "key": "quality_score",
        "section": "score",
        "ui": { "component": "number", "label": "品質 (40%)", "width": 3,
                "precision": 0, "suffix": "分" },
        "data": { "path": "review.quality_score", "type": "integer", "min": 0, "max": 100 },
        "workflow": { "required_when": "true", "editable_roles": ["admin", "approver"] }
      },
      {
        "key": "delivery_score",
        "section": "score",
        "ui": { "component": "number", "label": "交期 (30%)", "width": 3,
                "precision": 0, "suffix": "分" },
        "data": { "path": "review.delivery_score", "type": "integer", "min": 0, "max": 100 },
        "workflow": { "required_when": "true", "editable_roles": ["admin", "approver"] }
      },
      {
        "key": "price_score",
        "section": "score",
        "ui": { "component": "number", "label": "價格 (20%)", "width": 3,
                "precision": 0, "suffix": "分" },
        "data": { "path": "review.price_score", "type": "integer", "min": 0, "max": 100 },
        "workflow": { "required_when": "true", "editable_roles": ["admin", "approver"] }
      },
      {
        "key": "service_score",
        "section": "score",
        "ui": { "component": "number", "label": "服務 (10%)", "width": 3,
                "precision": 0, "suffix": "分" },
        "data": { "path": "review.service_score", "type": "integer", "min": 0, "max": 100 },
        "workflow": { "required_when": "true", "editable_roles": ["admin", "approver"] }
      },
      {
        "key": "total_score",
        "section": "result",
        "ui": { "component": "display", "label": "加權總分", "width": 4, "precision": 1 },
        "data": { "path": "review.total_score", "type": "decimal",
                  "computed": "quality_score*0.4 + delivery_score*0.3 + price_score*0.2 + service_score*0.1" }
      },
      {
        "key": "grade",
        "section": "result",
        "ui": { "component": "display", "label": "評等", "width": 4 },
        "data": { "path": "review.grade", "type": "string",
                  "computed": "total_score >= 90 ? 'A' : total_score >= 75 ? 'B' : 'C'" }
      },
      {
        "key": "comment",
        "section": "result",
        "ui": { "component": "textarea", "label": "改善建議", "width": 12 },
        "data": { "path": "review.comment", "type": "text" },
        "workflow": { "editable_roles": ["admin", "approver"] }
      }
    ]
  }
}
JSON
create_form "supplier_review_form" "供應商評鑑表" /tmp/form-supplier.json

rm -f /tmp/form-purchase.json /tmp/form-supplier.json

# ── 流程定義 ────────────────────────────────────────────
#
# 先前 seed 只建表單不建流程，導致 task-approval.spec.ts 的 6 個測試
# 全部回 404（POST /instances 找不到 quotation_approval）。
# 錯誤訊息是「啟動流程失敗」，看起來像服務沒起來，實際六個服務都正常——
# 這種誤導性讓新接手的人會在這裡卡很久。
#
# 流程與表單不同，要兩步：建立（草稿）之後還要發布。
# 只建不發布的話 POST /instances 一樣找不到——它只認 PUBLISHED 版本。

create_workflow() {
    local key="$1" name="$2" object="$3" fixture="$4"
    printf '  %-22s' "$key"

    # fixture 本身就是 content。API 要的是外層包 workflow_key 等欄位的結構，
    # 用 python 組裝比在 shell 裡拼 JSON 安全。
    python3 - "$fixture" "$key" "$name" "$object" > /tmp/seed-wf.json <<'PY'
import json, sys
fixture, key, name, object_ = sys.argv[1:5]
with open(fixture, encoding='utf-8') as f:
    content = json.load(f)
print(json.dumps({
    'workflow_key': key,
    'business_object': object_,
    'name': name,
    'description': content.get('description', ''),
    'content': content,
}, ensure_ascii=False))
PY

    local code
    code=$(curl -s -o /tmp/seed-resp.json -w '%{http_code}' -X POST "$API/workflows" \
        -H "Authorization: Bearer $TOKEN" \
        -H 'Content-Type: application/json' \
        -d @/tmp/seed-wf.json)

    case "$code" in
        201) printf '已建立 ' ;;
        409) printf '已存在 ' ;;
        *)   echo "失敗 HTTP $code"; head -c 300 /tmp/seed-resp.json; echo; return ;;
    esac

    # 發布。已發布過會回 409，視為正常。
    local pub
    pub=$(curl -s -o /tmp/seed-resp.json -w '%{http_code}' -X POST \
        "$API/workflows/$key/draft/publish" \
        -H "Authorization: Bearer $TOKEN")

    case "$pub" in
        200|201) echo "已發布" ;;
        409)     echo "（已是發布狀態）" ;;
        *)       echo "發布失敗 HTTP $pub"; head -c 300 /tmp/seed-resp.json; echo ;;
    esac
}

echo "── 建立流程 ──"
create_workflow "quotation_approval" "報價單簽核流程" "quotation" \
    "$(dirname "$0")/../schemas/fixtures/quotation_approval_v1.json"
create_workflow "purchase_approval" "採購申請簽核流程" "purchase_request" \
    "$(dirname "$0")/../schemas/fixtures/purchase_approval_v1.json"

rm -f /tmp/seed-wf.json /tmp/seed-resp.json

echo
echo "── 完成 ──"
psql "$ADMIN_URL" -tA <<SQL
begin;
set local app.tenant_id = '$TENANT_ID';
select '  使用者 ' || count(*) || ' 人' from app_user where tenant_id = '$TENANT_ID';
select '  部門 '   || count(*) || ' 個' from department where tenant_id = '$TENANT_ID';
select '  角色 '   || count(*) || ' 種' from role where tenant_id = '$TENANT_ID';
select '  表單 '   || count(*) || ' 張' from form_definition where tenant_id = '$TENANT_ID';
select '  流程 '   || count(*) || ' 個（已發布 '
       || (select count(*) from workflow_definition_version v
           join workflow_definition d2 on d2.id = v.workflow_id
           where d2.tenant_id = '$TENANT_ID' and v.status = 'PUBLISHED')
       || ' 版）'
  from workflow_definition where tenant_id = '$TENANT_ID';
commit;
SQL

cat <<'INFO'

  登入：http://localhost:3040
  租戶代碼 demo，密碼統一 demo1234

  帳號                    姓名     角色
  admin@demo.local        林建志   系統管理員
  designer@demo.local     陳雅婷   流程設計者、業務部主管
  sales@demo.local        王景榮   申請人
  sales2@demo.local       劉建國   申請人
  cfo@demo.local          張文華   財務長、簽核人員
  finance@demo.local      李淑芬   財務主管、簽核人員
  qa@demo.local           黃志明   簽核人員
  procure@demo.local      吳佩珊   申請人
INFO
