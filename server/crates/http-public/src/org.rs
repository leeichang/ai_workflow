//! 組織
//!
//! 部門樹、員工清單與編輯，以及健康檢查。
//!
//! 見 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §3、§9、§12.1。
//!
//! ## 權限
//!
//! 讀取只要登入即可——組織結構是每個人都看得到的資訊
//! （下拉選單本來就查得到人與部門）。
//!
//! 寫入要 `admin`。**不給 designer**：改主管等於改簽核路徑，
//! designer 管的是表單與流程定義，不是人事。

use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/org/health", get(health))
        .route(
            "/org/departments",
            get(list_departments).post(create_department),
        )
        .route(
            "/org/departments/{id}",
            patch(update_department).delete(delete_department),
        )
        .route(
            "/org/departments/{id}/unlock-fields",
            post(unlock_department_fields),
        )
        .route("/org/employees", get(list_employees))
        .route("/org/employees/{id}", patch(update_employee))
        .route(
            "/org/employees/{id}/unlock-fields",
            post(unlock_employee_fields),
        )
}

// ── 健康檢查 ────────────────────────────────────────────

/// 組織健康檢查
///
/// 回答「哪些人現在送單會卡住」。導入期的價值在於問題**上線前**
/// 就浮出來，而不是使用者送單後才卡住。
///
/// 不做角色門檻：這是唯讀報表，且內容是組織結構本身——
/// 任何能看到員工清單的人都看得到同樣的資料。
async fn health(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<persistence::org_health::HealthReport>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    let report = persistence::org_health::check(&mut tx)
        .await
        .map_err(|e| ApiError::Internal(format!("組織健康檢查失敗：{e}")))?;

    tx.commit().await?;
    Ok(Json(report))
}

// ── 部門 ────────────────────────────────────────────────

async fn list_departments(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Vec<persistence::organization::Department>>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let rows = persistence::organization::list_departments(&mut tx).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn create_department(
    State(state): State<AppState>,
    actor: Actor,
    Json(body): Json<persistence::organization::NewDepartment>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    if body.code.trim().is_empty() || body.name.trim().is_empty() {
        return Err(ApiError::ValidationFailed("代碼與名稱不可空白".into()));
    }

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let id = persistence::organization::create_department(&mut tx, &body).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "org.department.create", "department")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(id.to_string())
            .payload(json!({ "code": body.code, "name": body.name })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "id": id })))
}

async fn update_department(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<persistence::organization::DepartmentPatch>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    persistence::organization::update_department(&mut tx, id, &body).await?;

    // 記錄改了哪些欄位而非改成什麼值：組織資料含個資，
    // 稽核紀錄不該變成另一份個資副本
    tx.audit(
        persistence::AuditEvent::new("internal", "org.department.update", "department")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(id.to_string())
            .payload(json!({ "fields": body.fields })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

/// 刪除部門
///
/// 只刪得掉沒有成員也沒有子部門的部門，條件在 persistence 層。
/// 有成員的部門應該改成停用而非刪除——見需求 §7。
async fn delete_department(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;

    // 先把名字查出來再刪。稽核紀錄要能讀，光有 uuid 事後查不出是誰
    let name: Option<String> =
        sqlx::query_scalar("select name from department where id = $1")
            .bind(id)
            .fetch_optional(tx.executor())
            .await
            .map_err(|e| ApiError::Internal(format!("查詢部門失敗：{e}")))?;

    persistence::organization::delete_department(&mut tx, id).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "org.department.delete", "department")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(id.to_string())
            .payload(json!({ "name": name })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

/// 要解除鎖定的欄位
#[derive(Deserialize)]
pub struct UnlockBody {
    pub fields: Vec<String>,
}

/// 改回跟隨來源系統（需求 §3 規則 3）
///
/// 解除後下一次同步就會覆蓋這些欄位。要稽核——
/// 這等同「放棄平台上的人工修正」，出事時要查得到是誰決定的。
async fn unlock_department_fields(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<UnlockBody>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    persistence::organization::unlock_fields(&mut tx, "department", id, &body.fields).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "org.department.unlock_fields", "department")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(id.to_string())
            .payload(json!({ "fields": body.fields })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

// ── 員工 ────────────────────────────────────────────────

async fn list_employees(
    State(state): State<AppState>,
    actor: Actor,
    Query(filter): Query<persistence::organization::EmployeeFilter>,
) -> ApiResult<Json<Vec<persistence::organization::Employee>>> {
    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    let rows = persistence::organization::list_employees(&mut tx, &filter).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn update_employee(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<persistence::organization::EmployeePatch>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    persistence::organization::update_employee(&mut tx, id, &body).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "org.employee.update", "app_user")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(id.to_string())
            .payload(json!({ "fields": body.fields })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

async fn unlock_employee_fields(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    Json(body): Json<UnlockBody>,
) -> ApiResult<Json<serde_json::Value>> {
    actor.require_any(&["admin"])?;

    let mut tx = state.db.tenant_tx(actor.tenant_id).await?;
    persistence::organization::unlock_fields(&mut tx, "app_user", id, &body.fields).await?;

    tx.audit(
        persistence::AuditEvent::new("internal", "org.employee.unlock_fields", "app_user")
            .actor(actor.user_id.to_string(), actor.name.clone())
            .target(id.to_string())
            .payload(json!({ "fields": body.fields })),
    )
    .await?;

    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}
