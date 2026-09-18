#!/usr/bin/env bash
#
# 建立完整的展示資料
#
# 與 seed.sh 的差別：
#   seed.sh      最小資料，讓 API 能跑起來
#   seed-demo.sh 完整情境，含多個表單、多角色、多部門，供畫面展示
#
# 用法：
#   ./server/seed.sh        # 先建基礎資料
#   ./server/seed-demo.sh   # 再疊加展示資料
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
    local key="$1" name="$2" file="$3"
    printf '  %-22s' "$key"
    local code
    code=$(curl -s -o /tmp/seed-resp.json -w '%{http_code}' -X POST "$API/forms" \
        -H "Authorization: Bearer $TOKEN" \
        -H 'Content-Type: application/json' \
        -d @"$file")
    case "$code" in
        201) echo "已建立" ;;
        409) echo "已存在，略過" ;;
        *)   echo "失敗 HTTP $code"; head -c 200 /tmp/seed-resp.json; echo ;;
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

# ── 流程定義 ────────────────────────────────────────────
#
# 為什麼要在這裡建：沒有流程定義的話，web/e2e/task-approval.spec.ts
# 的 6 個測試會全部回 404（POST /instances 找不到 quotation_approval），
# 而錯誤訊息是「啟動流程失敗」——看起來像服務沒起來，
# 但六個服務其實都正常。新接手的人會在這裡卡很久。
#
# 定義取自 schemas/fixtures/，與 Rust validator 的測試 fixture 同一份。
# 兩邊共用一份可以避免「測試通過但實際跑不起來」。

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

create_workflow() {
    # macOS 的 bash 3.2：同一個 local 敘述不可引用前面剛宣告的變數
    local key="$1"; local fixture="$2"
    printf '  %-22s' "$key"

    if [[ ! -f "$fixture" ]]; then
        echo "找不到 $fixture"
        return
    fi

    # 把 fixture 包成 API 要的建立請求
    python3 - "$fixture" "$key" > /tmp/seed-wf.json <<'PY'
import json, sys
fixture, key = sys.argv[1], sys.argv[2]
dsl = json.load(open(fixture))
json.dump({
    "workflow_key": key,
    "business_object": dsl["business_object"],
    "name": dsl["name"],
    "description": dsl.get("description"),
    "content": dsl,
}, open("/dev/stdout", "w"), ensure_ascii=False)
PY

    local code
    code=$(curl -s -o /tmp/seed-resp.json -w '%{http_code}' -X POST "$API/workflows" \
        -H "Authorization: Bearer $TOKEN" \
        -H 'Content-Type: application/json' \
        -d @/tmp/seed-wf.json)

    case "$code" in
        201) printf '已建立 ' ;;
        409) echo "已存在，略過"; return ;;
        *)   echo "建立失敗 HTTP $code"; head -c 200 /tmp/seed-resp.json; echo; return ;;
    esac

    # 發布。此處才做完整圖結構驗證（含 WF-E012 路徑存在性），
    # 驗不過表示 fixture 與業務物件定義不一致，要當成錯誤而非略過。
    local pub
    pub=$(curl -s -o /tmp/seed-resp.json -w '%{http_code}' -X POST \
        "$API/workflows/$key/draft/publish" \
        -H "Authorization: Bearer $TOKEN")

    case "$pub" in
        200) echo "並已發布" ;;
        *)   echo "發布失敗 HTTP $pub"; head -c 300 /tmp/seed-resp.json; echo ;;
    esac
}

echo "── 建立流程定義 ──"
create_workflow "quotation_approval" "$ROOT/schemas/fixtures/quotation_approval_v1.json"

rm -f /tmp/form-purchase.json /tmp/form-supplier.json /tmp/seed-resp.json /tmp/seed-wf.json

echo
echo "── 完成 ──"
psql "$ADMIN_URL" -tA <<SQL
begin;
set local app.tenant_id = '$TENANT_ID';
select '  使用者 ' || count(*) || ' 人' from app_user where tenant_id = '$TENANT_ID';
select '  部門 '   || count(*) || ' 個' from department where tenant_id = '$TENANT_ID';
select '  角色 '   || count(*) || ' 種' from role where tenant_id = '$TENANT_ID';
select '  表單 '   || count(*) || ' 張' from form_definition where tenant_id = '$TENANT_ID';
select '  流程 '   || count(*) || ' 個' from workflow_definition where tenant_id = '$TENANT_ID';
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
