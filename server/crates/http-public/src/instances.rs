//! 流程實例 API
//!
//! 啟動流程牽涉兩個系統：本地資料庫與 Temporal。兩者沒有分散式
//! 交易，因此順序很重要。
//!
//! 先寫資料庫再啟動 Temporal：
//!   啟動失敗 → 留下一筆 RUNNING 但實際沒跑的孤兒紀錄，
//!              可由對帳程序偵測並清除。
//!   反過來做 → 流程已在 Temporal 執行，但系統不知道它存在，
//!              沒有任何線索可追，人工待辦會憑空出現。
//! 寧可留孤兒，不可留幽靈。

use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/instances", get(list).post(start))
        .route("/instances/{id}", get(get_one))
        .route("/instances/{id}/cancel", post(cancel))
}

#[derive(Deserialize)]
pub struct StartBody {
    /// 要執行的流程
    workflow_key: String,
    /// 業務單號，例如 QT-2026-0001
    business_key: String,
    /// 傳給流程的業務資料
    #[serde(default)]
    input: Value,
}

#[derive(Serialize)]
pub struct InstanceDetail {
    #[serde(flatten)]
    instance: persistence::instance::WorkflowInstance,
    /// Temporal 回報的即時狀態。查不到時為 None。
    ///
    /// 與 instance.status 分開呈現：後者是本地投影，
    /// 可能落後於 Temporal。兩者不一致本身就是有用的訊號。
    live_status: Option<String>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    business_object: Option<String>,
    status: Option<String>,
    #[serde(default = "default_limit")]
    limit: i64,
}

fn default_limit() -> i64 {
    50
}

/// 取得 Temporal 連線，未設定時回 503
///
/// 回 503 而非 500：這是「服務暫時不可用」，呼叫端重試有意義。
fn temporal(state: &AppState) -> ApiResult<&crate::TemporalHandle> {
    state.temporal.as_ref().ok_or_else(|| {
        ApiError::ServiceUnavailable(
            "流程引擎未連線。請確認 TEMPORAL_TARGET 設定與 Temporal Server 狀態".into(),
        )
    })
}

/// 啟動流程
async fn start(
    State(state): State<AppState>,
    actor: Actor,
    Json(body): Json<StartBody>,
) -> ApiResult<(axum::http::StatusCode, Json<InstanceDetail>)> {
    let handle = temporal(&state)?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    // 只能用已發布的版本啟動。草稿是設計中的半成品，
    // 拿它跑真實流程會讓簽核跑到一半才發現節點接錯。
    let definition = persistence::workflow::find_by_key(&mut tx, &body.workflow_key).await?;
    let published = persistence::workflow::get_latest_published(&mut tx, definition.id)
        .await?
        .ok_or_else(|| {
            ApiError::Conflict(format!("流程 {} 尚未發布，無法啟動", body.workflow_key))
        })?;

    // workflow_id 由後端組合，呼叫端無從指定 tenant 前綴。
    // 這是租戶隔離的一部分，見 temporal_client::workflow_id。
    let wf_id = temporal_client::workflow_id::build(
        actor.tenant_id,
        &definition.business_object,
        &body.business_key,
    )
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // 鎖定建立當下的表單版本。
    //
    // 不鎖的話，流程跑到一半改表單定義，進行中的實例會用到新版——
    // 欄位被刪掉、必填變選填、權限改了，都會直接影響還沒簽完的單。
    //
    // 以 business_object 反查：表單自己宣告服務哪個業務物件
    // （form_definition.business_object），方向是「表單指定業務物件」。
    //
    // 查無已發布表單時為 None，不擋啟動——不是每個業務物件都有表單
    // （例如純系統觸發的流程），而且既有租戶的表單可能還沒發布過。
    let form_version = persistence::form::find_published_by_business_object(
        &mut tx,
        &definition.business_object,
    )
    .await?;

    if form_version.is_none() {
        tracing::info!(
            business_object = %definition.business_object,
            business_key = %body.business_key,
            "業務物件沒有已發布的表單，此實例不鎖定表單版本"
        );
    }

    let instance = persistence::instance::create(
        &mut tx,
        persistence::instance::CreateInstance {
            workflow_version_id: published.id,
            form_version_id: form_version.map(|v| v.id),
            business_object: definition.business_object.clone(),
            business_key: body.business_key.clone(),
            temporal_workflow_id: wf_id.clone(),
            // 啟動後才知道，先留空
            temporal_run_id: String::new(),
            input: body.input.clone(),
        },
        actor.user_id,
    )
    .await
    .map_err(|e| match e {
        // unique 衝突代表同一單號已有進行中的流程
        persistence::Error::Sqlx(ref db) if is_unique_violation(db) => {
            ApiError::Conflict(format!("單號 {} 已有進行中的流程", body.business_key))
        }
        other => other.into(),
    })?;

    tx.audit(
        persistence::AuditEvent::new("internal", "instance.start", "workflow_instance")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(instance.id.to_string())
            .payload(serde_json::json!({
                "workflow_key": body.workflow_key,
                "business_key": body.business_key,
            })),
    )
    .await?;

    tx.commit().await?;

    // 資料已落地，現在啟動 Temporal。
    let started = handle
        .client
        .start_workflow(temporal_client::StartWorkflow {
            tenant_id: actor.tenant_id,
            business_object: definition.business_object.clone(),
            instance_id: body.business_key.clone(),
            workflow_type: "DslInterpreter".into(),
            task_queue: handle.task_queue.clone(),
            args: vec![serde_json::json!({
                "tenant_id": actor.tenant_id,
                "instance_id": instance.id,
                "workflow_version_id": published.id,
                "dsl": published.content,
                "business_object": body.input,
            })],
        })
        .await;

    match started {
        Ok(s) => {
            let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
            persistence::instance::set_run_id(&mut tx, instance.id, &s.run_id).await?;
            tx.commit().await?;

            Ok((
                axum::http::StatusCode::CREATED,
                Json(InstanceDetail {
                    instance,
                    live_status: Some("RUNNING".into()),
                }),
            ))
        }
        Err(e) => {
            // 標記為 FAILED，讓孤兒紀錄一眼可辨，不必等對帳程序
            let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
            let _ = persistence::instance::finish(
                &mut tx,
                instance.id,
                "FAILED",
                None,
                Some(format!("啟動流程引擎失敗：{e}")),
            )
            .await;
            tx.commit().await?;

            Err(ApiError::ServiceUnavailable(format!(
                "流程引擎啟動失敗：{e}"
            )))
        }
    }
}

