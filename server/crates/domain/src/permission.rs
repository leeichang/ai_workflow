//! 欄位權限解析
//!
//! 對應設計稿 docs/UI 設計/.../_7（表單權限設定矩陣）。
//!
//! 權限三態：可編輯、唯讀、隱藏。
//! 由四個因素決定，優先序由高而低：
//!   1. 欄位本身是計算欄位（`data.computed`）→ 強制唯讀，使用者改不了
//!   2. 角色不在 `readable_roles` → 隱藏
//!   3. `visible_when` 求值為 false → 隱藏
//!   4. `readonly_when` 求值為 true，或角色不在 `editable_roles` → 唯讀
//!   否則可編輯。
//!
//! 表達式求值目前是簡化版，只支援矩陣 UI 會產生的形式。
//! 完整 CEL 實作排在任務 2.2，屆時此處改為呼叫共用求值器。

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Permission {
    /// 可編輯
    Editable,
    /// 唯讀，看得到但改不了
    Readonly,
    /// 隱藏，完全看不到
    Hidden,
}

/// 求值情境
#[derive(Debug, Clone)]
pub struct Context<'a> {
    /// 當前流程節點 id，`None` 代表表單初始狀態
    pub node_id: Option<&'a str>,
    /// 當前使用者角色
    pub roles: &'a [String],
    /// 參與者類型：internal 或 external
    pub participant_kind: &'a str,
    /// 業務資料，供條件式權限求值
    pub data: &'a Value,
}

impl<'a> Context<'a> {
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }

    pub fn is_admin(&self) -> bool {
        self.has_role("admin")
    }

    pub fn is_external(&self) -> bool {
        self.participant_kind == "external"
    }
}

/// 單一欄位的權限解析結果
#[derive(Debug, Clone, Serialize)]
pub struct FieldPermission {
    pub key: String,
    pub permission: Permission,
    pub required: bool,
    /// 為何是這個結果，供 UI 顯示與除錯
    pub reason: &'static str,
}

/// 解析整張表單在特定情境下的欄位權限
pub fn resolve_form(content: &Value, ctx: &Context) -> Vec<FieldPermission> {
    let Some(fields) = content.get("fields").and_then(Value::as_array) else {
        return Vec::new();
    };

    fields.iter().map(|f| resolve_field(f, ctx)).collect()
}

/// 解析單一欄位
pub fn resolve_field(field: &Value, ctx: &Context) -> FieldPermission {
    let key = field
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let (permission, reason) = compute(field, ctx);
    let required = permission != Permission::Hidden && is_required(field, ctx);

    FieldPermission {
        key,
        permission,
        required,
        reason,
    }
}

fn compute(field: &Value, ctx: &Context) -> (Permission, &'static str) {
    // 外部參與者：external_visible = false 的欄位完全隱藏。
    // 這是報價單「成本、毛利不給客戶看」的實作。
    if ctx.is_external()
        && field.pointer("/ui/external_visible").and_then(Value::as_bool) == Some(false)
    {
        (Permission::Hidden, "外部參與者不可見")
    }
    // 計算欄位一律唯讀，即使是 admin。使用者改了也會被後端重算覆蓋。
    else if field.pointer("/data/computed").is_some() {
        (Permission::Readonly, "系統計算欄位")
    }
    // 角色不在可讀清單
    else if !role_allowed(field, "readable_roles", ctx) {
        (Permission::Hidden, "角色不在可讀清單")
    }
    // 條件式隱藏
    else if let Some(expr) = field.pointer("/workflow/visible_when").and_then(Value::as_str) {
        if eval_bool(expr, ctx) {
            readonly_or_editable(field, ctx)
        } else {
            (Permission::Hidden, "不符合顯示條件")
        }
    } else {
        readonly_or_editable(field, ctx)
    }
}

