//! 收件匣與決策 API
//!
//! 使用者按下「核准」後發生兩件事：資料庫記錄決策，
//! Temporal 收到 Signal 讓流程前進。兩者沒有分散式交易。
//!
//! 順序是先寫資料庫（未提交）→ 送 Signal → 成功才提交。
//! Signal 失敗時交易回滾，使用者看到錯誤可以重試。
//!
//! 反過來做（先提交再送 Signal）會出現最糟的狀態：
//! 待辦顯示「已核准」，但流程還停在原地等 Signal，
//! 而且沒有任何人會再送一次——使用者以為做完了。

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/tasks", get(list))
        .route("/tasks/{id}/decision", post(decide))
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default = "default_status")]
    status: String,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_status() -> String {
    "PENDING".into()
}

fn default_limit() -> i64 {
    50
}

#[derive(Serialize)]
pub struct TaskItem {
    #[serde(flatten)]
    task: persistence::human_task::HumanTask,
    /// 所屬流程的業務單號，讓收件匣不必再查一次
    business_key: Option<String>,
    business_object: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    actor: Actor,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<TaskItem>>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let tasks = persistence::human_task::inbox(
        &mut tx,
        actor.user_id,
        &actor.roles,
        &q.status,
        q.limit,
    )
    .await?;

    // 補上單號。逐筆查在待辦數量不大時可接受；
    // 成為瓶頸時改為一次 join。
    let mut items = Vec::with_capacity(tasks.len());
    for task in tasks {
        let instance = persistence::instance::find_by_id(&mut tx, task.instance_id)
            .await
            .ok();
        items.push(TaskItem {
            business_key: instance.as_ref().map(|i| i.business_key.clone()),
            business_object: instance.as_ref().map(|i| i.business_object.clone()),
            task,
        });
    }

    tx.commit().await?;
    Ok(Json(items))
}

#[derive(Deserialize)]
pub struct DecisionBody {
    /// APPROVE 或 REJECT
    decision: String,
    #[serde(default)]
    comment: String,
}

#[derive(Serialize)]
pub struct DecisionResult {
    task_id: Uuid,
    decision: String,
    /// 流程是否已收到 Signal
    signalled: bool,
}

async fn decide(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<DecisionBody>,
) -> ApiResult<Json<DecisionResult>> {
    if body.decision != "APPROVE" && body.decision != "REJECT" {
        return Err(ApiError::BadRequest(
            "decision 必須是 APPROVE 或 REJECT".into(),
        ));
    }

    // Temporal 的檢查刻意放在授權與狀態檢查之後。
    // 放前面的話，探測別人的待辦會得到 503 而非 403——
    // 回應碼本身就洩漏了「這張待辦存在」，而且 Temporal 斷線時
    // 所有的權限檢查都不會執行。
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let task = persistence::human_task::find_by_id(&mut tx, id).await?;

    if task.status != "PENDING" {
        return Err(ApiError::Conflict(format!(
            "待辦狀態為 {}，無法再次決策",
            task.status
        )));
    }

    // 驗證這張待辦確實指派給當前使用者。
    // RLS 只保證同租戶，不保證是本人——沒有這道檢查，
    // 同租戶的任何人都能核准別人的單。
    let is_assignee = task.assignee_user_id == Some(actor.user_id);
    let has_role = task
        .assignee_role
        .as_deref()
        .is_some_and(|r| actor.roles.iter().any(|mine| mine == r));

    if !is_assignee && !has_role {
        return Err(ApiError::Forbidden("此待辦不是指派給您的".into()));
    }

    let instance = persistence::instance::find_by_id(&mut tx, task.instance_id).await?;

    // 到這裡才檢查流程引擎。授權與狀態都通過了，
    // 此時的 503 確實只代表基礎設施問題。
    let handle = state.temporal.as_ref().ok_or_else(|| {
        ApiError::ServiceUnavailable("流程引擎未連線，暫時無法送出決策".into())
    })?;

    let updated = persistence::human_task::decide(
        &mut tx,
        id,
        &body.decision,
        &body.comment,
        actor.user_id,
    )
    .await?;

    if !updated {
        // 兩人同時點同一張待辦時，第二個人走到這裡
        return Err(ApiError::Conflict("待辦已被處理".into()));
    }

    tx.audit(
        persistence::AuditEvent::new("internal", "task.decide", "human_task")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(id.to_string())
            .payload(serde_json::json!({
                "decision": body.decision,
                "node_id": task.node_id,
            })),
    )
    .await?;

    // 先送 Signal 再提交。Signal 失敗時交易回滾，
    // 待辦維持 PENDING，使用者重試即可。
    handle
        .client
        .signal(
            &instance.temporal_workflow_id,
            "task_completed",
            serde_json::json!({
                "task_id": id.to_string(),
                "node_id": task.node_id,
                "participant_id": actor.user_id.to_string(),
                "decision": body.decision,
                "comment": body.comment,
            }),
        )
        .await
        .map_err(|e| {
            ApiError::ServiceUnavailable(format!("流程引擎未收到決策，請重試：{e}"))
        })?;

    tx.commit().await?;

    Ok(Json(DecisionResult {
        task_id: id,
        decision: body.decision,
        signalled: true,
    }))
}
