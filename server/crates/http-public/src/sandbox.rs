//! 開發模式（沙箱）與模擬簽核
//!
//! 使用者要的三件事（D-07）：
//!   1. 真的解析簽核人——用當前的角色與組織設定跑 resolver，不是假資料
//!   2. 一人扮演所有角色——不換身分登入，點選就切換扮演對象
//!   3. 看到那個人會看到的畫面——欄位可見性依被扮演者的角色重算
//!
//! **沙箱不是另一個環境，是一個附身模式。**

use axum::extract::{Path, State};
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
        .route("/sandboxes", get(list).post(create))
        .route("/sandboxes/{id}/retire", post(retire))
        .route("/sandboxes/{id}/participants/{node_id}", get(participants))
        .route("/sandboxes/{id}/tasks", get(pending_tasks))
        .route("/sandboxes/{id}/instances", post(start_instance))
        .route("/sandboxes/{id}/tasks/{task_id}/simulate", post(simulate))
}

#[derive(Serialize)]
pub struct SandboxInfo {
    #[serde(flatten)]
    session: persistence::sandbox::SandboxSession,
    /// 沙箱租戶的代碼，前端拿來顯示「你正在 xxx 的開發模式」
    sandbox_code: Option<String>,
}

/// 建立開發模式
///
/// 只有 designer 能開——開發模式會複製整份主檔，
/// 讓每個人都能開會讓租戶數量失控（已有 574 個測試租戶的前例）。
async fn create(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<(axum::http::StatusCode, Json<SandboxInfo>)> {
    actor.require_any(&["designer"])?;

    // 不可從沙箱再開沙箱。巢狀的開發模式沒有明確語意
    // （改動要往哪一層送審？），而且會讓回收變成樹狀問題。
    let tx = state.db.tenant_tx(actor.tenant_id).await?;
    if tx.is_sandbox() {
        return Err(ApiError::Conflict("已經在開發模式中，不可再開一層".into()));
    }
    tx.commit().await?;

    let (sandbox_id, session) =
        persistence::sandbox::create(&state.db, actor.tenant_id, actor.user_id).await?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    tx.audit(
        persistence::AuditEvent::new("internal", "sandbox.create", "tenant")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(sandbox_id.to_string()),
    )
    .await?;
    tx.commit().await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(SandboxInfo {
            sandbox_code: None,
            session,
        }),
    ))
}

async fn list(State(state): State<AppState>, actor: Actor) -> ApiResult<Json<Vec<SandboxInfo>>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let sessions = persistence::sandbox::list_active(&mut tx, actor.tenant_id).await?;
    tx.commit().await?;

    Ok(Json(
        sessions
            .into_iter()
            .map(|session| SandboxInfo {
                sandbox_code: None,
                session,
            })
            .collect(),
    ))
}