fn readonly_or_editable(field: &Value, ctx: &Context) -> (Permission, &'static str) {
    // admin 永遠可編輯，不受條件限制。
    // 設計稿 _7 的角色清單中 admin 標示為「永遠具備可編輯屬性」。
    if ctx.is_admin() {
        return (Permission::Editable, "管理員");
    }

    // 外部參與者一律唯讀。客戶只簽核，不改內容。
    if ctx.is_external() {
        return (Permission::Readonly, "外部參與者唯讀");
    }

    if !role_allowed(field, "editable_roles", ctx) {
        return (Permission::Readonly, "角色不在可編輯清單");
    }

    if let Some(expr) = field.pointer("/workflow/readonly_when").and_then(Value::as_str) {
        if eval_bool(expr, ctx) {
            return (Permission::Readonly, "符合唯讀條件");
        }
    }

    (Permission::Editable, "預設可編輯")
}

/// 角色清單檢查
///
/// 清單省略時代表「不限制」，這是刻意的預設。
/// 若預設為「全部拒絕」，每個欄位都得列舉所有角色，實務上難以維護。
fn role_allowed(field: &Value, list_key: &str, ctx: &Context) -> bool {
    if ctx.is_admin() {
        return true;
    }
    let Some(list) = field
        .pointer(&format!("/workflow/{list_key}"))
        .and_then(Value::as_array)
    else {
        return true;
    };
    if list.is_empty() {
        return true;
    }
    list.iter()
        .filter_map(Value::as_str)
        .any(|r| ctx.has_role(r))
}

fn is_required(field: &Value, ctx: &Context) -> bool {
    field
        .pointer("/workflow/required_when")
        .and_then(Value::as_str)
        .is_some_and(|expr| eval_bool(expr, ctx))
}

/// 簡化的布林表達式求值
///
/// 支援矩陣 UI 會產生的形式：
///   true / false
///   node.id == 'x'        node.id != 'x'
///   node.id in ['a','b']  node.id not in ['a','b']
///   <path> > <number>     以及 < >= <= == !=
///   <expr> && <expr>      <expr> || <expr>
///
/// 完整 CEL 實作排在任務 2.2。此處刻意保持簡單，
/// 遇到看不懂的表達式回 false（fail-closed），
/// 寧可少給權限也不要多給。
pub fn eval_bool(expr: &str, ctx: &Context) -> bool {
    let e = expr.trim();

    if e.eq_ignore_ascii_case("true") {
        return true;
    }
    if e.eq_ignore_ascii_case("false") {
        return false;
    }

    // 先處理 || 再處理 &&，因為 || 優先序較低
    if let Some((l, r)) = split_top(e, "||") {
        return eval_bool(&l, ctx) || eval_bool(&r, ctx);
    }
    if let Some((l, r)) = split_top(e, "&&") {
        return eval_bool(&l, ctx) && eval_bool(&r, ctx);
    }

    // 去掉外層括號後重試
    if e.starts_with('(') && e.ends_with(')') && balanced(&e[1..e.len() - 1]) {
        return eval_bool(&e[1..e.len() - 1], ctx);
    }

    eval_comparison(e, ctx)
}

fn eval_comparison(e: &str, ctx: &Context) -> bool {
    // in / not in
    if let Some(idx) = e.find(" not in ") {
        let (lhs, rhs) = (&e[..idx], &e[idx + 8..]);
        return !in_list(lhs.trim(), rhs.trim(), ctx);
    }
    if let Some(idx) = e.find(" in ") {
        let (lhs, rhs) = (&e[..idx], &e[idx + 4..]);
        return in_list(lhs.trim(), rhs.trim(), ctx);
    }

    // 比較運算子，長的先比避免 >= 被當成 >
    for op in ["==", "!=", ">=", "<=", ">", "<"] {
        if let Some(idx) = e.find(op) {
            let lhs = e[..idx].trim();
            let rhs = e[idx + op.len()..].trim();
            return compare(lhs, op, rhs, ctx);
        }
    }

    false
}