async fn list(
    State(state): State<AppState>,
    actor: Actor,
    Query(q): Query<ListQuery>,
) -> ApiResult<Json<Vec<persistence::instance::WorkflowInstance>>> {
    // 一般使用者只看得到自己的單。RLS 只隔離到租戶，
    // 同租戶的人預設看得見彼此——流程監控要的是更嚴的範圍。
    //
    // 只有 admin 不設限。刻意不把 approver、finance_manager 等
    // 一併放行：那些角色是「會簽到某些單」，不是「該看到全部單」，
    // 兩者混為一談會讓監控變成全租戶的資料出口。
    let visible_to = if actor.has_role("admin") {
        None
    } else {
        Some(actor.user_id)
    };

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let items = persistence::instance::list(
        &mut tx,
        persistence::instance::ListFilter {
            business_object: q.business_object,
            status: q.status,
            limit: q.limit,
            visible_to,
        },
    )
    .await?;
    tx.commit().await?;

    Ok(Json(items))
}

async fn get_one(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<InstanceDetail>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let instance = persistence::instance::find_by_id(&mut tx, id).await?;

    // 清單過濾了，單筆也要濾——否則知道 id 就能繞過清單的限制，
    // 而 id 會出現在通知連結、稽核紀錄裡，不是秘密。
    let visible = actor.has_role("admin")
        || instance.started_by == Some(actor.user_id)
        || persistence::human_task::is_participant(&mut tx, id, actor.user_id).await?;
    tx.commit().await?;

    if !visible {
        // 回 404 而非 403：403 等於告訴對方「這張單存在」。
        return Err(ApiError::not_found("workflow_instance", id));
    }

    // 向 Temporal 問即時狀態。問不到不是錯誤——Temporal 可能暫時
    // 不可用，或流程已超過 retention 被清除，兩者都不影響歷史查詢。
    //
    // 必須用 peek 而非 get_result：後者會 long poll 等到流程結束，
    // 讓這個原本該立刻回應的 GET 卡住數十秒。
    let live_status = match state.temporal.as_ref() {
        Some(h) => match h.client.peek_result(&instance.temporal_workflow_id).await {
            Ok(r) => Some(describe(&r)),
            Err(e) => {
                tracing::debug!(%e, "無法取得 Temporal 即時狀態");
                None
            }
        },
        None => None,
    };

    Ok(Json(InstanceDetail {
        instance,
        live_status,
    }))
}

#[derive(Deserialize)]
pub struct CancelBody {
    #[serde(default)]
    reason: String,
}

async fn cancel(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<CancelBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let handle = temporal(&state)?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let instance = persistence::instance::find_by_id(&mut tx, id).await?;

    if instance.status != "RUNNING" {
        return Err(ApiError::Conflict(format!(
            "流程狀態為 {}，無法取消",
            instance.status
        )));
    }

    // 縱深防禦。RLS 已經擋住跨租戶查詢，這裡再確認一次：
    // workflow_id 是要送給 Temporal 的，送錯會影響其他租戶的流程。
    if !temporal_client::workflow_id::belongs_to(&instance.temporal_workflow_id, actor.tenant_id) {
        return Err(ApiError::Forbidden("流程不屬於此租戶".into()));
    }

    tx.audit(
        persistence::AuditEvent::new("internal", "instance.cancel", "workflow_instance")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(instance.id.to_string())
            .payload(serde_json::json!({ "reason": body.reason })),
    )
    .await?;
    tx.commit().await?;

    // 只送取消請求，不直接改狀態。Workflow 收到取消後會做清理
    // （取消待辦、寫稽核），完成時才回報最終狀態。
    // 這裡先改成 CANCELLED 會讓「已取消但待辦還在」的窗口出現。
    let reason = if body.reason.is_empty() {
        "使用者取消".to_string()
    } else {
        body.reason.clone()
    };

    handle
        .client
        .cancel(&instance.temporal_workflow_id, &reason)
        .await
        .map_err(|e| ApiError::ServiceUnavailable(format!("取消請求送出失敗：{e}")))?;

    Ok(Json(serde_json::json!({
        "requested": true,
        "note": "取消請求已送出。流程完成清理後狀態才會更新"
    })))
}

fn describe(r: &temporal_client::WorkflowResult) -> String {
    use temporal_client::WorkflowResult as W;
    match r {
        W::Running => "RUNNING".into(),
        W::Completed(_) => "COMPLETED".into(),
        W::Failed(_) => "FAILED".into(),
        W::Cancelled => "CANCELLED".into(),
        W::Terminated(_) => "TERMINATED".into(),
        W::TimedOut => "TIMED_OUT".into(),
    }
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|d| d.code())
        .is_some_and(|c| c == "23505")
}
