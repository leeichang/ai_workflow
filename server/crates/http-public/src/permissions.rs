//! 權限矩陣 API
//!
//! 對應設計稿 _7。提供兩種檢視：
//!   依角色：列為欄位、欄為流程節點，固定角色
//!   依節點：列為角色、欄為欄位，固定節點
//!
//! 矩陣由後端計算而非前端。原因是權限規則會影響安全，
//! 前端算一套、後端算另一套遲早會不一致，屆時以哪邊為準？
//! 統一由後端算，前端只負責顯示與收集使用者的修改。

use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use domain::permission::{self, Context, Permission};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/forms/{form_key}/permissions", get(matrix))
        .route("/forms/{form_key}/permissions/preview", get(preview))
}

#[derive(Deserialize)]
pub struct MatrixQuery {
    /// 流程 key。省略時只回單一「初始」欄。
    workflow_key: Option<String>,
    /// 檢視模式：by_role（預設）或 by_node
    #[serde(default)]
    mode: ViewMode,
    /// mode = by_role 時要看的角色
    role: Option<String>,
    /// mode = by_node 時要看的節點
    node_id: Option<String>,
    /// 用哪個版本計算。省略時用草稿，沒草稿則用最新發布版。
    version: Option<i32>,
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    #[default]
    ByRole,
    ByNode,
}

/// 矩陣的一格
#[derive(Serialize)]
pub struct Cell {
    permission: Permission,
    required: bool,
    /// 判定依據，滑鼠移上去可看
    reason: &'static str,
    /// 是否由系統鎖定（計算欄位）。鎖定的格子 UI 不可點擊。
    locked: bool,
}

/// 矩陣的一列
#[derive(Serialize)]
pub struct MatrixRow {
    /// by_role 模式為欄位 key，by_node 模式為角色 code
    key: String,
    label: String,
    /// 僅 by_role 模式：欄位的資料路徑，UI 顯示於標籤下方
    #[serde(skip_serializing_if = "Option::is_none")]
    data_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<String>,
    /// 對外是否可見。false 時 UI 顯示「客戶看不到」標記。
    #[serde(skip_serializing_if = "Option::is_none")]
    external_visible: Option<bool>,
    /// key 為欄名（by_role 為節點 id，by_node 為欄位 key）
    cells: std::collections::BTreeMap<String, Cell>,
}

#[derive(Serialize)]
pub struct MatrixColumn {
    key: String,
    label: String,
    /// 外部節點在 UI 標示橘色「外部」badge
    participant_kind: String,
}

#[derive(Serialize)]
pub struct MatrixResponse {
    form_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    workflow_key: Option<String>,
    /// 計算所用的表單版本，null 代表草稿
    version: Option<i32>,
    columns: Vec<MatrixColumn>,
    rows: Vec<MatrixRow>,
    /// 可選的分組，對應 form content 的 sections
    sections: Vec<SectionInfo>,
}

#[derive(Serialize)]
pub struct SectionInfo {
    key: String,
    title: String,
    field_count: usize,
}

/// 取得權限矩陣
async fn matrix(
    State(state): State<AppState>,
    actor: Actor,
    Path(form_key): Path<String>,
    Query(q): Query<MatrixQuery>,
) -> ApiResult<Json<MatrixResponse>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let form = persistence::form::find_by_key(&mut tx, &form_key).await?;

    // 版本選擇：明確指定 > 草稿 > 最新發布
    let source = match q.version {
        Some(v) => persistence::form::get_version(&mut tx, form.id, v).await?,
        None => match persistence::form::get_draft(&mut tx, form.id).await? {
            Some(d) => d,
            None => persistence::form::get_latest_published(&mut tx, form.id)
                .await?
                .ok_or_else(|| ApiError::Conflict("表單尚無任何版本".into()))?,
        },
    };

    let workflow_nodes = match &q.workflow_key {
        Some(key) => load_workflow_nodes(&mut tx, key).await?,
        None => vec![NodeInfo::initial()],
    };

    tx.commit().await?;

    let content = &source.content;
    let response = match q.mode {
        ViewMode::ByRole => build_by_role(&form_key, &q, &source, content, &workflow_nodes)?,
        ViewMode::ByNode => build_by_node(&form_key, &q, &source, content, &workflow_nodes)?,
    };

    Ok(Json(response))
}