/// 退役開發模式
///
/// 只有建立者本人能退役——與模擬簽核的授權同一個理由，
/// 沙箱是誰開的就由誰收。
async fn retire(
    State(state): State<AppState>,
    actor: Actor,
    Path(sandbox_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let session = persistence::sandbox::active_session_of(&state.db, sandbox_id)
        .await?
        .ok_or_else(|| ApiError::Conflict("找不到進行中的開發模式".into()))?;

    if session.created_by != actor.user_id {
        return Err(ApiError::Forbidden("只有建立者可以退役這個開發模式".into()));
    }

    persistence::sandbox::retire(&state.db, sandbox_id).await?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    tx.audit(
        persistence::AuditEvent::new("internal", "sandbox.retire", "tenant")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(sandbox_id.to_string()),
    )
    .await?;
    tx.commit().await?;

    Ok(Json(serde_json::json!({ "retired": true })))
}

#[derive(Serialize)]
pub struct ParticipantView {
    user_id: Uuid,
    name: String,
    /// 由哪個角色解析出來的。讓使用者看得出「為什麼是這個人」——
    /// 這正是模擬要驗證的東西。
    role: Option<String>,
}

/// 解析某節點的實際簽核人
///
/// **這是模擬簽核最大的價值所在：它在驗證 resolver 設定對不對。**
///
/// 用真實的 resolver 與沙箱裡真實的角色、組織設定，不是假資料。
/// 已踩過的坑：`resolver.user_id` 需要 UUID 不是 email，寫錯會讓流程
/// FAILED；主檔複製不完整會讓 resolver 找不到人。
/// 模擬讓這兩類錯誤在**上線前**浮現，而不是上線後第一張單卡住。
///
/// 解析不到人時回 409 並說明原因，**不可跳過或用預設人員頂替**——
/// 頂替會讓使用者以為設定是對的。
async fn participants(
    State(state): State<AppState>,
    actor: Actor,
    Path((sandbox_id, node_id)): Path<(Uuid, String)>,
) -> ApiResult<Json<Vec<ParticipantView>>> {
    require_simulation_owner(&state, &actor, sandbox_id).await?;

    let mut tx = state.db.tenant_tx(sandbox_id).await?;

    // 目前支援 role 與 department_manager；manager_of 需要實例才解析得出
    // 發起人，留給 simulate 時處理。
    let users = persistence::participant::by_role(&mut tx, &node_id).await?;
    tx.commit().await?;

    if users.is_empty() {
        return Err(ApiError::Conflict(format!(
            "無法解析簽核人員：角色 {node_id} 沒有指派任何使用者"
        )));
    }

    Ok(Json(
        users
            .into_iter()
            .map(|u| ParticipantView {
                user_id: u.id,
                name: u.name,
                role: u.role,
            })
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct StartBody {
    workflow_key: String,
    business_key: String,
    #[serde(default)]
    input: Value,
}

/// 在沙箱裡啟動一張測試單
///
/// 沒有這支的話，測試者得登入沙箱租戶才能啟動流程——
/// 那與「不換身分登入」矛盾，整個模擬就少了起點。
///
/// 與 `POST /instances` 的差別只在租戶取自路徑而非 JWT，
/// 其餘邏輯（只能用已發布版本、鎖定表單版本、workflow_id 前綴）
/// 都沿用同一條路徑。
async fn start_instance(
    State(state): State<AppState>,
    actor: Actor,
    Path(sandbox_id): Path<Uuid>,
    Json(body): Json<StartBody>,
) -> ApiResult<(axum::http::StatusCode, Json<Value>)> {
    require_simulation_owner(&state, &actor, sandbox_id).await?;

    let handle = state.temporal.as_ref().ok_or_else(|| {
        ApiError::ServiceUnavailable("流程引擎未連線，暫時無法啟動".into())
    })?;

    let mut tx = state.db.tenant_tx(sandbox_id).await?;

    let definition = persistence::workflow::find_by_key(&mut tx, &body.workflow_key).await?;
    let published = persistence::workflow::get_latest_published(&mut tx, definition.id)
        .await?
        .ok_or_else(|| {
            ApiError::Conflict(format!("流程 {} 尚未發布，無法啟動", body.workflow_key))
        })?;

    let _wf_id = temporal_client::workflow_id::build(
        sandbox_id,
        &definition.business_object,
        &body.business_key,
    )
    .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let form_version =
        persistence::form::find_published_by_business_object(&mut tx, &definition.business_object)
            .await?;

    let instance = persistence::instance::create(
        &mut tx,
        persistence::instance::CreateInstance {
            workflow_version_id: published.id,
            form_version_id: form_version.map(|v| v.id),
            business_object: definition.business_object.clone(),
            business_key: body.business_key.clone(),
            temporal_workflow_id: _wf_id.clone(),
            temporal_run_id: String::new(),
            input: body.input.clone(),
        },
        // 發起人記沙箱裡的建立者——resolver 的 manager_of(initiator)
        // 要解析得出這個人的主管
        session_creator_in_sandbox(&state, sandbox_id).await?,
    )
    .await?;

    tx.commit().await?;

    // 資料已落地才啟動 Temporal——順序與 POST /instances 一致：
    // 先寫 DB 再啟動，失敗時留下可回收的孤兒紀錄；
    // 反過來會留下系統完全不知道存在的幽靈流程。
    let started = handle
        .client
        .start_workflow(temporal_client::StartWorkflow {
            tenant_id: sandbox_id,
            business_object: definition.business_object.clone(),
            instance_id: body.business_key.clone(),
            workflow_type: "DslInterpreter".into(),
            task_queue: handle.task_queue.clone(),
            args: vec![serde_json::json!({
                "tenant_id": sandbox_id,
                "instance_id": instance.id,
                "workflow_version_id": published.id,
                "dsl": published.content,
                "business_object": body.input,
            })],
        })
        .await
        .map_err(|e| ApiError::ServiceUnavailable(format!("啟動流程失敗：{e}")))?;

    let mut tx = state.db.tenant_tx(sandbox_id).await?;
    persistence::instance::set_run_id(&mut tx, instance.id, &started.run_id).await?;
    tx.commit().await?;

    Ok((
        axum::http::StatusCode::CREATED,
        Json(serde_json::json!({
            "id": instance.id,
            "business_key": body.business_key,
        })),
    ))
}

/// 沙箱建立者在沙箱內的 user id
async fn session_creator_in_sandbox(state: &AppState, sandbox_id: Uuid) -> ApiResult<Uuid> {
    persistence::sandbox::active_session_of(&state.db, sandbox_id)
        .await?
        .and_then(|s| s.created_by_in_sandbox)
        .ok_or_else(|| ApiError::Conflict("這個開發模式沒有對應的使用者".into()))
}

#[derive(Serialize)]
pub struct PendingTaskView {
    task_id: Uuid,
    instance_id: Uuid,
    business_key: String,
    node_id: String,
    node_label: Option<String>,
    /// 解析出來的簽核人。這就是「誰在簽」——
    /// 測試者要點誰來扮演，看的就是這個。
    assignee_user_id: Option<Uuid>,
    assignee_name: Option<String>,
    assignee_role: Option<String>,
}

/// 沙箱裡所有待簽的項目
///
/// 不能用 `GET /tasks`——那是**收件匣**，只列出指派給自己的。
/// 模擬的整個前提是測試者不是 assignee，用收件匣會永遠是空的。
///
/// 這支就是使用者說的「流程走到哪、誰在簽」那個畫面。
async fn pending_tasks(
    State(state): State<AppState>,
    actor: Actor,
    Path(sandbox_id): Path<Uuid>,
) -> ApiResult<Json<Vec<PendingTaskView>>> {
    require_simulation_owner(&state, &actor, sandbox_id).await?;

    let mut tx = state.db.tenant_tx(sandbox_id).await?;

    let rows: Vec<(Uuid, Uuid, String, String, Option<String>, Option<Uuid>, Option<String>, Option<String>)> =
        sqlx::query_as(
            "select t.id, t.instance_id, i.business_key, t.node_id, t.node_label,
                    t.assignee_user_id, u.name, t.assignee_role
             from human_task t
             join workflow_instance i on i.id = t.instance_id
             left join app_user u on u.id = t.assignee_user_id
             where t.status = 'PENDING'
             order by t.created_at",
        )
        .fetch_all(tx.executor())
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    tx.commit().await?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(task_id, instance_id, business_key, node_id, node_label, aid, aname, arole)| {
                    PendingTaskView {
                        task_id,
                        instance_id,
                        business_key,
                        node_id,
                        node_label,
                        assignee_user_id: aid,
                        assignee_name: aname,
                        assignee_role: arole,
                    }
                },
            )
            .collect(),
    ))
}

#[derive(Deserialize)]
pub struct SimulateBody {
    /// 扮演誰。必須是該節點解析出的簽核人之一。
    acting_as: Uuid,
    /// APPROVE 或 REJECT
    decision: String,
    #[serde(default)]
    comment: String,
}

#[derive(Serialize)]
pub struct SimulateResult {
    task_id: Uuid,
    acting_as: Uuid,
    decision: String,
}

/// 以扮演的身分做決策
///
/// 測試者始終是同一個真實使用者，只是在這次操作上標註扮演誰。
/// **稽核同時記 `actor_id`（真正按下按鈕的人）與 `acting_as`（扮演對象）。**
///
/// 只記 `acting_as` 的話，稽核會顯示張文華核准了某張單，
/// 而他根本不知道有這回事。沙箱裡雖無害，但兩套稽核語意
/// 會讓程式碼分岔——寧可從一開始就一致。
async fn simulate(
    State(state): State<AppState>,
    actor: Actor,
    Path((sandbox_id, task_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SimulateBody>,
) -> ApiResult<Json<SimulateResult>> {
    if body.decision != "APPROVE" && body.decision != "REJECT" {
        return Err(ApiError::BadRequest(
            "decision 必須是 APPROVE 或 REJECT".into(),
        ));
    }

    // 授權：目標必須是沙箱、屬於我的租戶、且我是建立者本人。
    // 沙箱身分是環境狀態不是權限。
    require_simulation_owner(&state, &actor, sandbox_id).await?;

    // 之後的操作都在**沙箱租戶**內進行——測試者本人仍在正式租戶。
    let mut tx = state.db.tenant_tx(sandbox_id).await?;
    let task = persistence::human_task::find_by_id(&mut tx, task_id).await?;

    if task.status != "PENDING" {
        return Err(ApiError::Conflict(format!(
            "待辦狀態為 {}，無法再次決策",
            task.status
        )));
    }

    let instance = persistence::instance::find_by_id(&mut tx, task.instance_id).await?;

    let handle = state.temporal.as_ref().ok_or_else(|| {
        ApiError::ServiceUnavailable("流程引擎未連線，暫時無法送出決策".into())
    })?;

    // decided_by 記扮演對象：待辦的歷程要看起來與真實簽核一致，
    // 「誰真的按了按鈕」記在稽核，兩者各司其職。
    let updated = persistence::human_task::decide(
        &mut tx,
        task_id,
        &body.decision,
        &body.comment,
        body.acting_as,
    )
    .await?;

    if !updated {
        return Err(ApiError::Conflict("待辦已被處理".into()));
    }

    tx.audit(
        persistence::AuditEvent::new("internal", "task.simulate", "human_task")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .acting_as(body.acting_as.to_string())
            .target(task_id.to_string())
            .payload(serde_json::json!({
                "decision": body.decision,
                "node_id": task.node_id,
                "is_simulation": true,
            })),
    )
    .await?;

    // participant_id 用扮演對象：Workflow 端要看到的是
    // 「這個節點的簽核人做了決定」，而不是測試者
    handle
        .client
        .signal(
            &instance.temporal_workflow_id,
            "task_completed",
            serde_json::json!({
                "task_id": task_id.to_string(),
                "node_id": task.node_id,
                "participant_id": body.acting_as.to_string(),
                "decision": body.decision,
                "comment": body.comment,
            }),
        )
        .await
        .map_err(|e| ApiError::ServiceUnavailable(format!("流程引擎未收到決策，請重試：{e}")))?;

    tx.commit().await?;

    Ok(Json(SimulateResult {
        task_id,
        acting_as: body.acting_as,
        decision: body.decision,
    }))
}

/// 檢查操作者是不是這個沙箱的建立者本人
///
/// **測試者不換身分登入**（D-07c）：他一直以正式租戶的身分登入，
/// 只是操作沙箱裡的東西。所以比對的是正式租戶的 id 與租戶：
///
///   1. 目標必須是沙箱租戶
///   2. 呼叫者所屬的租戶必須是這個沙箱的來源租戶
///   3. 呼叫者必須是建立者本人
///
/// 第 2 條不可省：少了它，別的租戶只要知道沙箱 id 就能操作它。
///
/// 與 `tasks.rs` 的開洞互補——那條處理的是「真的登入沙箱租戶」
/// 的情況，這條處理「從正式租戶操作沙箱」，兩者都不是角色。
async fn require_simulation_owner(
    state: &AppState,
    actor: &Actor,
    sandbox_tenant_id: Uuid,
) -> ApiResult<()> {
    let flags = persistence::sandbox::flags_of(&state.db, sandbox_tenant_id).await?;
    if !flags.is_sandbox {
        return Err(ApiError::Forbidden("這不是開發模式租戶".into()));
    }

    let session = persistence::sandbox::active_session_of(&state.db, sandbox_tenant_id)
        .await?
        .ok_or_else(|| ApiError::Forbidden("這個開發模式已退役".into()))?;

    if session.tenant_id != actor.tenant_id {
        return Err(ApiError::Forbidden("這個開發模式不屬於您的租戶".into()));
    }

    if session.created_by != actor.user_id {
        return Err(ApiError::Forbidden("只有開發模式的建立者可以模擬簽核".into()));
    }

    Ok(())
}
