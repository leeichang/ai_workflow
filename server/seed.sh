#!/usr/bin/env bash
#
# 建立開發用的測試資料
#
# 用法：./server/seed.sh
# 產出一個租戶、三個使用者、一張依設計稿欄位的報價單表單。
set -euo pipefail

ADMIN_URL="${ADMIN_DATABASE_URL:-postgres://localhost:5432/workflow}"
API="${API_URL:-http://localhost:3001}"

TENANT_ID="11111111-1111-1111-1111-111111111111"
TENANT_CODE="demo"

echo "建立租戶與使用者"

# argon2 雜湊，明碼為 demo1234
PASSWORD_HASH='$argon2id$v=19$m=19456,t=2,p=1$ZGV2c2FsdGRldnNhbHRkZXY$EYnjvkPmvkr+4NcjGxmwtqeh+PtjDuxPhln8LtBejN8'

psql "$ADMIN_URL" -q <<SQL
begin;

insert into tenant (id, code, name, timezone, currency)
values ('$TENANT_ID', '$TENANT_CODE', '台製精工股份有限公司', 'Asia/Taipei', 'TWD')
on conflict (id) do update set
  code = excluded.code,
  name = excluded.name,
  status = 'ACTIVE';

set local app.tenant_id = '$TENANT_ID';

insert into role (tenant_id, code, name, is_system) values
  ('$TENANT_ID', 'admin',     '系統管理員', true),
  ('$TENANT_ID', 'designer',  '流程設計者', true),
  ('$TENANT_ID', 'approver',  '簽核人員',   true),
  ('$TENANT_ID', 'requester', '申請人',     true),
  ('$TENANT_ID', 'viewer',    '檢視者',     true),
  -- 流程監控員（N1，2026-09-19 定案）。語意單一：看全部流程並介入
  -- 處理。刻意不讓 approver／finance_manager 兼差——那些是「會簽到
  -- 某些單」不是「該看到全部單」
  ('$TENANT_ID', 'process_monitor', '流程監控員', true)
on conflict (tenant_id, code) do nothing;

insert into department (tenant_id, code, name) values
  ('$TENANT_ID', 'sales',   '業務一部'),
  ('$TENANT_ID', 'finance', '財務部'),
  ('$TENANT_ID', 'mfg',     '營運製造部')
on conflict (tenant_id, code) do nothing;

insert into app_user (tenant_id, email, name, password_hash, department_id)
select '$TENANT_ID', u.email, u.name, '$PASSWORD_HASH',
       (select id from department where tenant_id = '$TENANT_ID' and code = u.dept)
from (values
  ('admin@demo.local',    '林建志', 'mfg'),
  ('designer@demo.local', '陳雅婷', 'sales'),
  ('sales@demo.local',    '王景榮', 'sales'),
  -- 流程監控員。獨立於 admin 才驗得出「非 admin 也看得到全部」，
  -- 用 admin 測等於沒測到新角色
  ('monitor@demo.local',  '周美玲', 'mfg')
) as u(email, name, dept)
on conflict (tenant_id, email) do nothing;

insert into user_role (user_id, role_id, tenant_id)
select u.id, r.id, '$TENANT_ID'
from app_user u
join role r on r.tenant_id = '$TENANT_ID'
where u.tenant_id = '$TENANT_ID'
  and ((u.email = 'admin@demo.local'    and r.code = 'admin')
    or (u.email = 'designer@demo.local' and r.code = 'designer')
    or (u.email = 'sales@demo.local'    and r.code = 'requester')
    or (u.email = 'monitor@demo.local'  and r.code = 'process_monitor'))
on conflict do nothing;

commit;
SQL

echo "登入取得 token"
TOKEN=$(curl -s -X POST "$API/auth/login" \
  -H 'Content-Type: application/json' \
  -d "{\"tenant_code\":\"$TENANT_CODE\",\"email\":\"designer@demo.local\",\"password\":\"demo1234\"}" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin).get("access_token",""))')