/// 單一情境的權限預覽
///
/// 表單設計器的「預覽」功能用。給定節點與角色，
/// 回傳所有欄位的權限，讓設計者確認規則寫對了。
#[derive(Deserialize)]
pub struct PreviewQuery {
    node_id: Option<String>,
    #[serde(default)]
    roles: String,
    #[serde(default = "default_kind")]
    participant_kind: String,
    /// 帶入真實單據的業務資料，供條件式權限求值
    ///
    /// 設計器預覽沒有實際單據，不傳即可（`data` 為空物件，行為與先前相同）。
    /// 模擬簽核有真實單據，不傳的話 `visibility.when` 依賴欄位值的欄位
    /// 全部會判錯——而沙箱的整個價值就是「看到的畫面與那個人會看到的一樣」。
    instance_id: Option<uuid::Uuid>,
}

fn default_kind() -> String {
    "internal".into()
}

async fn preview(
    State(state): State<AppState>,
    actor: Actor,
    Path(form_key): Path<String>,
    Query(q): Query<PreviewQuery>,
) -> ApiResult<Json<Vec<domain::permission::FieldPermission>>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let form = persistence::form::find_by_key(&mut tx, &form_key).await?;

    let source = match persistence::form::get_draft(&mut tx, form.id).await? {
        Some(d) => d,
        None => persistence::form::get_latest_published(&mut tx, form.id)
            .await?
            .ok_or_else(|| ApiError::Conflict("表單尚無任何版本".into()))?,
    };

    // 有帶 instance_id 就用該實例的業務資料求值條件式。
    //
    // 查不到時讓 find_by_id 的 NotFound 往上冒（回 404），
    // 不要默默退回空物件——默默退回會讓模擬畫面判錯而無人察覺，
    // 正是本專案一再踩到的靜默失敗類型。
    // 跨租戶的 id 同樣落在這裡：RLS 讓查詢查不到，回 404。
    let data = match q.instance_id {
        Some(id) => persistence::instance::find_by_id(&mut tx, id).await?.input,
        None => Value::Object(Default::default()),
    };

    tx.commit().await?;

    // 未指定角色時用當前使用者的角色
    let roles: Vec<String> = if q.roles.is_empty() {
        actor.roles.clone()
    } else {
        q.roles.split(',').map(|s| s.trim().to_string()).collect()
    };

    let ctx = Context {
        node_id: q.node_id.as_deref(),
        roles: &roles,
        participant_kind: &q.participant_kind,
        data: &data,
    };

    Ok(Json(permission::resolve_form(&source.content, &ctx)))
}

// ── 矩陣建構 ────────────────────────────────────────────

struct NodeInfo {
    id: String,
    label: String,
    participant_kind: String,
}

impl NodeInfo {
    fn initial() -> Self {
        Self {
            id: "start".into(),
            label: "開始".into(),
            participant_kind: "internal".into(),
        }
    }
}

/// 從流程定義取出會顯示表單的節點
///
/// 只有 human_approval 與 human_task 會顯示表單，
/// condition、action 這些節點沒有使用者介面，列進矩陣只是雜訊。
///
/// 一律從草稿優先讀取。設計者調整流程後，權限矩陣要立刻反映新節點，
/// 否則得先發布流程才能設定權限，順序上說不通。
async fn load_workflow_nodes(
    tx: &mut persistence::TenantTx<'_>,
    workflow_key: &str,
) -> ApiResult<Vec<NodeInfo>> {
    let definition = persistence::workflow::find_by_key(tx, workflow_key).await?;

    let source = match persistence::workflow::get_draft(tx, definition.id).await? {
        Some(d) => d,
        None => persistence::workflow::get_latest_published(tx, definition.id)
            .await?
            .ok_or_else(|| ApiError::Conflict("流程尚無任何版本".into()))?,
    };

    Ok(form_bearing_nodes(&source.content))
}

/// 挑出會顯示表單的節點
///
/// 與資料庫無關，單獨一個函式以便測試。
fn form_bearing_nodes(content: &Value) -> Vec<NodeInfo> {
    // 起始節點永遠在最前面：申請人填寫表單的情境不對應任何流程節點，
    // 但它是權限設定中最常用的一欄。
    let mut nodes = vec![NodeInfo::initial()];

    let Some(items) = content.get("nodes").and_then(Value::as_array) else {
        return nodes;
    };

    for node in items {
        let node_type = node.get("type").and_then(Value::as_str).unwrap_or_default();
        if !matches!(node_type, "human_approval" | "human_task") {
            continue;
        }

        let id = node.get("id").and_then(Value::as_str).unwrap_or_default();
        if id.is_empty() {
            continue;
        }

        let label = node
            .get("label")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(id);

        let participant_kind = node
            .get("participant")
            .and_then(Value::as_str)
            .unwrap_or("internal");

        nodes.push(NodeInfo {
            id: id.to_string(),
            label: label.to_string(),
            participant_kind: participant_kind.to_string(),
        });
    }

    nodes
}

