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
    /// 不影響 `valid` 的提醒
    ///
    /// 與 errors 分開的理由：警告不該擋下發布，但使用者要看得到。
    /// 目前只有一種——業務物件未定義（見 `business_object_warnings`）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

/// 發布成功的回應
///
/// 比 `WorkflowVersion` 多一個 warnings。發布可能帶著警告成功，
/// 呼叫端要能拿到那些警告顯示給使用者。
#[derive(Serialize)]
pub struct PublishResult {
    #[serde(flatten)]
    version: persistence::workflow::WorkflowVersion,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<String>,
}

/// 業務物件未定義時的警告
///
/// `paths_for()` 查無業務物件時回空集合，而空集合在 WF-E012 代表
/// 「不檢查」（沿用 known_roles 的既有慣例）。所以業務物件沒有定義時，
/// 該流程的所有條件式與 resolver 路徑都不會被檢查——打錯字不會被擋，
/// 而畫面上沒有任何提示。
///
/// 這是 E012 本身要擋的那類靜默失敗，只是換了一層。
///
/// 不擋下發布：新業務物件尚未定義時仍應可用，否則使用者要先請人
/// 幫他建 JSON 定義才能做事——與「使用者自己是 Owner」的定位衝突。
fn business_object_warnings(business_object: &str) -> Vec<String> {
    if domain::business_object::is_known(business_object) {
        return Vec::new();
    }

    let known = domain::business_object::known_business_objects().join("、");
    vec![format!(
        "業務物件「{business_object}」尚未定義，\
         此流程的條件式與簽核人路徑不會被檢查（WF-E012 需要業務物件定義才能運作）。\
         打錯字的路徑會讓流程靜默跳過簽核。\
         已定義的業務物件：{known}"
    )]
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

    // 路徑名單取自業務物件定義，不從表單推導——
    // 同一個 business_object 的多張表單詞彙互不相容，取聯集會讓拼錯字合法。
    let ctx = ValidationContext::with_roles(roles)
        .with_actions(actions)
        .with_paths(domain::business_object::paths_for(&wf.business_object));
    let errors = workflow_validator::validate_graph(&draft.content, &ctx);

    Ok(Json(ValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings: business_object_warnings(&wf.business_object),
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
) -> ApiResult<Json<PublishResult>> {
    actor.require_any(&["designer"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let wf = persistence::workflow::find_by_key(&mut tx, &key).await?;

    let draft = persistence::workflow::get_draft(&mut tx, wf.id)
        .await?
        .ok_or_else(|| ApiError::Conflict("沒有可發布的草稿".into()))?;

    let (roles, actions) = persistence::workflow::load_validation_data(&mut tx).await?;
    // 路徑名單取自業務物件定義，不從表單推導——
    // 同一個 business_object 的多張表單詞彙互不相容，取聯集會讓拼錯字合法。
    let ctx = ValidationContext::with_roles(roles)
        .with_actions(actions)
        .with_paths(domain::business_object::paths_for(&wf.business_object));
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
    let warnings = business_object_warnings(&wf.business_object);

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
            // 警告也要記——事後查得到哪些流程是在沒有 E012 保護的
            // 狀態下發布的
            "warnings": warnings,
        })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(PublishResult {
        version: published,
        warnings,
    }))
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
