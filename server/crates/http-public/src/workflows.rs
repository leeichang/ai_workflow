//! 流程定義 API
//!
//! 端點與表單 API 對稱。差別在發布前多一層圖結構驗證，
//! 因為流程的錯誤（無法到達的節點、goto 指向下游）在執行時
//! 才會顯現，且可能讓實例永久卡住。

use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use domain::workflow_validator::{self, GraphError, ValidationContext};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/workflows", get(list).post(create))
        .route("/workflows/{key}", get(get_one))
        .route("/workflows/{key}/draft", put(save_draft))
        .route("/workflows/{key}/draft", delete(discard_draft))
        .route("/workflows/{key}/draft/validate", post(validate_draft))
        .route("/workflows/{key}/draft/publish", post(publish))
        .route("/workflows/{key}/versions", get(list_versions))
        .route("/workflows/{key}/versions/{version}", get(get_version))
}

#[derive(Serialize)]
pub struct WorkflowDetail {
    #[serde(flatten)]
    definition: persistence::workflow::WorkflowDefinition,
    draft: Option<persistence::workflow::WorkflowVersion>,
    published: Option<persistence::workflow::WorkflowVersion>,
}

#[derive(Serialize)]
pub struct ValidationResult {
    valid: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<GraphError>,
}

#[derive(Deserialize)]
pub struct SaveDraftBody {
    content: Value,
}

// ── Handler ─────────────────────────────────────────────

async fn list(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Vec<persistence::workflow::WorkflowSummary>>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let items = persistence::workflow::list(&mut tx).await?;
    tx.commit().await?;
    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    actor: Actor,
    Path(key): Path<String>,
) -> ApiResult<Json<WorkflowDetail>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let definition = persistence::workflow::find_by_key(&mut tx, &key).await?;
    let draft = persistence::workflow::get_draft(&mut tx, definition.id).await?;
    let published = persistence::workflow::get_latest_published(&mut tx, definition.id).await?;

    tx.commit().await?;
    Ok(Json(WorkflowDetail {
        definition,
        draft,
        published,
    }))
}

async fn create(
    State(state): State<AppState>,
    actor: Actor,
    Json(input): Json<persistence::workflow::CreateWorkflow>,
) -> ApiResult<(axum::http::StatusCode, Json<WorkflowDetail>)> {
    actor.require_any(&["designer"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let key = input.workflow_key.clone();

    let (definition, draft) = persistence::workflow::create(&mut tx, input, actor.user_id).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "workflow.create", "workflow_definition")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(definition.id.to_string())
            .payload(serde_json::json!({ "workflow_key": key })),
    )
    .await?;

    tx.commit().await?;
    Ok((
        axum::http::StatusCode::CREATED,
        Json(WorkflowDetail {
            definition,
            draft: Some(draft),
            published: None,
        }),
    ))
}

/// 儲存草稿
///
/// 不做圖結構驗證。設計中的流程常有暫時的斷點，
/// 每次存檔都報錯會妨礙編輯。
async fn save_draft(
    State(state): State<AppState>,
    actor: Actor,
    Path(key): Path<String>,
    Json(body): Json<SaveDraftBody>,
) -> ApiResult<Json<persistence::workflow::WorkflowVersion>> {
    actor.require_any(&["designer"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let wf = persistence::workflow::find_by_key(&mut tx, &key).await?;
    let draft =
        persistence::workflow::save_draft(&mut tx, wf.id, &body.content, actor.user_id).await?;

    tx.commit().await?;
    Ok(Json(draft))
}

async fn discard_draft(
    State(state): State<AppState>,
    actor: Actor,
    Path(key): Path<String>,
) -> ApiResult<axum::http::StatusCode> {
    actor.require_any(&["designer"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let wf = persistence::workflow::find_by_key(&mut tx, &key).await?;
    let removed = persistence::workflow::discard_draft(&mut tx, wf.id).await?;

    if removed {
        tx.audit(
            persistence::AuditEvent::new(
                "internal",
                "workflow.discard_draft",
                "workflow_definition",
            )
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(wf.id.to_string()),
        )
        .await?;
    }

    tx.commit().await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// 驗證草稿但不發布
///
/// 回 200 加結果物件，而非用 4xx 表示驗證失敗。
/// 前端需要逐條錯誤來標記節點，用錯誤碼表達會讓處理變複雜。
async fn validate_draft(
    State(state): State<AppState>,
    actor: Actor,
    Path(key): Path<String>,
) -> ApiResult<Json<ValidationResult>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let wf = persistence::workflow::find_by_key(&mut tx, &key).await?;

    let draft = persistence::workflow::get_draft(&mut tx, wf.id)
        .await?
        .ok_or_else(|| ApiError::Conflict("沒有草稿可驗證".into()))?;

    let (roles, actions) = persistence::workflow::load_validation_data(&mut tx).await?;
    tx.commit().await?;

    let ctx = ValidationContext::with_roles(roles).with_actions(actions);
    let errors = workflow_validator::validate_graph(&draft.content, &ctx);

    Ok(Json(ValidationResult {
        valid: errors.is_empty(),
        errors,
    }))
}

/// 發布草稿
///
/// 此處做完整圖結構驗證。流程一旦發布就會被實例使用，
/// 無法到達的節點或錯誤的退回目標會讓實例永久卡住。
async fn publish(
    State(state): State<AppState>,
    actor: Actor,
    Path(key): Path<String>,
) -> ApiResult<Json<persistence::workflow::WorkflowVersion>> {
    actor.require_any(&["designer"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let wf = persistence::workflow::find_by_key(&mut tx, &key).await?;

    let draft = persistence::workflow::get_draft(&mut tx, wf.id)
        .await?
        .ok_or_else(|| ApiError::Conflict("沒有可發布的草稿".into()))?;

    let (roles, actions) = persistence::workflow::load_validation_data(&mut tx).await?;
    let ctx = ValidationContext::with_roles(roles).with_actions(actions);
    let errors = workflow_validator::validate_graph(&draft.content, &ctx);

    if !errors.is_empty() {
        let summary = errors
            .iter()
            .map(|e| format!("[{}] {}", e.code, e.message))
            .collect::<Vec<_>>()
            .join("；");
        return Err(ApiError::ValidationFailed(summary));
    }

    let published = persistence::workflow::publish(&mut tx, wf.id, actor.user_id).await?;

    tx.audit(
        persistence::AuditEvent::new(
            "internal",
            "workflow.publish",
            "workflow_definition_version",
        )
        .actor(actor.user_id.to_string(), actor.name.clone())
        .target(published.id.to_string())
        .payload(serde_json::json!({
            "workflow_key": key,
            "version": published.version,
        })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(published))
}

async fn list_versions(
    State(state): State<AppState>,
    actor: Actor,
    Path(key): Path<String>,
) -> ApiResult<Json<Vec<persistence::workflow::WorkflowVersion>>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let wf = persistence::workflow::find_by_key(&mut tx, &key).await?;
    let versions = persistence::workflow::list_versions(&mut tx, wf.id).await?;
    tx.commit().await?;
    Ok(Json(versions))
}

async fn get_version(
    State(state): State<AppState>,
    actor: Actor,
    Path((key, version)): Path<(String, i32)>,
) -> ApiResult<Json<persistence::workflow::WorkflowVersion>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let wf = persistence::workflow::find_by_key(&mut tx, &key).await?;
    let v = persistence::workflow::get_version(&mut tx, wf.id, version).await?;
    tx.commit().await?;
    Ok(Json(v))
}
