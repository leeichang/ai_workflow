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
use std::collections::{HashMap, HashSet};

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
        .route("/sandboxes/{id}/tasks/{task_id}/form", get(task_form))
        .route("/sandboxes/{id}/tasks/{task_id}/simulate", post(simulate))
        .route(
            "/sandboxes/{id}/tasks/{task_id}/skip-time",
            post(skip_time),
        )
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
                // 只有標記為沙箱的實例可以時間快轉。
                // 正式路徑（instances.rs）不帶這個欄位，Python 端
                // 預設 False，因此既有的執行中實例也一併受保護。
                "is_sandbox": true,
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
    /// 這張待辦所屬的會簽組。不在平行結構裡就是 `None`
    branch: Option<BranchProgress>,
    /// 這個節點設的逾時。沒設就是 `None`——
    /// 前端據此決定要不要給「時間快轉」，並說清楚快轉後會發生什麼
    timeout: Option<NodeTimeout>,
}

#[derive(Serialize)]
pub struct NodeTimeout {
    /// ISO 8601 duration，例如 P2D
    after: String,
    /// WAIT / AUTO_APPROVE / AUTO_REJECT / ESCALATE
    policy: String,
}

/// 會簽進度
///
/// 執行期不存這個（Temporal 自己持久化分支進度），所以是從 DSL
/// 推出結構、再用同實例的待辦狀態算出進度。
#[derive(Serialize)]
pub struct BranchProgress {
    #[serde(flatten)]
    info: domain::parallel::BranchInfo,
    /// 已完成的分支數。使用者要看的是「還在等誰」
    done: usize,
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

    // 會簽進度要靠 DSL 推（執行期沒有 parallel_group 表），
    // 所以要拿到每個實例鎖定的流程版本內容。
    let instance_ids: Vec<Uuid> = {
        let mut ids: Vec<Uuid> = rows.iter().map(|r| r.1).collect();
        ids.sort();
        ids.dedup();
        ids
    };