fn compare(lhs: &str, op: &str, rhs: &str, ctx: &Context) -> bool {
    let l = resolve(lhs, ctx);
    let r = resolve(rhs, ctx);

    // 數值比較
    if let (Some(a), Some(b)) = (as_number(&l), as_number(&r)) {
        return match op {
            "==" => (a - b).abs() < f64::EPSILON,
            "!=" => (a - b).abs() >= f64::EPSILON,
            ">" => a > b,
            "<" => a < b,
            ">=" => a >= b,
            "<=" => a <= b,
            _ => false,
        };
    }

    // 字串比較只支援等於與不等於
    let ls = as_string(&l);
    let rs = as_string(&r);
    match op {
        "==" => ls == rs,
        "!=" => ls != rs,
        _ => false,
    }
}

fn in_list(lhs: &str, rhs: &str, ctx: &Context) -> bool {
    let value = as_string(&resolve(lhs, ctx));
    let inner = rhs.trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|s| s.trim().trim_matches(['\'', '"']))
        .any(|s| s == value)
}

/// 解析識別字為值
fn resolve(token: &str, ctx: &Context) -> Value {
    let t = token.trim();

    // 字面字串
    if (t.starts_with('\'') && t.ends_with('\'')) || (t.starts_with('"') && t.ends_with('"')) {
        return Value::String(t[1..t.len() - 1].to_string());
    }
    // 字面數字
    if let Ok(n) = t.parse::<f64>() {
        return Value::from(n);
    }

    match t {
        "node.id" => ctx
            .node_id
            .map_or(Value::Null, |v| Value::String(v.to_string())),
        "user.kind" => Value::String(ctx.participant_kind.to_string()),
        _ => {
            // 業務資料路徑，例如 quotation.total
            let pointer = format!("/{}", t.replace('.', "/"));
            ctx.data.pointer(&pointer).cloned().unwrap_or(Value::Null)
        }
    }
}

fn as_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn as_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// 在括號外層尋找運算子並切開
fn split_top(e: &str, op: &str) -> Option<(String, String)> {
    let bytes = e.as_bytes();
    let mut depth = 0i32;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth == 0 && e[i..].starts_with(op) {
            return Some((e[..i].to_string(), e[i + op.len()..].to_string()));
        }
        i += 1;
    }
    None
}

