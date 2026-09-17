//! 流程 DSL 圖結構驗證
//!
//! JSON Schema 只能驗單一節點的結構，驗不了跨節點的語意。
//! 本模組實作 WF-E001 到 E008，與 schemas/tools/graph-check.mjs
//! 的參考實作必須對同一組 fixture 產生相同的錯誤碼集合。
//!
//! 最容易踩到的設計細節（見 schemas/README.md）：
//!   流程有兩種控制流。edges 是正常前進路徑，on_reject 與 on_failure
//!   的 goto 是退回路徑。
//!
//!   可達性分析（E002）必須包含兩者，否則只靠 goto 進入的節點
//!   會被誤判為孤立。報價單的 revise 節點正是如此。
//!
//!   上游判斷（E004）只能看 edges，不能含 goto。否則 goto 會讓
//!   目標自動成為上游，規則形同虛設。

use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GraphError {
    pub code: &'static str,
    pub node_id: Option<String>,
    pub message: String,
}

impl GraphError {
    fn new(code: &'static str, node_id: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            code,
            node_id: node_id.map(str::to_owned),
            message: message.into(),
        }
    }
}

/// 驗證所需的租戶資料
///
/// 正式使用時由呼叫端從資料庫載入。測試可直接建構。
#[derive(Debug, Default)]
pub struct ValidationContext {
    pub known_roles: HashSet<String>,
    pub known_actions: HashSet<String>,
    /// 業務物件的合法資料路徑。空集合代表不檢查。
    pub known_paths: HashSet<String>,
}

impl ValidationContext {
    pub fn with_roles(roles: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            known_roles: roles.into_iter().map(Into::into).collect(),
            ..Default::default()
        }
    }

    pub fn with_actions(mut self, actions: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.known_actions = actions.into_iter().map(Into::into).collect();
        self
    }

    /// 業務物件的合法資料路徑，來源為 `schemas/business-objects/*.json`
    ///
    /// 刻意不從表單定義推導：同一個 business_object 可以有多張表單，
    /// 而實測顯示它們的詞彙互不相容（一張叫 `quotation.total`、
    /// 另一張叫 `quotation.total_amount`）。取聯集會讓拼錯字合法，
    /// 而拼錯字正是 E012 要擋的東西。
    pub fn with_paths(mut self, paths: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.known_paths = paths.into_iter().map(Into::into).collect();
        self
    }
}

/// 鄰接表
///
/// 兩組：一組含 goto 供可達性分析，一組僅 edges 供上游判斷。
struct Graph<'a> {
    /// 含 goto
    out: HashMap<&'a str, Vec<Edge<'a>>>,
    inn: HashMap<&'a str, Vec<&'a str>>,
    /// 僅 edges
    inn_fwd: HashMap<&'a str, Vec<&'a str>>,
}

struct Edge<'a> {
    to: &'a str,
    when: Option<bool>,
}

pub fn validate_graph(dsl: &Value, ctx: &ValidationContext) -> Vec<GraphError> {
    let mut errors = Vec::new();

    let Some(nodes) = dsl.get("nodes").and_then(Value::as_array) else {
        errors.push(GraphError::new("WF-E001", None, "缺少 nodes"));
        return errors;
    };

    let by_id: HashMap<&str, &Value> = nodes
        .iter()
        .filter_map(|n| n.get("id").and_then(Value::as_str).map(|id| (id, n)))
        .collect();

    let graph = build_graph(dsl, &by_id, &mut errors);

    check_endpoints(nodes, &mut errors);
    check_reachability(nodes, &by_id, &graph, &mut errors);
    check_goto_targets(nodes, &by_id, &graph, &mut errors);
    check_parallel(nodes, &by_id, &graph, &mut errors);

    for node in nodes {
        check_node(node, &graph, ctx, &mut errors);
    }

    errors
}

fn build_graph<'a>(
    dsl: &'a Value,
    by_id: &HashMap<&'a str, &'a Value>,
    errors: &mut Vec<GraphError>,
) -> Graph<'a> {
    let mut out: HashMap<&str, Vec<Edge>> = by_id.keys().map(|k| (*k, Vec::new())).collect();
    let mut inn: HashMap<&str, Vec<&str>> = by_id.keys().map(|k| (*k, Vec::new())).collect();
    let mut inn_fwd: HashMap<&str, Vec<&str>> = by_id.keys().map(|k| (*k, Vec::new())).collect();

    if let Some(edges) = dsl.get("edges").and_then(Value::as_array) {
        for e in edges {
            let Some(arr) = e.as_array() else { continue };
            let (Some(from), Some(to)) = (
                arr.first().and_then(Value::as_str),
                arr.get(1).and_then(Value::as_str),
            ) else {
                continue;
            };

            if !by_id.contains_key(from) {
                errors.push(GraphError::new(
                    "WF-E002",
                    Some(from),
                    format!("邊的起點節點不存在：{from}"),
                ));
                continue;
            }
            if !by_id.contains_key(to) {
                errors.push(GraphError::new(
                    "WF-E002",
                    Some(to),
                    format!("邊的終點節點不存在：{to}"),
                ));
                continue;
            }

            let when = arr.get(2).and_then(|m| m.get("when")).and_then(Value::as_bool);
            out.get_mut(from).expect("已檢查存在").push(Edge { to, when });
            inn.get_mut(to).expect("已檢查存在").push(from);
            inn_fwd.get_mut(to).expect("已檢查存在").push(from);
        }
    }

    // goto 併入可達性用的鄰接表，但不進 inn_fwd
    for (id, node) in by_id {
        for key in ["on_reject", "on_failure"] {
            let Some(p) = node.get(key) else { continue };
            if p.get("action").and_then(Value::as_str) != Some("goto") {
                continue;
            }
            let Some(target) = p.get("node").and_then(Value::as_str) else {
                continue;
            };
            if !by_id.contains_key(target) {
                continue; // 由 E004 回報
            }
            out.get_mut(id).expect("節點存在").push(Edge {
                to: target,
                when: None,
            });
            inn.get_mut(target).expect("已檢查存在").push(id);
        }
    }

    Graph { out, inn, inn_fwd }
}