    let dsls: Vec<(Uuid, Value)> = sqlx::query_as(
        "select i.id, v.content
         from workflow_instance i
         join workflow_definition_version v on v.id = i.workflow_version_id
         where i.id = any($1)",
    )
    .bind(&instance_ids)
    .fetch_all(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    // 同實例的所有待辦（不只 PENDING）——分支算不算完成要看它
    let all_tasks: Vec<(Uuid, String, String)> = sqlx::query_as(
        "select instance_id, node_id, status from human_task where instance_id = any($1)",
    )
    .bind(&instance_ids)
    .fetch_all(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    tx.commit().await?;

    let dsl_of: HashMap<Uuid, &Value> = dsls.iter().map(|(id, c)| (*id, c)).collect();

    Ok(Json(
        rows.into_iter()
            .map(
                |(task_id, instance_id, business_key, node_id, node_label, aid, aname, arole)| {
                    let dsl = dsl_of.get(&instance_id);
                    let branch = dsl.and_then(|dsl| {
                        let info = domain::parallel::branch_of(dsl, &node_id)?;
                        let done = branches_done(dsl, &info, instance_id, &all_tasks);
                        Some(BranchProgress { info, done })
                    });
                    let timeout = dsl.and_then(|dsl| node_timeout(dsl, &node_id));

                    PendingTaskView {
                        task_id,
                        instance_id,
                        business_key,
                        node_id,
                        node_label,
                        assignee_user_id: aid,
                        assignee_name: aname,
                        assignee_role: arole,
                        branch,
                        timeout,
                    }
                },
            )
            .collect(),
    ))
}

/// 節點設的逾時
///
/// 拿來決定要不要顯示「時間快轉」。沒設逾時的節點沒有等待可以快轉，
/// 給了按鈕卻什麼都不會發生，比不給更讓人困惑。
fn node_timeout(dsl: &Value, node_id: &str) -> Option<NodeTimeout> {
    let t = dsl
        .get("nodes")?
        .as_array()?
        .iter()
        .find(|n| n.get("id").and_then(Value::as_str) == Some(node_id))?
        .get("timeout")?;

    Some(NodeTimeout {
        after: t.get("after").and_then(Value::as_str)?.to_string(),
        policy: t
            .get("policy")
            .and_then(Value::as_str)
            .unwrap_or("WAIT")
            .to_string(),
    })
}

/// 這組會簽已經完成幾條分支
///
/// 一條分支算完成的條件是「沒有待處理的待辦，且至少處理過一張」。
/// 只看「沒有 PENDING」會把還沒開始的分支也算成完成——
/// 那會讓畫面顯示「2/2 完成」而流程其實還卡著。
fn branches_done(
    dsl: &Value,
    info: &domain::parallel::BranchInfo,
    instance_id: Uuid,
    all_tasks: &[(Uuid, String, String)],
) -> usize {
    let mut pending: HashSet<String> = HashSet::new();
    let mut touched: HashSet<String> = HashSet::new();

    for (iid, node_id, status) in all_tasks {
        if *iid != instance_id {
            continue;
        }
        // 同一組會簽的分支才算——別組的待辦不影響這組的進度
        let Some(b) = domain::parallel::branch_of(dsl, node_id) else {
            continue;
        };
        if b.parallel_id != info.parallel_id {
            continue;
        }
        if status == "PENDING" {
            pending.insert(b.branch_id.clone());
        }
        touched.insert(b.branch_id);
    }

    touched.difference(&pending).count()
}

#[derive(Serialize)]
pub struct SimulatedFormView {
    form_key: String,
    /// 被扮演者的角色。讓使用者看得出「為什麼這些欄位是這樣」
    acting_as_roles: Vec<String>,
    acting_as_name: Option<String>,
    fields: Vec<domain::permission::FieldPermission>,
}

/// 以被扮演者的身分看這張單的欄位權限
///
/// **這是使用者要的第三件事：「系統依據權限設定顯示每個簽核人員的畫面」。**
///
/// 三個關鍵：
///
/// 1. **用被扮演者的角色**，不是測試者的。測試者是 designer，
///    看到的會與 cfo 完全不同——而沙箱的價值就是看到後者會看到的。
///
/// 2. **帶實例的真實資料**。`visibility.when` 會用欄位值求值
///    （`permission.rs` 的 `eval_bool`），傳空物件的話凡是條件顯示的欄位
///    全部判錯。這正是 P2 擴充預覽 API 的理由。
///
/// 3. **判定複用 `domain::permission::resolve_form()`**，不在這裡重寫一份。
///    兩份必然漂移，而漂移的症狀是「畫面說可編輯、實際送出被擋」，極難追查。
///
/// 表單取自實例鎖定的版本（P3 的 `form_version_id`），不是當前發布版——
/// 模擬要看的是這張單當初送簽時的樣子。
async fn task_form(
    State(state): State<AppState>,
    actor: Actor,
    Path((sandbox_id, task_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<SimulatedFormView>> {
    require_simulation_owner(&state, &actor, sandbox_id).await?;

    let mut tx = state.db.tenant_tx(sandbox_id).await?;

    let task = persistence::human_task::find_by_id(&mut tx, task_id).await?;
    let instance = persistence::instance::find_by_id(&mut tx, task.instance_id).await?;

    let assignee = task
        .assignee_user_id
        .ok_or_else(|| ApiError::Conflict("這張待辦還沒有解析出簽核人".into()))?;

    // 被扮演者的角色。resolve_form 依這些角色判定可見性與可編輯性。
    let roles: Vec<String> = sqlx::query_scalar(
        "select r.code from user_role ur join role r on r.id = ur.role_id
         where ur.user_id = $1",
    )
    .bind(assignee)
    .fetch_all(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    let assignee_name: Option<String> =
        sqlx::query_scalar("select name from app_user where id = $1")
            .bind(assignee)
            .fetch_optional(tx.executor())
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

    // 用實例鎖定的表單版本，不是當前發布版
    let version_id = instance.form_version_id.ok_or_else(|| {
        ApiError::Conflict("這張單沒有鎖定表單版本，無法顯示欄位權限".into())
    })?;

    let version: (Uuid, Value) = sqlx::query_as(
        "select form_id, content from form_definition_version where id = $1",
    )
    .bind(version_id)
    .fetch_one(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    let form_key: String =
        sqlx::query_scalar("select form_key from form_definition where id = $1")
            .bind(version.0)
            .fetch_one(tx.executor())
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

    tx.commit().await?;

    let ctx = domain::permission::Context {
        node_id: Some(&task.node_id),
        roles: &roles,
        participant_kind: &task.participant_kind,
        // 真實單據資料——條件顯示的欄位靠它求值
        data: &instance.input,
    };

    Ok(Json(SimulatedFormView {
        form_key,
        acting_as_roles: roles.clone(),
        acting_as_name: assignee_name,
        fields: domain::permission::resolve_form(&version.1, &ctx),
    }))
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

#[derive(Serialize)]
pub struct SkipTimeResult {
    task_id: Uuid,
    node_id: String,
    /// 這個節點設的逾時策略。使用者要知道快轉後會發生什麼
    policy: String,
    after: String,
}

/// 時間快轉
///
/// 含 `timeout: {after: P2D}` 的流程在模擬時原本要等兩天才看得到
/// 逾時行為，等於無法驗證。快轉讓等待立刻視為到期。
///
/// **不假造決策**——Workflow 端跑的是真正的逾時處理，
/// 與真的等到逾時走同一條路。假造一個 APPROVE 會讓模擬顯示
/// 「通過了」，而正式環境設的 AUTO_REJECT 其實是退回。
///
/// 授權與模擬簽核同一條線（`require_simulation_owner`）。
/// Workflow 端另有一道 `is_sandbox` 檢查——快轉會讓單據在
/// 沒人簽的情況下前進，值得擋兩層。
async fn skip_time(
    State(state): State<AppState>,
    actor: Actor,
    Path((sandbox_id, task_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<SkipTimeResult>> {
    require_simulation_owner(&state, &actor, sandbox_id).await?;

    let mut tx = state.db.tenant_tx(sandbox_id).await?;
    let task = persistence::human_task::find_by_id(&mut tx, task_id).await?;
    let instance = persistence::instance::find_by_id(&mut tx, task.instance_id).await?;

    let dsl: Value = sqlx::query_scalar(
        "select content from workflow_definition_version where id = $1",
    )
    .bind(instance.workflow_version_id)
    .fetch_one(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))?;

    tx.commit().await?;

    // 沒設逾時的節點快轉沒有意義——Workflow 端會直接忽略，
    // 使用者卻會以為做了什麼。明講比靜默無效好。
    let timeout = dsl
        .get("nodes")
        .and_then(Value::as_array)
        .and_then(|nodes| {
            nodes
                .iter()
                .find(|n| n.get("id").and_then(Value::as_str) == Some(task.node_id.as_str()))
        })
        .and_then(|n| n.get("timeout"))
        .ok_or_else(|| {
            ApiError::Conflict(format!(
                "節點「{}」沒有設定逾時，沒有可以快轉的等待",
                task.node_label.clone().unwrap_or_else(|| task.node_id.clone())
            ))
        })?;

    let handle = state.temporal.as_ref().ok_or_else(|| {
        ApiError::ServiceUnavailable("流程引擎未連線，暫時無法快轉".into())
    })?;

    handle
        .client
        .signal(
            &instance.temporal_workflow_id,
            "skip_time",
            Value::String(task.node_id.clone()),
        )
        .await
        .map_err(|e| ApiError::ServiceUnavailable(format!("快轉失敗：{e}")))?;

    let mut tx = state.db.tenant_tx(sandbox_id).await?;
    tx.audit(
        persistence::AuditEvent::new("internal", "task.skip_time", "human_task")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(task_id.to_string())
            .payload(serde_json::json!({
                "node_id": task.node_id,
                "timeout": timeout,
                "is_simulation": true,
            })),
    )
    .await?;
    tx.commit().await?;

    Ok(Json(SkipTimeResult {
        task_id,
        node_id: task.node_id,
        policy: timeout
            .get("policy")
            .and_then(Value::as_str)
            .unwrap_or("WAIT")
            .to_string(),
        after: timeout
            .get("after")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    }))
}