if [[ -z "$TOKEN" ]]; then
    echo "登入失敗。請確認 API 已啟動：" >&2
    echo "  DATABASE_URL=postgres://workflow_app:app_dev_only@localhost:5433/workflow \\" >&2
    echo "  cargo run -p api" >&2
    exit 1
fi

# 表單定義先寫成檔案，因為要送兩次：
# 第一次用 POST 建立，若表單已存在（撞 form_key 的唯一約束）
# 就改用 PUT 更新草稿。
#
# 先前只有 POST，於是**表單定義的任何修改都不會套用到既有環境**——
# seed 印出 CONFLICT 就跳過，接著把舊草稿發布出去。
# 這讓 readable_roles 這類後來才加的設定在實機上從未生效過。
FORM_JSON=$(mktemp)
trap 'rm -f "$FORM_JSON"' EXIT

cat > "$FORM_JSON" <<'JSON'
{
  "form_key": "quotation_form",
  "business_object": "quotation",
  "name": "報價單 (FM-SALES-QT01)",
  "content": {
    "form_key": "quotation_form",
    "version": 1,
    "business_object": "quotation",
    "name": "報價單",
    "sections": [
      { "key": "customer", "title": "客戶資訊" },
      { "key": "terms", "title": "報價條件與折扣" },
      { "key": "lines", "title": "報價明細清單" },
      { "key": "amount", "title": "金額與稅額" },
      { "key": "internal", "title": "內部資訊與管控" }
    ],
    "fields": [
      {
        "key": "customer_name",
        "section": "customer",
        "ui": { "component": "reference", "label": "客戶名稱 (主鍵)", "width": 6,
                "reference": { "source": "partners", "label_field": "name" } },
        "data": { "path": "customer.name", "type": "string" },
        "workflow": { "required_when": "true",
                      "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "tax_id",
        "section": "customer",
        "ui": { "component": "input", "label": "統一編號", "width": 6 },
        "data": { "path": "customer.tax_id", "type": "string", "pattern": "^[0-9]{8}$" },
        "workflow": { "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "payment_terms",
        "section": "customer",
        "ui": { "component": "input", "label": "付款條件", "width": 12,
                "placeholder": "例如：月結 60 天 (TT 匯款)" },
        "data": { "path": "customer.payment_terms", "type": "string" },
        "workflow": { "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "discount_rate",
        "section": "terms",
        "ui": { "component": "number", "label": "表頭折扣率", "width": 4,
                "precision": 4, "suffix": "%" },
        "data": { "path": "quotation.discount_rate", "type": "decimal", "min": 0, "max": 0.5 },
        "workflow": { "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "currency",
        "section": "terms",
        "ui": { "component": "select", "label": "報價幣別", "width": 4,
                "options": [
                  { "value": "TWD", "label": "新台幣 (TWD)" },
                  { "value": "USD", "label": "美金 (USD)" },
                  { "value": "JPY", "label": "日圓 (JPY)" }
                ] },
        "data": { "path": "quotation.currency", "type": "string", "default": "TWD" },
        "workflow": { "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "lines",
        "section": "lines",
        "ui": {
          "component": "table",
          "label": "報價明細表格 (子表群組)",
          "width": 12,
          "columns": [
            { "key": "part_no",     "label": "料號 / 規格品名", "component": "reference", "width": "200px",
              "reference": { "source": "products", "label_field": "name" } },
            { "key": "drawing_no",  "label": "圖號 / 版次", "component": "input", "width": "130px" },
            { "key": "material",    "label": "材質 / 熱處理條件", "component": "input", "width": "150px" },
            { "key": "qty",         "label": "數量", "component": "number", "width": "90px",
              "align": "right", "precision": 0 },
            { "key": "unit",        "label": "單位", "component": "input", "width": "70px" },
            { "key": "material_cost","label": "單位材料費", "component": "number", "width": "110px",
              "align": "right", "precision": 2 },
            { "key": "process_cost","label": "單位加工費", "component": "number", "width": "110px",
              "align": "right", "precision": 2 },
            { "key": "target_price","label": "目標報價", "component": "number", "width": "110px",
              "align": "right", "precision": 2 },
            { "key": "discount",    "label": "折扣率", "component": "number", "width": "90px",
              "align": "right", "precision": 3 }
          ]
        },
        "data": { "path": "quotation.lines", "type": "array", "item": "quotation_line" },
        "workflow": { "required_when": "true",
                      "readonly_when": "node.id not in ['start','revise']" }
      },
      {
        "key": "margin_rate",
        "section": "lines",
        "ui": { "component": "display", "label": "預估毛利率 (%)", "width": 4,
                "precision": 2, "suffix": "%", "external_visible": false },
        "data": { "path": "quotation.margin_rate", "type": "decimal",
                  "computed": "(subtotal - total_cost) / subtotal" }
      },
      {
        "key": "total",
        "section": "amount",
        "ui": { "component": "display", "label": "報價總計 (含稅)", "width": 4, "precision": 2 },
        "data": { "path": "quotation.total", "type": "decimal",
                  "computed": "subtotal + tax" }
      },
      {
        "key": "internal_note",
        "section": "internal",
        "ui": { "component": "textarea", "label": "內部簽核備註", "width": 12,
                "help": "僅限廠務主管與核決層可見", "external_visible": false },
        "data": { "path": "quotation.internal_note", "type": "text", "max_length": 2000 },
        "workflow": { "readable_roles": ["admin", "approver", "designer"] }
      }
    ]
  }
}
JSON

echo "建立報價單表單"
create=$(curl -s -o /tmp/seed-form-create.json -w '%{http_code}' -X POST "$API/forms" \
  -H "Authorization: Bearer $TOKEN" \
  -H 'Content-Type: application/json' \
  -d @"$FORM_JSON")

if [ "$create" = "409" ]; then
    # 已存在。把 content 取出來當草稿送——PUT /draft 的 body 只要 content
    printf '  表單已存在，改為更新草稿  '
    python3 -c "
import json, sys
doc = json.load(open('$FORM_JSON'))
json.dump({'content': doc['content']}, open('$FORM_JSON.draft', 'w'))
"
    upd=$(curl -s -o /tmp/seed-form-draft.json -w '%{http_code}' -X PUT \
      "$API/forms/quotation_form/draft" \
      -H "Authorization: Bearer $TOKEN" \
      -H 'Content-Type: application/json' \
      -d @"$FORM_JSON.draft")
    rm -f "$FORM_JSON.draft"

    case "$upd" in
        2*) echo "已更新" ;;
        *)  echo "失敗（HTTP $upd）"; cat /tmp/seed-form-draft.json; exit 1 ;;
    esac
elif [ "${create#2}" != "$create" ]; then
    echo "  已建立"
else
    echo "  建立失敗（HTTP $create）"
    cat /tmp/seed-form-create.json
    exit 1
fi

# 建立只產生草稿，要發布才會有版本。
#
# workflow_instance.form_version_id 只鎖 PUBLISHED 的版本——
# 草稿隨時會變，鎖它等於沒鎖。少了這一步，所有實例的
# form_version_id 都會是 NULL。
echo
printf '發布報價單表單  '
pub=$(curl -s -o /tmp/seed-form-publish.json -w '%{http_code}' -X POST \
  "$API/forms/quotation_form/draft/publish" \
  -H "Authorization: Bearer $TOKEN")
case "$pub" in
  200) echo "已發布" ;;
  # 沒有草稿可發布代表已經是發布狀態——冪等，不是錯誤
  409) echo "已是發布狀態" ;;
  *)   echo "失敗 HTTP $pub"; head -c 200 /tmp/seed-form-publish.json; echo ;;
esac
rm -f /tmp/seed-form-publish.json

echo
echo "完成。登入資訊："
echo "  租戶代碼：$TENANT_CODE"
echo "  帳號：designer@demo.local / admin@demo.local / sales@demo.local"
echo "  密碼：demo1234"
