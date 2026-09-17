//! Internal API
//!
//! 供 Python Worker 呼叫。與公開 API 的關鍵差異：
//! **它信任呼叫端宣稱的 tenant_id**，因為 Worker 執行的是
//! 系統流程，沒有登入使用者。
//!
//! 因此這組端點絕不能暴露到公網。防線有兩層：
//!   1. 共享密鑰（X-Internal-Token）。密鑰外洩等同租戶隔離失效。
//!   2. 部署時不對外開放 /internal 路徑（反向代理層）。
//!
//! Worker 為什麼不直接連資料庫：
//!   租戶隔離、稽核、業務規則都在這一層。繞過它就等於繞過 RLS，
//!   而且業務規則會分裂成 Rust 與 Python 兩份，遲早漂移。

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/internal/participants/resolve", post(resolve_participants))
        .route("/internal/human-tasks", post(create_human_tasks))
        .route("/internal/human-tasks/cancel", post(cancel_human_tasks))
        .route("/internal/actions/run", post(run_action))
        .route("/internal/notifications", post(send_notification))
        .route("/internal/instances/{id}/finish", post(finish_instance))
}

/// 驗證內部密鑰並取出 tenant_id
///
/// 密鑰用常數時間比較。字串的 == 會在第一個不同的位元組就返回，
/// 攻擊者可藉由計時差異逐位元組猜出密鑰。
fn authorize(state: &AppState, headers: &HeaderMap) -> ApiResult<Uuid> {
    let expected = state.internal_token.as_deref().ok_or_else(|| {
        ApiError::Forbidden("未設定 INTERNAL_API_TOKEN，internal API 已停用".into())
    })?;

    let provided = headers
        .get("x-internal-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    if !constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
        return Err(ApiError::Unauthorized("內部密鑰錯誤".into()));
    }

    let tenant = headers
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::BadRequest("缺少 X-Tenant-Id".into()))?;

    Uuid::parse_str(tenant).map_err(|_| ApiError::BadRequest("X-Tenant-Id 格式錯誤".into()))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ── 參與者解析 ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct ResolveBody {
    #[allow(dead_code)]
    instance_id: Uuid,
    spec: Value,
    #[serde(default)]
    business_object: Value,
}

#[derive(Serialize)]
pub struct ResolvedParticipant {
    id: String,
    display: String,
    role: Option<String>,
    kind: String,
}

/// 解析簽核人
///
/// resolver 的型別由這裡解讀，因為組織結構與角色指派在資料庫裡。
/// Python 端只負責把結果交給 Workflow。
async fn resolve_participants(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ResolveBody>,
) -> ApiResult<Json<Vec<ResolvedParticipant>>> {
    let tenant_id = authorize(&state, &headers)?;
    let mut tx = state.db.tenant_tx(tenant_id).await?;

    let spec_type = body.spec.get("type").and_then(Value::as_str).unwrap_or("");

    let users = match spec_type {
        "role" => {
            let role = body.spec.get("value").and_then(Value::as_str).unwrap_or("");
            persistence::participant::by_role(&mut tx, role).await?
        }
        "user" => {
            let id = body
                .spec
                .get("user_id")
                .and_then(Value::as_str)
                .and_then(|s| Uuid::parse_str(s).ok());
            match id {
                Some(id) => persistence::participant::by_id(&mut tx, id).await?,
                None => vec![],
            }
        }
        "initiator" => {
            let id = initiator_of(&mut tx, body.instance_id).await?;
            persistence::participant::by_id(&mut tx, id).await?
        }
        "manager_of" => {
            // of = "initiator" 是目前唯一支援的形式。
            // 指定其他人的主管需要先解析那個人是誰，留待有需求時再做。
            let id = initiator_of(&mut tx, body.instance_id).await?;
            persistence::participant::manager_of(&mut tx, id).await?
        }
        "department_manager" => {
            let id = initiator_of(&mut tx, body.instance_id).await?;
            persistence::participant::department_manager_of(&mut tx, id).await?
        }
        "external_contacts" => {
            // 外部聯絡人存在業務物件裡，不在使用者表。
            // 這裡不查資料庫，直接從傳入的 business_object 取。
            let path = body.spec.get("path").and_then(Value::as_str).unwrap_or("");
            tx.commit().await?;
            return Ok(Json(external_contacts(&body.business_object, path)));
        }
        other => {
            tx.commit().await?;
            return Err(ApiError::BadRequest(format!(
                "尚未支援的簽核人型別：{other}"
            )));
        }
    };

    tx.commit().await?;

    Ok(Json(
        users
            .into_iter()
            .map(|u| ResolvedParticipant {
                id: u.id.to_string(),
                display: u.name,
                role: u.role,
                kind: "internal".into(),
            })
            .collect(),
    ))
}

