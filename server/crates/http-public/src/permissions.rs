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
    tx.commit().await?;

    // 未指定角色時用當前使用者的角色
    let roles: Vec<String> = if q.roles.is_empty() {
        actor.roles.clone()
    } else {
        q.roles.split(',').map(|s| s.trim().to_string()).collect()
    };

    let empty = Value::Object(Default::default());
    let ctx = Context {
        node_id: q.node_id.as_deref(),
        roles: &roles,
        participant_kind: &q.participant_kind,
        data: &empty,
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
/// condition、action 這些節點沒有使用者介面。
async fn load_workflow_nodes(
    tx: &mut persistence::TenantTx<'_>,
    workflow_key: &str,
) -> ApiResult<Vec<NodeInfo>> {
    // 流程定義表在任務 4.1 建立。目前先回初始節點，
    // 讓矩陣在流程模組完成前仍可用於單一情境。
    let _ = (tx, workflow_key);
    Ok(vec![NodeInfo::initial()])
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