fn balanced(s: &str) -> bool {
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx<'a>(node: Option<&'a str>, roles: &'a [String], data: &'a Value) -> Context<'a> {
        Context {
            node_id: node,
            roles,
            participant_kind: "internal",
            data,
        }
    }

    fn roles(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn plain_field_is_editable() {
        let f = json!({ "key": "name", "ui": {}, "data": { "path": "a.b" } });
        let r = roles(&["requester"]);
        let d = json!({});
        assert_eq!(resolve_field(&f, &ctx(None, &r, &d)).permission, Permission::Editable);
    }

    #[test]
    fn computed_field_is_readonly_even_for_admin() {
        let f = json!({
            "key": "total",
            "data": { "path": "q.total", "computed": "sum(lines.amount)" }
        });
        let r = roles(&["admin"]);
        let d = json!({});
        let result = resolve_field(&f, &ctx(None, &r, &d));
        assert_eq!(result.permission, Permission::Readonly);
        assert_eq!(result.reason, "系統計算欄位");
    }

    #[test]
    fn readonly_when_node_not_in_list() {
        let f = json!({
            "key": "discount",
            "workflow": { "readonly_when": "node.id not in ['start','revise']" }
        });
        let r = roles(&["requester"]);
        let d = json!({});

        assert_eq!(
            resolve_field(&f, &ctx(Some("start"), &r, &d)).permission,
            Permission::Editable,
            "在 start 節點應可編輯"
        );
        assert_eq!(
            resolve_field(&f, &ctx(Some("manager_approval"), &r, &d)).permission,
            Permission::Readonly,
            "在簽核節點應唯讀"
        );
    }

    #[test]
    fn external_cannot_see_internal_only_field() {
        let f = json!({
            "key": "margin",
            "ui": { "external_visible": false },
            "data": { "path": "q.margin" }
        });
        let r = roles(&[]);
        let d = json!({});
        let c = Context {
            node_id: Some("customer_review"),
            roles: &r,
            participant_kind: "external",
            data: &d,
        };
        assert_eq!(resolve_field(&f, &c).permission, Permission::Hidden);
    }

    #[test]
    fn external_is_always_readonly() {
        let f = json!({ "key": "note", "data": { "path": "q.note" } });
        let r = roles(&[]);
        let d = json!({});
        let c = Context {
            node_id: Some("customer_review"),
            roles: &r,
            participant_kind: "external",
            data: &d,
        };
        assert_eq!(resolve_field(&f, &c).permission, Permission::Readonly);
    }

    #[test]
    fn role_not_in_editable_list_gets_readonly() {
        let f = json!({
            "key": "price",
            "workflow": { "editable_roles": ["designer"] }
        });
        let r = roles(&["viewer"]);
        let d = json!({});
        assert_eq!(
            resolve_field(&f, &ctx(None, &r, &d)).permission,
            Permission::Readonly
        );
    }

    #[test]
    fn role_not_in_readable_list_gets_hidden() {
        let f = json!({
            "key": "cost",
            "workflow": { "readable_roles": ["admin", "finance_manager"] }
        });
        let r = roles(&["requester"]);
        let d = json!({});
        assert_eq!(
            resolve_field(&f, &ctx(None, &r, &d)).permission,
            Permission::Hidden
        );
    }

    #[test]
    fn conditional_permission_by_amount() {
        // 設計稿 _7 的 RULE_01：金額大於一百萬時唯讀
        let f = json!({
            "key": "discount_rate",
            "workflow": { "readonly_when": "quotation.total > 1000000" }
        });
        let r = roles(&["requester"]);

        let small = json!({ "quotation": { "total": 900000 } });
        assert_eq!(
            resolve_field(&f, &ctx(None, &r, &small)).permission,
            Permission::Editable
        );

        let large = json!({ "quotation": { "total": 1200000 } });
        assert_eq!(
            resolve_field(&f, &ctx(None, &r, &large)).permission,
            Permission::Readonly
        );
    }

    #[test]
    fn and_or_expressions() {
        let r = roles(&["requester"]);
        let d = json!({ "q": { "total": 500 } });
        let c = ctx(Some("start"), &r, &d);

        assert!(eval_bool("node.id == 'start' && q.total < 1000", &c));
        assert!(!eval_bool("node.id == 'start' && q.total > 1000", &c));
        assert!(eval_bool("node.id == 'other' || q.total < 1000", &c));
    }

    #[test]
    fn unknown_expression_fails_closed() {
        let r = roles(&["requester"]);
        let d = json!({});
        // 看不懂的表達式回 false，寧可少給權限
        assert!(!eval_bool("someFunc(x) matches /re/", &ctx(None, &r, &d)));
    }

    #[test]
    fn required_only_when_visible() {
        let f = json!({
            "key": "reason",
            "workflow": {
                "visible_when": "false",
                "required_when": "true"
            }
        });
        let r = roles(&["requester"]);
        let d = json!({});
        let result = resolve_field(&f, &ctx(None, &r, &d));
        assert_eq!(result.permission, Permission::Hidden);
        assert!(!result.required, "隱藏的欄位不該是必填");
    }

    #[test]
    fn resolves_whole_form() {
        let content = json!({
            "fields": [
                { "key": "a", "data": { "path": "x.a" } },
                { "key": "b", "data": { "path": "x.b", "computed": "1+1" } },
                { "key": "c", "ui": { "external_visible": false }, "data": { "path": "x.c" } }
            ]
        });
        let r = roles(&["requester"]);
        let d = json!({});
        let result = resolve_form(&content, &ctx(None, &r, &d));

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].permission, Permission::Editable);
        assert_eq!(result[1].permission, Permission::Readonly);
        assert_eq!(result[2].permission, Permission::Editable, "內部使用者仍看得到");
    }
}
