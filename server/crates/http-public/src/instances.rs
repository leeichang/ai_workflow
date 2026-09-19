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
        // 以下三個是流程監控的介入動作，只有 admin 與 process_monitor 能用
        .route("/instances/{id}/tasks", get(list_tasks))
        .route("/instances/{id}/remind", post(remind))
        .route("/instances/{id}/resolve-attention", post(resolve_attention))
        .route("/tasks/{id}/reassign", post(reassign_task))
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
    /// 只看需要人介入的單
    #[serde(default)]
    needs_attention: bool,
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
    // admin 與 process_monitor 不設限（見 Actor::can_monitor_all）。
    let visible_to = if actor.can_monitor_all() {
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
            needs_attention: q.needs_attention,
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
    let visible = actor.can_monitor_all()
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

    // 誰能取消：發起人自己，或監控角色（admin / process_monitor）。
    //
    // 這裡原本**完全沒有角色檢查**——同租戶任何人只要知道 id
    // 就能取消別人的單，而 id 會出現在通知連結與稽核紀錄裡。
    // 清單有過濾但取消沒有，是一個既有的漏洞，補在這裡。
    //
    // 回 404 而非 403：403 等於告訴對方「這張單存在」。
    if !actor.can_monitor_all() && instance.started_by != Some(actor.user_id) {
        return Err(ApiError::not_found("workflow_instance", id));
    }

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
            // by_monitor 讓稽核看得出「這張單是被營運中止的，
            // 不是發起人自己撤的」。兩者的責任歸屬不同。
            .payload(serde_json::json!({
                "reason": body.reason,
                "by_monitor": instance.started_by != Some(actor.user_id),
            })),
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

// ── 流程監控的介入動作 ──────────────────────────────────
//
// 四個端點都要求 can_monitor_all（admin 或 process_monitor）。
// 語意是「營運端排除卡關」，不是一般使用者處理自己的單——
// 後者走既有的收件匣與 /instances/{id}/cancel。

/// 這筆流程卡在誰身上
///
/// 監控頁要回答的核心問題。流程狀態只說「RUNNING」，
/// 卡在誰身上是待辦才知道的事。
async fn list_tasks(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<persistence::human_task::HumanTask>>> {
    actor.require_any(&["process_monitor"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    // 先確認流程存在且屬於本租戶。RLS 已擋跨租戶，
    // 這裡是為了讓不存在的 id 回 404 而非空陣列——
    // 空陣列會讓監控頁顯示「這張單沒有待辦」，語意是錯的。
    let _ = persistence::instance::find_by_id(&mut tx, id).await?;
    let tasks = persistence::human_task::list_by_instance(&mut tx, id).await?;
    tx.commit().await?;

    Ok(Json(tasks))
}

/// 催辦：對還沒簽的人重寄提醒
///
/// 「卡住」在實務上最常見的解法就是催人，而不是取消或改派。
/// 因此這是監控角色最常用的動作，放在最前面。
async fn remind(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    actor.require_any(&["process_monitor"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let instance = persistence::instance::find_by_id(&mut tx, id).await?;

    if instance.status != "RUNNING" {
        return Err(ApiError::Conflict(format!(
            "流程狀態為 {}，沒有待辦可催",
            instance.status
        )));
    }

    let tasks = persistence::human_task::list_by_instance(&mut tx, id).await?;
    let pending: Vec<_> = tasks.iter().filter(|t| t.status == "PENDING").collect();

    if pending.is_empty() {
        return Err(ApiError::Conflict(
            "沒有待處理的待辦，不需要催辦".into(),
        ));
    }

    // 指派給角色的待辦沒有 assignee_user_id，收件人要用角色去解析。
    // 目前 resolve_recipients 只認 user id 與 email，因此角色型的
    // 待辦先跳過——催不到的人要讓呼叫端知道，不能靜默少寄。
    let (to, skipped): (Vec<String>, usize) = {
        let mut to = Vec::new();
        let mut skipped = 0;
        for t in &pending {
            match t.assignee_user_id {
                Some(u) => to.push(u.to_string()),
                None => skipped += 1,
            }
        }
        (to, skipped)
    };

    tx.audit(
        persistence::AuditEvent::new("internal", "instance.remind", "workflow_instance")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(instance.id.to_string())
            .payload(serde_json::json!({
                "pending": pending.len(),
                "notified": to.len(),
                "skipped_role_tasks": skipped,
            })),
    )
    .await?;
    tx.commit().await?;

    if to.is_empty() {
        return Ok(Json(serde_json::json!({
            "notified": 0,
            "skipped_role_tasks": skipped,
            "note": "待辦都指派給角色，目前無法定位個人信箱",
        })));
    }

    // 走 internal::deliver 而非自己組信：沙箱改寫只做在那裡，
    // 自己組信會讓沙箱的催辦信寄給真的同事。
    let node_id = pending
        .first()
        .map(|t| t.node_id.clone())
        .unwrap_or_default();

    let result = crate::internal::deliver(
        &state,
        actor.tenant_id,
        crate::internal::NotificationBody {
            instance_id: instance.id,
            node_id,
            channel: vec!["email".into()],
            to,
            kind: Some("reminder".into()),
            remaining: None,
        },
    )
    .await?;

    Ok(Json(serde_json::json!({
        "notified": pending.len() - skipped,
        "skipped_role_tasks": skipped,
        "send": result.0,
    })))
}

#[derive(Deserialize)]
pub struct ResolveAttentionBody {
    #[serde(default)]
    note: String,
}

/// 標記異常已處理
///
/// 刻意做成手動：「解析不到加簽對象」修好組織資料之後，
/// 不會有任何事件回來通知系統。自動解除只能靠輪詢重試，
/// 而重試的時機無從判定。由處理的人按下按鈕，順帶留稽核。
async fn resolve_attention(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<ResolveAttentionBody>,
) -> ApiResult<Json<Value>> {
    actor.require_any(&["process_monitor"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let instance = persistence::instance::find_by_id(&mut tx, id).await?;

    if !instance.needs_attention {
        return Err(ApiError::Conflict("這筆流程沒有待處理的異常".into()));
    }

    let cleared = persistence::instance::clear_attention(&mut tx, id).await?;

    tx.audit(
        persistence::AuditEvent::new(
            "internal",
            "instance.attention_resolved",
            "workflow_instance",
        )
        .actor(actor.user_id.to_string(), actor.name.clone())
        .target(instance.id.to_string())
        // 記下原本的異常代碼：解除之後欄位會被清空，
        // 不記的話事後查不出「當時是什麼問題」。
        .payload(serde_json::json!({
            "code": instance.attention_code,
            "detail": instance.attention_detail,
            "note": body.note,
        })),
    )
    .await?;
    tx.commit().await?;

    Ok(Json(serde_json::json!({ "resolved": cleared })))
}

#[derive(Deserialize)]
pub struct ReassignBody {
    to_user_id: Uuid,
    #[serde(default)]
    reason: String,
}

/// 把卡住的待辦改派給別人
///
/// 這會動到簽核責任歸屬，因此稽核事件型別與一般的指派分開
/// （`task.reassign` 而非 `task.create`）——查稽核時要看得出
/// 「這張單是被營運改派的，不是原主管指派的」。
async fn reassign_task(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<ReassignBody>,
) -> ApiResult<Json<Value>> {
    actor.require_any(&["process_monitor"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let task = persistence::human_task::find_by_id(&mut tx, id).await?;

    if task.status != "PENDING" {
        return Err(ApiError::Conflict(format!(
            "待辦狀態為 {}，無法改派",
            task.status
        )));
    }

    // 目標必須是同租戶的在職員工。RLS 保證跨租戶查不到，
    // 但離職的人查得到——改派給離職者等於把單丟進黑洞。
    let target = persistence::organization::find_employee(&mut tx, body.to_user_id).await?;
    if target.status != "ACTIVE" {
        return Err(ApiError::BadRequest(format!(
            "{} 不是在職員工，無法改派",
            target.name
        )));
    }

    let moved = persistence::human_task::reassign(&mut tx, id, body.to_user_id).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "task.reassign", "human_task")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(task.id.to_string())
            .payload(serde_json::json!({
                "instance_id": task.instance_id,
                "node_id": task.node_id,
                "from_user_id": task.assignee_user_id,
                "from_role": task.assignee_role,
                "to_user_id": body.to_user_id,
                "to_name": target.name,
                "reason": body.reason,
            })),
    )
    .await?;
    tx.commit().await?;

    Ok(Json(serde_json::json!({ "reassigned": moved })))
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