fn build_by_role(
    form_key: &str,
    q: &MatrixQuery,
    source: &persistence::form::FormVersion,
    content: &Value,
    nodes: &[NodeInfo],
) -> ApiResult<MatrixResponse> {
    let role = q
        .role
        .clone()
        .ok_or_else(|| ApiError::BadRequest("mode=by_role 時必須指定 role".into()))?;
    let roles = vec![role];

    let fields = content
        .get("fields")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::Conflict("表單內容缺少 fields".into()))?;

    let empty = Value::Object(Default::default());
    let mut rows = Vec::with_capacity(fields.len());

    for field in fields {
        let key = field.get("key").and_then(Value::as_str).unwrap_or_default();
        let mut cells = std::collections::BTreeMap::new();

        for node in nodes {
            let ctx = Context {
                node_id: Some(&node.id),
                roles: &roles,
                participant_kind: &node.participant_kind,
                data: &empty,
            };
            let r = permission::resolve_field(field, &ctx);
            cells.insert(
                node.id.clone(),
                Cell {
                    permission: r.permission,
                    required: r.required,
                    reason: r.reason,
                    locked: r.reason == "系統計算欄位",
                },
            );
        }

        rows.push(MatrixRow {
            key: key.to_string(),
            label: field
                .pointer("/ui/label")
                .and_then(Value::as_str)
                .unwrap_or(key)
                .to_string(),
            data_path: field
                .pointer("/data/path")
                .and_then(Value::as_str)
                .map(str::to_owned),
            section: field.get("section").and_then(Value::as_str).map(str::to_owned),
            external_visible: field
                .pointer("/ui/external_visible")
                .and_then(Value::as_bool),
            cells,
        });
    }

    Ok(MatrixResponse {
        form_key: form_key.to_string(),
        workflow_key: q.workflow_key.clone(),
        version: source.version,
        columns: nodes
            .iter()
            .map(|n| MatrixColumn {
                key: n.id.clone(),
                label: n.label.clone(),
                participant_kind: n.participant_kind.clone(),
            })
            .collect(),
        rows,
        sections: collect_sections(content),
    })
}

fn build_by_node(
    form_key: &str,
    q: &MatrixQuery,
    source: &persistence::form::FormVersion,
    content: &Value,
    nodes: &[NodeInfo],
) -> ApiResult<MatrixResponse> {
    let node_id = q
        .node_id
        .clone()
        .ok_or_else(|| ApiError::BadRequest("mode=by_node 時必須指定 node_id".into()))?;

    let node = nodes
        .iter()
        .find(|n| n.id == node_id)
        .ok_or_else(|| ApiError::not_found("node", &node_id))?;

    let fields = content
        .get("fields")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::Conflict("表單內容缺少 fields".into()))?;

    // 轉置：列為角色
    let all_roles = ["admin", "designer", "approver", "requester", "viewer"];
    let empty = Value::Object(Default::default());
    let mut rows = Vec::new();

    for role in all_roles {
        let roles = vec![role.to_string()];
        let mut cells = std::collections::BTreeMap::new();

        for field in fields {
            let key = field.get("key").and_then(Value::as_str).unwrap_or_default();
            let ctx = Context {
                node_id: Some(&node.id),
                roles: &roles,
                participant_kind: &node.participant_kind,
                data: &empty,
            };
            let r = permission::resolve_field(field, &ctx);
            cells.insert(
                key.to_string(),
                Cell {
                    permission: r.permission,
                    required: r.required,
                    reason: r.reason,
                    locked: r.reason == "系統計算欄位",
                },
            );
        }

        rows.push(MatrixRow {
            key: role.to_string(),
            label: role_label(role).to_string(),
            data_path: None,
            section: None,
            external_visible: None,
            cells,
        });
    }

    Ok(MatrixResponse {
        form_key: form_key.to_string(),
        workflow_key: q.workflow_key.clone(),
        version: source.version,
        columns: fields
            .iter()
            .map(|f| {
                let key = f.get("key").and_then(Value::as_str).unwrap_or_default();
                MatrixColumn {
                    key: key.to_string(),
                    label: f
                        .pointer("/ui/label")
                        .and_then(Value::as_str)
                        .unwrap_or(key)
                        .to_string(),
                    participant_kind: "internal".into(),
                }
            })
            .collect(),
        rows,
        sections: collect_sections(content),
    })
}