async fn initiator_of(tx: &mut persistence::TenantTx<'_>, instance_id: Uuid) -> ApiResult<Uuid> {
    let instance = persistence::instance::find_by_id(tx, instance_id).await?;
    instance
        .started_by
        .ok_or_else(|| ApiError::Conflict("流程沒有發起人，無法解析簽核人".into()))
}

/// 從業務物件取外部聯絡人
///
/// path 形如 quotation.customer_contact_ids，值可以是
/// 字串陣列或物件陣列（帶 id 與 name）。
fn external_contacts(business: &Value, path: &str) -> Vec<ResolvedParticipant> {
    let mut current = business;
    for segment in path.split('.').filter(|s| !s.is_empty()) {
        match current.get(segment) {
            Some(next) => current = next,
            None => return vec![],
        }
    }

    let Some(items) = current.as_array() else {
        return vec![];
    };

    items
        .iter()
        .filter_map(|item| {
            if let Some(s) = item.as_str() {
                return Some(ResolvedParticipant {
                    id: s.to_string(),
                    display: s.to_string(),
                    role: None,
                    kind: "external".into(),
                });
            }
            let id = item.get("id").and_then(Value::as_str)?;
            Some(ResolvedParticipant {
                id: id.to_string(),
                display: item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .to_string(),
                role: None,
                kind: "external".into(),
            })
        })
        .collect()
}

// ── 人工待辦 ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateTasksBody {
    instance_id: Uuid,
    node_id: String,
    node_label: Option<String>,
    participants: Vec<ParticipantRef>,
    #[serde(default = "default_kind")]
    participant_kind: String,
    form_key: Option<String>,
    due_at: Option<String>,
}

fn default_kind() -> String {
    "internal".into()
}

#[derive(Deserialize)]
pub struct ParticipantRef {
    id: String,
    #[serde(default)]
    role: Option<String>,
}

#[derive(Serialize)]
pub struct CreatedTasks {
    task_ids: Vec<String>,
}

async fn create_human_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateTasksBody>,
) -> ApiResult<Json<CreatedTasks>> {
    let tenant_id = authorize(&state, &headers)?;

    let due_at = body
        .due_at
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));

    let mut tx = state.db.tenant_tx(tenant_id).await?;

    let mut task_ids = Vec::with_capacity(body.participants.len());
    for p in &body.participants {
        // 外部聯絡人的 id 不是 app_user 的主鍵，存成 role 欄位的形式。
        // 正式支援客戶 Portal 時會有獨立的 external_contact 表。
        let user_id = Uuid::parse_str(&p.id).ok();

        let task = persistence::human_task::create(
            &mut tx,
            persistence::human_task::CreateTask {
                instance_id: body.instance_id,
                node_id: body.node_id.clone(),
                node_label: body.node_label.clone(),
                assignee_user_id: user_id,
                assignee_role: if user_id.is_none() {
                    Some(p.id.clone())
                } else {
                    p.role.clone()
                },
                participant_kind: body.participant_kind.clone(),
                form_key: body.form_key.clone(),
                due_at,
            },
        )
        .await?;

        task_ids.push(task.id.to_string());
    }

    tx.audit(
        persistence::AuditEvent::new("system", "human_task.create", "human_task")
            .target(body.instance_id.to_string())
            .payload(serde_json::json!({
                "node_id": body.node_id,
                "count": task_ids.len(),
            })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(CreatedTasks { task_ids }))
}

#[derive(Deserialize)]
pub struct CancelTasksBody {
    task_ids: Vec<String>,
    #[serde(default)]
    reason: String,
}

async fn cancel_human_tasks(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CancelTasksBody>,
) -> ApiResult<Json<Value>> {
    let tenant_id = authorize(&state, &headers)?;

    let ids: Vec<Uuid> = body
        .task_ids
        .iter()
        .filter_map(|s| Uuid::parse_str(s).ok())
        .collect();

    let mut tx = state.db.tenant_tx(tenant_id).await?;
    let cancelled = persistence::human_task::cancel_many(&mut tx, &ids, &body.reason).await?;
    tx.commit().await?;

    Ok(Json(serde_json::json!({ "cancelled": cancelled })))
}

// ── 系統動作與通知 ──────────────────────────────────────

#[derive(Deserialize)]
pub struct RunActionBody {
    #[allow(dead_code)]
    instance_id: Uuid,
    node_id: String,
    action: String,
    #[serde(default)]
    business_object: Value,
    idempotency_key: String,
}