fn check_endpoints(nodes: &[Value], errors: &mut Vec<GraphError>) {
    let count = |t: &str| {
        nodes
            .iter()
            .filter(|n| n.get("type").and_then(Value::as_str) == Some(t))
            .count()
    };

    let triggers = count("trigger");
    if triggers != 1 {
        errors.push(GraphError::new(
            "WF-E001",
            None,
            format!("trigger 節點應恰有 1 個，實際 {triggers} 個"),
        ));
    }
    if count("end") < 1 {
        errors.push(GraphError::new("WF-E001", None, "end 節點至少需 1 個"));
    }
}

/// WF-E009：parallel 節點與 join 策略的合法性
/// WF-E010：分支必須匯合，join 必須有對應的 parallel
///
/// 執行期也會檢查這些（interpreter 的 NoBranches / NoJoinNode），
/// 但那時使用者可能已經簽了一半。設計器就該擋下來。
fn check_parallel(
    nodes: &[Value],
    by_id: &HashMap<&str, &Value>,
    graph: &Graph<'_>,
    errors: &mut Vec<GraphError>,
) {
    let parallels: Vec<&str> = nodes
        .iter()
        .filter(|n| n.get("type").and_then(Value::as_str) == Some("parallel"))
        .filter_map(|n| n.get("id").and_then(Value::as_str))
        .collect();

    let joins: Vec<&str> = nodes
        .iter()
        .filter(|n| n.get("type").and_then(Value::as_str) == Some("join"))
        .filter_map(|n| n.get("id").and_then(Value::as_str))
        .collect();

    let mut matched_joins: HashSet<&str> = HashSet::new();

    for pid in &parallels {
        let branches: Vec<&str> = graph
            .out
            .get(pid)
            .map(|es| es.iter().map(|e| e.to).collect())
            .unwrap_or_default();

        // 單一分支的 parallel 沒有意義，多半是設計時漏接了。
        if branches.len() < 2 {
            errors.push(GraphError::new(
                "WF-E009",
                Some(pid),
                format!("parallel 需要至少 2 個分支，目前有 {}", branches.len()),
            ));
            continue;
        }

        // 每個分支都要走到同一個 join
        let mut join_ids: Vec<&str> = Vec::new();
        for b in &branches {
            match find_join_from(b, by_id, graph) {
                Some(j) => join_ids.push(j),
                None => errors.push(GraphError::new(
                    "WF-E010",
                    Some(b),
                    format!("分支 {b} 沒有匯合到 join 節點"),
                )),
            }
        }

        if join_ids.is_empty() {
            continue;
        }

        let first = join_ids[0];
        if join_ids.iter().any(|j| *j != first) {
            errors.push(GraphError::new(
                "WF-E010",
                Some(pid),
                "各分支匯合到不同的 join 節點".to_string(),
            ));
            continue;
        }
        matched_joins.insert(first);

        // join 策略的合法性
        if let Some(join_node) = by_id.get(first) {
            check_join_policy(first, join_node, branches.len(), errors);
            check_join_timeout(first, join_node, errors);
        }
    }

    // 沒有 parallel 卻有 join
    for jid in joins {
        if !matched_joins.contains(jid) {
            errors.push(GraphError::new(
                "WF-E010",
                Some(jid),
                format!("join 節點 {jid} 沒有對應的 parallel"),
            ));
        }
    }
}

/// 從分支起點往下找 join，廣度優先
fn find_join_from<'a>(
    start: &'a str,
    by_id: &HashMap<&'a str, &'a Value>,
    graph: &Graph<'a>,
) -> Option<&'a str> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut queue: Vec<&str> = vec![start];

    while let Some(current) = queue.pop() {
        if !seen.insert(current) {
            continue;
        }
        if let Some(node) = by_id.get(current) {
            if node.get("type").and_then(Value::as_str) == Some("join") {
                return by_id.get_key_value(current).map(|(k, _)| *k);
            }
        }
        if let Some(edges) = graph.out.get(current) {
            queue.extend(edges.iter().map(|e| e.to));
        }
    }
    None
}

fn check_join_policy(
    join_id: &str,
    node: &Value,
    branch_count: usize,
    errors: &mut Vec<GraphError>,
) {
    let Some(join) = node.get("join") else {
        errors.push(GraphError::new(
            "WF-E009",
            Some(join_id),
            "join 節點缺少 join 策略",
        ));
        return;
    };

    let completion = join.get("completion").and_then(Value::as_str).unwrap_or("ALL");
    if completion != "N_OF_M" {
        return;
    }

    let Some(n) = join.get("n").and_then(Value::as_u64) else {
        errors.push(GraphError::new(
            "WF-E009",
            Some(join_id),
            "completion=N_OF_M 時必須指定 n",
        ));
        return;
    };

    // n 大於分支數的話條件永遠滿足不了，流程會永久卡住
    if n as usize > branch_count {
        errors.push(GraphError::new(
            "WF-E009",
            Some(join_id),
            format!("n={n} 大於分支數 {branch_count}，條件永遠無法滿足"),
        ));
    }
}