fn collect_sections(content: &Value) -> Vec<SectionInfo> {
    let Some(sections) = content.get("sections").and_then(Value::as_array) else {
        return Vec::new();
    };
    let fields = content.get("fields").and_then(Value::as_array);

    sections
        .iter()
        .filter_map(|s| {
            let key = s.get("key").and_then(Value::as_str)?;
            let count = fields
                .map(|fs| {
                    fs.iter()
                        .filter(|f| f.get("section").and_then(Value::as_str) == Some(key))
                        .count()
                })
                .unwrap_or(0);
            Some(SectionInfo {
                key: key.to_string(),
                title: s
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(key)
                    .to_string(),
                field_count: count,
            })
        })
        .collect()
}

fn role_label(code: &str) -> &str {
    match code {
        "admin" => "系統管理員",
        "designer" => "流程設計者",
        "approver" => "簽核人員",
        "requester" => "申請人",
        "viewer" => "檢視者",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn content() -> Value {
        json!({
            "nodes": [
                { "id": "start", "type": "trigger", "label": "業務送出報價單" },
                { "id": "mgr", "type": "human_approval", "label": "主管簽核",
                  "participant": "internal" },
                { "id": "gate", "type": "condition", "expression": "a > 1" },
                { "id": "revise", "type": "human_task", "label": "業務修改報價",
                  "participant": "internal" },
                { "id": "publish", "type": "action", "action": "quotation.publish" },
                { "id": "customer", "type": "human_approval", "label": "客戶簽核",
                  "participant": "external" },
                { "id": "notify", "type": "notification" },
                { "id": "end", "type": "end" }
            ]
        })
    }

    #[test]
    fn 只取會顯示表單的節點() {
        // condition、action、notification 沒有使用者介面，
        // 列進矩陣只會讓設計者困惑「這一欄要設定什麼」。
        let nodes = form_bearing_nodes(&content());
        let ids: Vec<&str> = nodes.iter().map(|n| n.id.as_str()).collect();

        assert_eq!(ids, vec!["start", "mgr", "revise", "customer"]);
    }

    #[test]
    fn 起始節點永遠在最前面() {
        let nodes = form_bearing_nodes(&content());
        assert_eq!(nodes[0].id, "start");
        assert_eq!(nodes[0].label, "開始");
    }

    #[test]
    fn 保留外部參與者標記() {
        // participant_kind 決定 resolve_field 是否套用「外部一律唯讀」，
        // 漏帶會讓客戶簽核那一欄顯示成可編輯。
        let nodes = form_bearing_nodes(&content());
        let customer = nodes.iter().find(|n| n.id == "customer").unwrap();

        assert_eq!(customer.participant_kind, "external");
    }

    #[test]
    fn 內部節點預設為內部參與者() {
        let c = json!({
            "nodes": [{ "id": "a", "type": "human_task", "label": "處理" }]
        });
        let nodes = form_bearing_nodes(&c);

        assert_eq!(nodes[1].participant_kind, "internal");
    }

    #[test]
    fn 沒有標籤時以節點代碼代替() {
        let c = json!({
            "nodes": [{ "id": "mgr", "type": "human_approval" }]
        });
        let nodes = form_bearing_nodes(&c);

        assert_eq!(nodes[1].label, "mgr");
    }

    #[test]
    fn 空標籤視同沒有標籤() {
        let c = json!({
            "nodes": [{ "id": "mgr", "type": "human_approval", "label": "" }]
        });
        let nodes = form_bearing_nodes(&c);

        assert_eq!(nodes[1].label, "mgr");
    }

    #[test]
    fn 缺少節點清單時仍回傳起始節點() {
        // 流程內容損壞不該讓整個權限矩陣打不開
        let nodes = form_bearing_nodes(&json!({}));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, "start");
    }

    #[test]
    fn 忽略沒有代碼的節點() {
        let c = json!({
            "nodes": [{ "type": "human_approval", "label": "壞掉的節點" }]
        });
        let nodes = form_bearing_nodes(&c);

        assert_eq!(nodes.len(), 1, "只剩起始節點");
    }
}