/// 執行系統動作
///
/// 目前只記錄稽核並回傳成功。實際的 ERP 寫回、PDF 產生
/// 在後續階段接上（任務 2.x 與 3.x）。
///
/// 先做成端點而非等實作完成，是為了讓流程能真的跑完——
/// 否則 Interpreter 的 action 節點永遠測不到真實路徑。
async fn run_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RunActionBody>,
) -> ApiResult<Json<Value>> {
    let tenant_id = authorize(&state, &headers)?;

    let mut tx = state.db.tenant_tx(tenant_id).await?;
    tx.audit(
        persistence::AuditEvent::new("system", "action.run", "workflow_instance")
            .target(body.instance_id.to_string())
            .payload(serde_json::json!({
                "node_id": body.node_id,
                "action": body.action,
                "idempotency_key": body.idempotency_key,
            })),
    )
    .await?;
    tx.commit().await?;

    tracing::info!(
        action = %body.action,
        node = %body.node_id,
        "系統動作尚未實作，僅記錄稽核"
    );

    // 回傳要合併回業務物件的欄位。目前為空。
    let _ = body.business_object;
    Ok(Json(serde_json::json!({ "business_object": {} })))
}

#[derive(Deserialize)]
pub struct NotificationBody {
    instance_id: Uuid,
    node_id: String,
    channel: Vec<String>,
    #[serde(default)]
    to: Vec<String>,
    /// "reminder" 代表逾時前提醒，None 代表流程中的 notification 節點
    #[serde(default)]
    kind: Option<String>,
    /// 提醒專用：距離到期還有多久（ISO 8601）
    #[serde(default)]
    remaining: Option<String>,
}

/// 送出通知
///
/// `to` 帶的是 user id 或直接的 email（外部聯絡人）。
/// 前者要查出信箱，後者直接用——外部客戶不在 app_user 裡。
///
/// 寄信失敗不回錯誤：通知失敗不該讓整張單卡住，
/// 單據已經核准卻因為寄信失敗而流程中斷，業務上說不通。
/// 結果寫進稽核，可事後查。
async fn send_notification(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NotificationBody>,
) -> ApiResult<Json<Value>> {
    let tenant_id = authorize(&state, &headers)?;

    let mut tx = state.db.tenant_tx(tenant_id).await?;
    let recipients = resolve_recipients(&mut tx, &body.to).await?;

    let outcome = if body.channel.iter().any(|c| c == "email") {
        let mail = crate::mailer::Mail {
            to: recipients.clone(),
            subject: subject_of(&body),
            body: text_of(&body),
        };
        state.mailer.send(&mail).await
    } else {
        // LINE 尚未實作。明確標示而非假裝送出。
        crate::mailer::SendOutcome::Disabled
    };

    let (status, detail) = match &outcome {
        crate::mailer::SendOutcome::Sent { accepted } => ("sent", format!("{accepted} 封")),
        crate::mailer::SendOutcome::Disabled => ("disabled", "未設定或管道未支援".into()),
        crate::mailer::SendOutcome::Failed { reason } => ("failed", reason.clone()),
    };

    tx.audit(
        persistence::AuditEvent::new("system", "notification.send", "workflow_instance")
            .target(body.instance_id.to_string())
            .payload(serde_json::json!({
                "node_id": body.node_id,
                "kind": body.kind,
                "channel": body.channel,
                "to": body.to,
                "recipients": recipients,
                "status": status,
                "detail": detail,
            })),
    )
    .await?;
    tx.commit().await?;

    if let crate::mailer::SendOutcome::Failed { reason } = &outcome {
        tracing::warn!(node = %body.node_id, "通知寄送失敗：{reason}");
    }

    Ok(Json(serde_json::json!({
        "sent": status == "sent",
        "status": status,
    })))
}

/// 把 user id 轉成 email，非 UUID 的值視為 email 直接使用
///
/// 外部聯絡人的「id」就是 email 本身（resolver 型別 external_contacts），
/// 他們不在 app_user 裡，查不到是正常的。
async fn resolve_recipients(
    tx: &mut persistence::TenantTx<'_>,
    to: &[String],
) -> ApiResult<Vec<String>> {
    let mut ids: Vec<Uuid> = Vec::new();
    let mut direct: Vec<String> = Vec::new();

    for t in to {
        match Uuid::parse_str(t) {
            Ok(id) => ids.push(id),
            Err(_) => {
                if t.contains('@') {
                    direct.push(t.clone());
                }
            }
        }
    }

    let mut result = persistence::participant::emails_of(tx, &ids).await?;
    result.extend(direct);
    Ok(result)
}