/// WF-E009：join 的逾時策略
///
/// join 不支援 ESCALATE——分支各有自己的簽核人，加簽給誰沒有明確語意。
/// 猜一個對象比明確失敗更糟：使用者以為設好了，實際上加簽給了不預期的人。
fn check_join_timeout(join_id: &str, node: &Value, errors: &mut Vec<GraphError>) {
    let Some(timeout) = node.get("timeout") else {
        return;
    };
    if timeout.get("policy").and_then(Value::as_str) == Some("ESCALATE") {
        errors.push(GraphError::new(
            "WF-E009",
            Some(join_id),
            "join 不支援 ESCALATE，請改在各分支的簽核節點設定",
        ));
    }
}

/// WF-E011：ESCALATE 必須指定加簽對象
///
/// 缺 to 的話執行期才會失敗，而那時使用者已經等了設定的天數。
fn check_escalate_target(node: &Value, errors: &mut Vec<GraphError>) {
    let Some(timeout) = node.get("timeout") else {
        return;
    };
    if timeout.get("policy").and_then(Value::as_str) != Some("ESCALATE") {
        return;
    }
    if timeout.get("to").is_none() {
        let id = node.get("id").and_then(Value::as_str);
        errors.push(GraphError::new(
            "WF-E011",
            id,
            "timeout policy 為 ESCALATE 時必須指定加簽對象 to",
        ));
    }
}

fn check_reachability(
    nodes: &[Value],
    by_id: &HashMap<&str, &Value>,
    graph: &Graph,
    errors: &mut Vec<GraphError>,
) {
    let trigger = nodes
        .iter()
        .find(|n| n.get("type").and_then(Value::as_str) == Some("trigger"))
        .and_then(|n| n.get("id"))
        .and_then(Value::as_str);
    let Some(start) = trigger else { return };

    let ends: Vec<&str> = nodes
        .iter()
        .filter(|n| n.get("type").and_then(Value::as_str) == Some("end"))
        .filter_map(|n| n.get("id").and_then(Value::as_str))
        .collect();
    if ends.is_empty() {
        return;
    }

    let forward = reachable(start, |id| {
        graph
            .out
            .get(id)
            .map(|v| v.iter().map(|e| e.to).collect::<Vec<_>>())
            .unwrap_or_default()
    });

    let mut backward = HashSet::new();
    for e in &ends {
        for id in reachable(e, |id| graph.inn.get(id).cloned().unwrap_or_default()) {
            backward.insert(id);
        }
    }

    for id in by_id.keys() {
        if !forward.contains(id) {
            errors.push(GraphError::new(
                "WF-E002",
                Some(id),
                format!("節點無法從 trigger 到達：{id}"),
            ));
        } else if !backward.contains(id) {
            errors.push(GraphError::new(
                "WF-E002",
                Some(id),
                format!("節點無法到達任何 end：{id}"),
            ));
        }
    }
}

fn check_goto_targets(
    nodes: &[Value],
    by_id: &HashMap<&str, &Value>,
    graph: &Graph,
    errors: &mut Vec<GraphError>,
) {
    for node in nodes {
        let Some(id) = node.get("id").and_then(Value::as_str) else {
            continue;
        };

        for key in ["on_reject", "on_failure"] {
            let Some(p) = node.get(key) else { continue };
            if p.get("action").and_then(Value::as_str) != Some("goto") {
                continue;
            }

            let Some(target) = p.get("node").and_then(Value::as_str) else {
                errors.push(GraphError::new(
                    "WF-E004",
                    Some(id),
                    format!("{key} 為 goto 但未指定 node"),
                ));
                continue;
            };

            if !by_id.contains_key(target) {
                errors.push(GraphError::new(
                    "WF-E004",
                    Some(id),
                    format!("{key} 目標節點不存在：{target}"),
                ));
                continue;
            }

            // 上游判斷只看正向 edges。含 goto 會讓目標自己變成上游。
            let upstream = reachable(id, |n| graph.inn_fwd.get(n).cloned().unwrap_or_default());
            if !upstream.contains(target) {
                errors.push(GraphError::new(
                    "WF-E004",
                    Some(id),
                    format!("{key} 目標「{target}」不在節點「{id}」的上游路徑"),
                ));
            }
        }
    }
}

fn check_node(
    node: &Value,
    graph: &Graph,
    ctx: &ValidationContext,
    errors: &mut Vec<GraphError>,
) {
    let Some(id) = node.get("id").and_then(Value::as_str) else {
        return;
    };
    let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");

    // E011：ESCALATE 必須有加簽對象。join 的 ESCALATE 由 check_join_timeout
    // 擋下（不支援），這裡管的是簽核節點。
    if node_type != "join" {
        check_escalate_target(node, errors);
    }

    // E003：condition 需要 true 與 false 兩條出邊
    if node_type == "condition" {
        let outs = graph.out.get(id).map(Vec::as_slice).unwrap_or(&[]);
        let has_true = outs.iter().any(|e| e.when == Some(true));
        let has_false = outs.iter().any(|e| e.when == Some(false));

        if !has_true || !has_false {
            let missing = match (has_true, has_false) {
                (false, false) => "when=true 與 when=false",
                (false, true) => "when=true",
                _ => "when=false",
            };
            errors.push(GraphError::new(
                "WF-E003",
                Some(id),
                format!("condition 節點「{id}」缺少 {missing} 分支"),
            ));
        }
    }

    // E005：角色存在
    if !ctx.known_roles.is_empty() {
        for role in collect_roles(node) {
            if !ctx.known_roles.contains(&role) {
                errors.push(GraphError::new(
                    "WF-E005",
                    Some(id),
                    format!("角色不存在：{role}"),
                ));
            }
        }
    }

    // E006：表達式可解析
    for expr in collect_expressions(node) {
        if !expression_parses(&expr) {
            errors.push(GraphError::new(
                "WF-E006",
                Some(id),
                format!("表達式無法解析：{expr}"),
            ));
        }
    }

    // E012：引用的資料路徑存在於業務物件定義
    //
    // 擋的是「靜默跳過簽核」：條件式路徑打錯時求值取不到值，
    // 被當成 false，流程照跑只是少走一段簽核而無任何錯誤訊息。
    // 人工編輯時使用者至少會看 JSON，AI 產生時不會。
    //
    // 沿用 known_roles / known_actions 的慣例：空集合代表不檢查。
    if !ctx.known_paths.is_empty() {
        let from_expr = collect_expressions(node)
            .iter()
            .flat_map(|e| extract_paths_from_expression(e))
            .collect::<Vec<_>>();

        for path in from_expr.into_iter().chain(collect_data_paths(node)) {
            if !ctx.known_paths.contains(&path) {
                errors.push(GraphError::new(
                    "WF-E012",
                    Some(id),
                    format!("資料路徑不存在於業務物件定義：{path}"),
                ));
            }
        }
    }

    // E007：action 已註冊，且寫入外部系統者需 idempotency_key
    if node_type == "action" {
        let action = node.get("action").and_then(Value::as_str).unwrap_or("");

        if !ctx.known_actions.is_empty() && !ctx.known_actions.contains(action) {
            errors.push(GraphError::new(
                "WF-E007",
                Some(id),
                format!("action 不存在於 Registry：{action}"),
            ));
        }

        let writes_external = action.starts_with("odoo.") || action.starts_with("erp.");
        if writes_external && node.get("idempotency_key").is_none() {
            errors.push(GraphError::new(
                "WF-E007",
                Some(id),
                format!("寫入外部系統的 action「{action}」缺少 idempotency_key"),
            ));
        }
    }

    // E008：外部參與者的 resolver 型別
    if node.get("participant").and_then(Value::as_str) == Some("external") {
        let resolver_type = node
            .get("resolver")
            .or_else(|| node.get("assignee"))
            .and_then(|r| r.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("(未指定)");

        if resolver_type != "external_contacts" {
            errors.push(GraphError::new(
                "WF-E008",
                Some(id),
                format!(
                    "participant=external 的 resolver 必須是 external_contacts，實際為 {resolver_type}"
                ),
            ));
        }
    }
}

// ── 輔助 ────────────────────────────────────────────────

fn reachable<'a, F>(start: &'a str, neighbors: F) -> HashSet<&'a str>
where
    F: Fn(&str) -> Vec<&'a str>,
{
    let mut seen = HashSet::new();
    let mut stack = vec![start];
    while let Some(cur) = stack.pop() {
        if !seen.insert(cur) {
            continue;
        }
        stack.extend(neighbors(cur));
    }
    seen
}

/// 收集節點內所有 role 值，含 composite 遞迴
fn collect_roles(node: &Value) -> Vec<String> {
    let mut acc = Vec::new();
    for key in ["resolver", "assignee"] {
        walk_resolver_roles(node.get(key), &mut acc);
    }
    walk_resolver_roles(node.pointer("/timeout/to"), &mut acc);
    acc
}

fn walk_resolver_roles(r: Option<&Value>, acc: &mut Vec<String>) {
    let Some(r) = r else { return };

    match r.get("type").and_then(Value::as_str) {
        Some("role") => {
            if let Some(v) = r.get("value").and_then(Value::as_str) {
                acc.push(v.to_string());
            }
        }
        Some("composite") => {
            if let Some(items) = r.get("of").and_then(Value::as_array) {
                for item in items {
                    walk_resolver_roles(item.get("spec"), acc);
                }
            }
        }
        _ => {}
    }
}

/// resolver 直接以 `path` 指定資料位置者（如 external_contacts）
///
/// 與 `collect_expressions` 分開：那支收的是表達式字串，
/// 這支收的是直接當成路徑用的欄位值，兩者的取用方式不同。
fn collect_data_paths(node: &Value) -> Vec<String> {
    let mut acc = Vec::new();

    for key in ["resolver", "assignee"] {
        walk_resolver_paths(node.get(key), &mut acc);
    }
    walk_resolver_paths(node.pointer("/timeout/to"), &mut acc);

    acc
}

fn walk_resolver_paths(r: Option<&Value>, acc: &mut Vec<String>) {
    let Some(r) = r else { return };

    if let Some(p) = r.get("path").and_then(Value::as_str) {
        acc.push(p.to_string());
    }

    // composite 的子項也可能各自帶 path
    if r.get("type").and_then(Value::as_str) == Some("composite") {
        if let Some(items) = r.get("of").and_then(Value::as_array) {
            for item in items {
                walk_resolver_paths(item.get("spec"), acc);
            }
        }
    }
}

/// 從 CEL 表達式抽出資料路徑
///
/// 刻意不做完整 CEL 解析，理由與 `expression_parses` 相同：
/// 看不懂的一律放行。誤擋會讓使用者無法發布正確的流程，
/// 比漏擋更快失去信任。
///
/// 只認「識別字.識別字(.識別字)*」的形態，並排除：
///   - 字串字面值內的內容（先把引號內的段落挖掉）
///   - 後面緊接 `(` 的識別字（CEL 內建函式如 has()、size()）
///   - 純數字開頭的 token（0.15 這類小數）
fn extract_paths_from_expression(expr: &str) -> Vec<String> {
    let stripped = strip_string_literals(expr);
    let bytes = stripped.as_bytes();
    let mut acc = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        let c = bytes[i] as char;

        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
                    i += 1;
                } else {
                    break;
                }
            }
            let token = stripped[start..i].trim_end_matches('.');

            // 後面緊接 ( 的是函式呼叫，不是路徑
            let is_call = stripped[i..].trim_start().starts_with('(');

            if !is_call && token.contains('.') {
                acc.push(token.to_string());
            }
            continue;
        }

        // 數字開頭的 token 整段跳過，避免 0.15 被拆出 .15
        if c.is_ascii_digit() {
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if ch.is_ascii_alphanumeric() || ch == '.' {
                    i += 1;
                } else {
                    break;
                }
            }
            continue;
        }

        i += 1;
    }

    acc
}