fn subject_of(body: &NotificationBody) -> String {
    match body.kind.as_deref() {
        Some("reminder") => {
            let remaining = body.remaining.as_deref().unwrap_or("");
            if remaining.is_empty() {
                "【待辦提醒】您有待簽核的項目".into()
            } else {
                format!("【待辦提醒】{} 後到期", humanize(remaining))
            }
        }
        _ => "【流程通知】".into(),
    }
}

fn text_of(body: &NotificationBody) -> String {
    let mut lines = vec![
        format!("流程節點：{}", body.node_id),
        format!("單據編號：{}", body.instance_id),
    ];
    if let Some(r) = &body.remaining {
        lines.push(format!("剩餘時間：{}", humanize(r)));
    }
    lines.push(String::new());
    lines.push("請至系統處理：http://localhost:3040/tasks".into());
    lines.join("\n")
}

/// ISO 8601 duration 轉成人看得懂的說法
///
/// 信件裡寫「P1D 後到期」使用者看不懂。只處理常見的幾種，
/// 其餘原樣輸出——寧可顯示 ISO 字串，也不要算錯。
fn humanize(iso: &str) -> String {
    match iso {
        "P3D" => "3 天".into(),
        "P2D" => "2 天".into(),
        "P1D" => "1 天".into(),
        "PT12H" => "12 小時".into(),
        "PT8H" => "8 小時".into(),
        "PT6H" => "6 小時".into(),
        "PT1H" => "1 小時".into(),
        other => other.to_string(),
    }
}

// ── 流程結束回報 ────────────────────────────────────────

#[derive(Deserialize)]
pub struct FinishBody {
    status: String,
    output: Option<Value>,
    error: Option<String>,
}

/// 回報流程結束
///
/// Temporal 知道流程結束了，但收件匣與報表查的是 PostgreSQL。
/// 不回報的話單據會永遠顯示「進行中」。
async fn finish_instance(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<FinishBody>,
) -> ApiResult<Json<Value>> {
    let tenant_id = authorize(&state, &headers)?;

    let mut tx = state.db.tenant_tx(tenant_id).await?;

    let updated =
        persistence::instance::finish(&mut tx, id, &body.status, body.output, body.error.clone())
            .await?;

    if updated {
        // 流程結束時把殘留的待辦一併取消。Interpreter 正常結束時
        // 已經取消過，但流程被強制終止時不會執行清理。
        //
        // 只影響 PENDING 的。已決策的不動——那是使用者真實做過的事，
        // 蓋掉會讓稽核紀錄失真。實測時曾看到已核准的待辦變成
        // CANCELLED，追下去是因為決策繞過 API 直接送 Signal，
        // 資料庫從未收到那筆決策，並非這裡的問題。
        persistence::human_task::cancel_by_instance(&mut tx, id, "流程已結束").await?;

        tx.audit(
            persistence::AuditEvent::new("system", "instance.finish", "workflow_instance")
                .target(id.to_string())
                .payload(serde_json::json!({ "status": body.status })),
        )
        .await?;
    }

    tx.commit().await?;

    // updated = false 代表已經回報過。重送不是錯誤，
    // Temporal 的 Activity 重試本來就可能造成重複呼叫。
    Ok(Json(serde_json::json!({ "updated": updated })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 常數時間比較正確判斷相等() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret2"));
        assert!(!constant_time_eq(b"", b"x"));
    }

    #[test]
    fn 外部聯絡人支援字串陣列() {
        let business = json!({ "quotation": { "customer_contact_ids": ["a@x.com", "b@x.com"] } });
        let list = external_contacts(&business, "quotation.customer_contact_ids");

        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "a@x.com");
        assert_eq!(list[0].kind, "external");
    }

    #[test]
    fn 外部聯絡人支援物件陣列() {
        let business = json!({
            "quotation": { "customer_contact_ids": [{ "id": "c1", "name": "陳先生" }] }
        });
        let list = external_contacts(&business, "quotation.customer_contact_ids");

        assert_eq!(list[0].id, "c1");
        assert_eq!(list[0].display, "陳先生");
    }

    #[test]
    fn 路徑不存在時回空陣列() {
        // 讓 Interpreter 以「找不到參與者」明確失敗，
        // 比在這裡 panic 好追查
        let list = external_contacts(&json!({}), "quotation.missing");
        assert!(list.is_empty());
    }

    #[test]
    fn 值不是陣列時回空陣列() {
        let business = json!({ "quotation": { "customer_contact_ids": "not-an-array" } });
        let list = external_contacts(&business, "quotation.customer_contact_ids");
        assert!(list.is_empty());
    }
}