/// 把單引號與雙引號內的內容換成空白
///
/// 字串字面值裡的 `TWD.dollar` 不是資料路徑。
fn strip_string_literals(expr: &str) -> String {
    let mut out = String::with_capacity(expr.len());
    let mut quote: Option<char> = None;

    for c in expr.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
                out.push(' ');
            }
            None => {
                if c == '\'' || c == '"' {
                    quote = Some(c);
                    out.push(' ');
                } else {
                    out.push(c);
                }
            }
        }
    }

    out
}

fn collect_expressions(node: &Value) -> Vec<String> {
    let mut acc = Vec::new();

    if node.get("type").and_then(Value::as_str) == Some("condition") {
        if let Some(e) = node.get("expression").and_then(Value::as_str) {
            acc.push(e.to_string());
        }
    }

    for key in ["resolver", "assignee"] {
        walk_resolver_expressions(node.get(key), &mut acc);
    }
    walk_resolver_expressions(node.pointer("/timeout/to"), &mut acc);

    acc
}

fn walk_resolver_expressions(r: Option<&Value>, acc: &mut Vec<String>) {
    let Some(r) = r else { return };

    match r.get("type").and_then(Value::as_str) {
        Some("expression") => {
            if let Some(cel) = r.get("cel").and_then(Value::as_str) {
                acc.push(cel.to_string());
            }
        }
        Some("composite") => {
            if let Some(items) = r.get("of").and_then(Value::as_array) {
                for item in items {
                    if let Some(w) = item.get("when").and_then(Value::as_str) {
                        acc.push(w.to_string());
                    }
                    walk_resolver_expressions(item.get("spec"), acc);
                }
            }
        }
        _ => {}
    }
}

/// 粗略的表達式語法檢查
///
/// 完整 CEL 解析排在任務 2.2。此處只擋明顯錯誤：
/// 括號不對稱、單等號比較。看不懂的一律放行，
/// 避免誤擋合法但複雜的表達式。
fn expression_parses(expr: &str) -> bool {
    let e = expr.trim();
    if e.is_empty() {
        return false;
    }

    let mut depth = 0i32;
    for c in e.chars() {
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
    if depth != 0 {
        return false;
    }

    // 單等號比較是常見筆誤。== != <= >= 都是合法的。
    let bytes = e.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] != b'=' {
            continue;
        }
        let prev_ok = i > 0 && matches!(bytes[i - 1], b'=' | b'!' | b'<' | b'>');
        let next_ok = i + 1 < bytes.len() && bytes[i + 1] == b'=';
        if !prev_ok && !next_ok {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> ValidationContext {
        ValidationContext::with_roles([
            "admin", "designer", "approver", "requester", "viewer",
            "finance_manager", "sales_director", "cfo",
        ])
        .with_actions([
            "quotation.publish",
            "odoo.create_sale_order",
            "odoo.create_purchase_order",
        ])
    }

    fn quotation_dsl() -> Value {
        let raw = include_str!("../../../../schemas/fixtures/quotation_approval_v1.json");
        serde_json::from_str(raw).expect("fixture 應為合法 JSON")
    }

    fn purchase_dsl() -> Value {
        let raw = include_str!("../../../../schemas/fixtures/purchase_approval_v1.json");
        serde_json::from_str(raw).expect("fixture 應為合法 JSON")
    }

    fn codes(errors: &[GraphError]) -> Vec<&str> {
        errors.iter().map(|e| e.code).collect()
    }

    // ── 正向 ────────────────────────────────────────────

    #[test]
    fn quotation_fixture_passes() {
        let errors = validate_graph(&quotation_dsl(), &ctx());
        assert!(errors.is_empty(), "報價單 fixture 不該有錯誤：{errors:?}");
    }

    #[test]
    fn purchase_fixture_passes() {
        let errors = validate_graph(&purchase_dsl(), &ctx());
        assert!(errors.is_empty(), "採購 fixture 不該有錯誤：{errors:?}");
    }

    #[test]
    fn revise_node_reachable_via_goto_only() {
        // revise 沒有任何 edges 指向它，只靠 on_reject 的 goto 進入。
        // 可達性分析必須含 goto，否則會誤判為孤立節點。
        let errors = validate_graph(&quotation_dsl(), &ctx());
        assert!(
            !errors.iter().any(|e| e.node_id.as_deref() == Some("revise")),
            "revise 靠 goto 進入，不應被判定孤立：{errors:?}"
        );
    }

    // ── 負向 ────────────────────────────────────────────

    #[test]
    fn missing_end_node() {
        let mut d = quotation_dsl();
        let nodes: Vec<Value> = d["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["type"] != "end")
            .cloned()
            .collect();
        d["nodes"] = json!(nodes);

        assert!(codes(&validate_graph(&d, &ctx())).contains(&"WF-E001"));
    }

    #[test]
    fn two_triggers() {
        let mut d = quotation_dsl();
        d["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "id": "start2", "type": "trigger" }));

        assert!(codes(&validate_graph(&d, &ctx())).contains(&"WF-E001"));
    }

    #[test]
    fn orphan_node() {
        let mut d = quotation_dsl();
        d["nodes"].as_array_mut().unwrap().push(json!({
            "id": "orphan", "type": "human_task",
            "participant": "internal", "assignee": { "type": "initiator" }
        }));

        let errors = validate_graph(&d, &ctx());
        assert!(errors
            .iter()
            .any(|e| e.code == "WF-E002" && e.node_id.as_deref() == Some("orphan")));
    }

    #[test]
    fn edge_to_nonexistent_node() {
        let mut d = quotation_dsl();
        d["edges"]
            .as_array_mut()
            .unwrap()
            .push(json!(["manager_approval", "ghost"]));

        assert!(codes(&validate_graph(&d, &ctx())).contains(&"WF-E002"));
    }

    #[test]
    fn condition_missing_branch() {
        let mut d = quotation_dsl();
        let edges: Vec<Value> = d["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| {
                let a = e.as_array().unwrap();
                !(a[0] == "discount_gate" && a.get(2).map(|m| m["when"] == false).unwrap_or(false))
            })
            .cloned()
            .collect();
        d["edges"] = json!(edges);

        let errors = validate_graph(&d, &ctx());
        assert!(errors
            .iter()
            .any(|e| e.code == "WF-E003" && e.node_id.as_deref() == Some("discount_gate")));
    }

    #[test]
    fn goto_pointing_downstream() {
        let mut d = quotation_dsl();
        let nodes = d["nodes"].as_array_mut().unwrap();
        let n = nodes
            .iter_mut()
            .find(|n| n["id"] == "manager_approval")
            .unwrap();
        n["on_reject"] = json!({ "action": "goto", "node": "create_order" });

        let errors = validate_graph(&d, &ctx());
        assert!(
            errors.iter().any(|e| e.code == "WF-E004"),
            "goto 指向下游應被擋下：{errors:?}"
        );
    }

    #[test]
    fn goto_to_nonexistent_node() {
        let mut d = quotation_dsl();
        let nodes = d["nodes"].as_array_mut().unwrap();
        nodes
            .iter_mut()
            .find(|n| n["id"] == "manager_approval")
            .unwrap()["on_reject"] = json!({ "action": "goto", "node": "nowhere" });

        assert!(codes(&validate_graph(&d, &ctx())).contains(&"WF-E004"));
    }

    #[test]
    fn unknown_role() {
        let mut d = quotation_dsl();
        d["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|n| n["id"] == "finance_approval")
            .unwrap()["resolver"] = json!({ "type": "role", "value": "chief_wizard" });

        assert!(codes(&validate_graph(&d, &ctx())).contains(&"WF-E005"));
    }

    #[test]
    fn unknown_role_inside_composite() {
        let mut d = purchase_dsl();
        d["nodes"].as_array_mut().unwrap()[1]["resolver"]["of"][2]["spec"]["value"] =
            json!("nonexistent_role");

        let errors = validate_graph(&d, &ctx());
        assert!(
            codes(&errors).contains(&"WF-E005"),
            "composite 內的角色也要檢查：{errors:?}"
        );
    }

    #[test]
    fn unbalanced_parentheses() {
        let mut d = quotation_dsl();
        d["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|n| n["id"] == "discount_gate")
            .unwrap()["expression"] = json!("(quotation.discount_rate > 0.15");

        assert!(codes(&validate_graph(&d, &ctx())).contains(&"WF-E006"));
    }

    #[test]
    fn single_equals_is_typo() {
        let mut d = quotation_dsl();
        d["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|n| n["id"] == "discount_gate")
            .unwrap()["expression"] = json!("quotation.status = 'draft'");

        assert!(codes(&validate_graph(&d, &ctx())).contains(&"WF-E006"));
    }

    #[test]
    fn comparison_operators_are_valid() {
        for expr in [
            "a == b", "a != b", "a >= b", "a <= b", "a > b", "a < b",
            "(a > 1) && (b < 2)",
        ] {
            assert!(expression_parses(expr), "{expr} 應為合法");
        }
    }

    #[test]
    fn unregistered_action() {
        let mut d = quotation_dsl();
        d["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|n| n["id"] == "create_order")
            .unwrap()["action"] = json!("odoo.launch_rocket");

        assert!(codes(&validate_graph(&d, &ctx())).contains(&"WF-E007"));
    }

    #[test]
    fn erp_write_without_idempotency_key() {
        let mut d = quotation_dsl();
        d["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|n| n["id"] == "create_order")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("idempotency_key");

        let errors = validate_graph(&d, &ctx());
        assert!(
            errors.iter().any(|e| e.code == "WF-E007"
                && e.message.contains("idempotency_key")),
            "ERP 寫入缺 idempotency_key 應被擋：{errors:?}"
        );
    }

    #[test]
    fn external_participant_with_role_resolver() {
        let mut d = quotation_dsl();
        d["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|n| n["id"] == "customer_review")
            .unwrap()["resolver"] = json!({ "type": "role", "value": "approver" });

        assert!(codes(&validate_graph(&d, &ctx())).contains(&"WF-E008"));
    }

    #[test]
    fn empty_context_skips_role_and_action_checks() {
        // 未提供租戶資料時不檢查，避免測試或預覽情境誤報
        let empty = ValidationContext::default();
        let mut d = quotation_dsl();
        d["nodes"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|n| n["id"] == "finance_approval")
            .unwrap()["resolver"] = json!({ "type": "role", "value": "whatever" });

        let errors = validate_graph(&d, &empty);
        assert!(!codes(&errors).contains(&"WF-E005"));
    }

    // ── 並行與匯合（WF-E009 / WF-E010）────────────────────

    fn parallel_dsl(edges: Value) -> Value {
        json!({
            "nodes": [
                { "id": "start", "type": "trigger" },
                { "id": "split", "type": "parallel" },
                { "id": "a", "type": "human_approval",
                  "resolver": { "type": "role", "value": "approver" } },
                { "id": "b", "type": "human_approval",
                  "resolver": { "type": "role", "value": "finance_manager" } },
                { "id": "merge", "type": "join",
                  "join": { "completion": "ALL", "result": "ALL_SUCCESS" } },
                { "id": "end", "type": "end", "result": "completed" }
            ],
            "edges": edges
        })
    }

    #[test]
    fn parallel_with_join_passes() {
        let dsl = parallel_dsl(json!([
            ["start", "split"],
            ["split", "a"], ["split", "b"],
            ["a", "merge"], ["b", "merge"],
            ["merge", "end"]
        ]));
        let errors = validate_graph(&dsl, &ctx());
        assert!(errors.is_empty(), "合法的並行不該有錯誤：{errors:?}");
    }

    #[test]
    fn parallel_without_branches_is_error() {
        // split 沒有出邊
        let dsl = parallel_dsl(json!([
            ["start", "split"],
            ["a", "merge"], ["b", "merge"],
            ["merge", "end"]
        ]));
        let errors = validate_graph(&dsl, &ctx());
        assert!(
            codes(&errors).contains(&"WF-E009"),
            "parallel 沒有分支應回 WF-E009：{errors:?}"
        );
    }

    #[test]
    fn parallel_with_single_branch_is_error() {
        // 只有一個分支，用 parallel 沒有意義
        let dsl = parallel_dsl(json!([
            ["start", "split"],
            ["split", "a"],
            ["a", "merge"],
            ["merge", "end"]
        ]));
        let errors = validate_graph(&dsl, &ctx());
        assert!(
            codes(&errors).contains(&"WF-E009"),
            "parallel 只有一個分支應回 WF-E009：{errors:?}"
        );
    }

    #[test]
    fn branch_not_reaching_join_is_error() {
        // b 直接到 end，沒有匯合
        let dsl = parallel_dsl(json!([
            ["start", "split"],
            ["split", "a"], ["split", "b"],
            ["a", "merge"], ["b", "end"],
            ["merge", "end"]
        ]));
        let errors = validate_graph(&dsl, &ctx());
        assert!(
            codes(&errors).contains(&"WF-E010"),
            "分支未匯合應回 WF-E010：{errors:?}"
        );
    }

    #[test]
    fn join_without_parallel_is_error() {
        let dsl = json!({
            "nodes": [
                { "id": "start", "type": "trigger" },
                { "id": "a", "type": "human_approval",
                  "resolver": { "type": "role", "value": "approver" } },
                { "id": "merge", "type": "join",
                  "join": { "completion": "ALL" } },
                { "id": "end", "type": "end", "result": "completed" }
            ],
            "edges": [["start", "a"], ["a", "merge"], ["merge", "end"]]
        });
        let errors = validate_graph(&dsl, &ctx());
        assert!(
            codes(&errors).contains(&"WF-E010"),
            "join 沒有對應的 parallel 應回 WF-E010：{errors:?}"
        );
    }

    #[test]
    fn n_of_m_without_n_is_error() {
        let mut dsl = parallel_dsl(json!([
            ["start", "split"],
            ["split", "a"], ["split", "b"],
            ["a", "merge"], ["b", "merge"],
            ["merge", "end"]
        ]));
        dsl["nodes"][4]["join"] = json!({ "completion": "N_OF_M" });
        let errors = validate_graph(&dsl, &ctx());
        assert!(
            codes(&errors).contains(&"WF-E009"),
            "N_OF_M 缺 n 應回 WF-E009：{errors:?}"
        );
    }

    #[test]
    fn n_greater_than_branch_count_is_error() {
        // 兩個分支卻要求三個核准，永遠滿足不了
        let mut dsl = parallel_dsl(json!([
            ["start", "split"],
            ["split", "a"], ["split", "b"],
            ["a", "merge"], ["b", "merge"],
            ["merge", "end"]
        ]));
        dsl["nodes"][4]["join"] = json!({ "completion": "N_OF_M", "n": 3 });
        let errors = validate_graph(&dsl, &ctx());
        assert!(
            codes(&errors).contains(&"WF-E009"),
            "n 大於分支數應回 WF-E009：{errors:?}"
        );
    }


    #[test]
    fn join_escalate_is_rejected() {
        // 分支各有自己的簽核人，加簽給誰沒有明確語意
        let mut dsl = parallel_dsl(json!([
            ["start", "split"],
            ["split", "a"], ["split", "b"],
            ["a", "merge"], ["b", "merge"],
            ["merge", "end"]
        ]));
        dsl["nodes"][4]["timeout"] =
            json!({ "after": "P1D", "policy": "ESCALATE",
                    "to": { "type": "role", "value": "cfo" } });
        let errors = validate_graph(&dsl, &ctx());
        assert!(
            codes(&errors).contains(&"WF-E009"),
            "join 的 ESCALATE 應回 WF-E009：{errors:?}"
        );
    }

    #[test]
    fn join_auto_approve_passes() {
        let mut dsl = parallel_dsl(json!([
            ["start", "split"],
            ["split", "a"], ["split", "b"],
            ["a", "merge"], ["b", "merge"],
            ["merge", "end"]
        ]));
        dsl["nodes"][4]["timeout"] = json!({ "after": "P2D", "policy": "AUTO_APPROVE" });
        let errors = validate_graph(&dsl, &ctx());
        assert!(errors.is_empty(), "join 的逾時放行不該有錯誤：{errors:?}");
    }

    #[test]
    fn escalate_without_to_is_error() {
        let dsl = json!({
            "nodes": [
                { "id": "start", "type": "trigger" },
                { "id": "a", "type": "human_approval",
                  "resolver": { "type": "role", "value": "approver" },
                  "timeout": { "after": "P1D", "policy": "ESCALATE" } },
                { "id": "end", "type": "end", "result": "completed" }
            ],
            "edges": [["start", "a"], ["a", "end"]]
        });
        let errors = validate_graph(&dsl, &ctx());
        assert!(
            codes(&errors).contains(&"WF-E011"),
            "ESCALATE 缺 to 應回 WF-E011：{errors:?}"
        );
    }

    // ── E012：資料路徑存在性 ────────────────────────────
    //
    // 這條規則擋的是「靜默跳過簽核」：條件式引用不存在的路徑時
    // 求值取不到值，被當成 false，流程照跑只是少走一段簽核，
    // 沒有任何錯誤訊息。高折扣的報價單會不經財務核准就發給客戶。

    /// 帶路徑的 context。未帶路徑的 ctx() 代表不檢查，見 known_paths 的註解。
    fn ctx_with_paths() -> ValidationContext {
        ctx().with_paths([
            "quotation.discount_rate",
            "quotation.customer_contact_ids",
            "quotation.total",
            "quotation.lines",
            "quotation.currency",
        ])
    }

    #[test]
    fn empty_known_paths_skips_check() {
        // 回歸：known_paths 為空時不檢查。
        // 既有呼叫端（與既有的 29 個測試）都沒有提供路徑，
        // 若改成「空集合等於沒有合法路徑」會讓它們全部爆掉。
        let mut dsl = quotation_dsl();
        dsl["nodes"][2]["expression"] = json!("quotation.不存在的欄位 > 0.15");
        let errors = validate_graph(&dsl, &ctx());
        assert!(
            !codes(&errors).contains(&"WF-E012"),
            "known_paths 為空時不該檢查路徑：{errors:?}"
        );
    }

    #[test]
    fn known_path_in_expression_passes() {
        let errors = validate_graph(&quotation_dsl(), &ctx_with_paths());
        assert!(
            !codes(&errors).contains(&"WF-E012"),
            "fixture 引用的路徑都已宣告，不該回 WF-E012：{errors:?}"
        );
    }

    #[test]
    fn unknown_path_in_expression_is_error() {
        let mut dsl = quotation_dsl();
        dsl["nodes"][2]["expression"] = json!("quotation.nonexistent > 0.15");
        let errors = validate_graph(&dsl, &ctx_with_paths());
        assert!(
            codes(&errors).contains(&"WF-E012"),
            "條件式引用不存在的路徑應回 WF-E012：{errors:?}"
        );
    }

    #[test]
    fn typo_path_is_error() {
        // 最危險的情境：少一個字母。discount_rat 取不到值 → 當成 false
        // → 高折扣報價單靜默跳過財務簽核。
        let mut dsl = quotation_dsl();
        dsl["nodes"][2]["expression"] = json!("quotation.discount_rat > 0.15");
        let errors = validate_graph(&dsl, &ctx_with_paths());
        assert!(
            codes(&errors).contains(&"WF-E012"),
            "打錯字的路徑應回 WF-E012：{errors:?}"
        );
    }

    #[test]
    fn unknown_resolver_path_is_error() {
        // resolver 的 path 不在 collect_expressions 的涵蓋範圍，
        // 要另外收集。取不到值會讓流程 NoParticipants → FAILED。
        let mut dsl = quotation_dsl();
        let nodes = dsl["nodes"].as_array_mut().unwrap();
        for node in nodes.iter_mut() {
            if node.pointer("/resolver/path").is_some() {
                node["resolver"]["path"] = json!("quotation.wrong_contacts");
            }
        }
        let errors = validate_graph(&dsl, &ctx_with_paths());
        assert!(
            codes(&errors).contains(&"WF-E012"),
            "resolver path 引用不存在的路徑應回 WF-E012：{errors:?}"
        );
    }

    #[test]
    fn cel_builtins_are_not_paths() {
        // has() / size() 是 CEL 內建函式，不是資料路徑。
        // 誤判會讓合法的流程無法發布——誤擋比漏擋更快失去信任。
        let mut dsl = quotation_dsl();
        dsl["nodes"][2]["expression"] =
            json!("has(quotation.discount_rate) && size(quotation.lines) > 0");
        let errors = validate_graph(&dsl, &ctx_with_paths());
        assert!(
            !codes(&errors).contains(&"WF-E012"),
            "CEL 內建函式不該被當成路徑：{errors:?}"
        );
    }

    #[test]
    fn numeric_literals_are_not_paths() {
        // 0.15 的形態是「識別字.識別字」以外的東西，但小數點容易誤判
        let mut dsl = quotation_dsl();
        dsl["nodes"][2]["expression"] = json!("quotation.discount_rate > 0.15");
        let errors = validate_graph(&dsl, &ctx_with_paths());
        assert!(
            !codes(&errors).contains(&"WF-E012"),
            "數字字面值不該被當成路徑：{errors:?}"
        );
    }

    #[test]
    fn string_literals_are_not_paths() {
        // 字串內的內容不是路徑
        let mut dsl = quotation_dsl();
        dsl["nodes"][2]["expression"] = json!("quotation.currency == 'TWD.dollar'");
        let errors = validate_graph(&dsl, &ctx_with_paths());
        assert!(
            !codes(&errors).contains(&"WF-E012"),
            "字串字面值不該被當成路徑：{errors:?}"
        );
    }
}
