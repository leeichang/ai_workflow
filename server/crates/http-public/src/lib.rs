//! 內部應用 API
//!
//! 路由組裝與共用狀態。

pub mod auth;
pub mod error;
pub mod forms;
pub mod permission_write;
pub mod permissions;

use axum::extract::FromRef;
use axum::routing::get;
use axum::{Json, Router};
use persistence::Db;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use auth::{Actor, JwtKeys};
pub use error::{ApiError, ApiResult};

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub jwt: JwtKeys,
}

impl FromRef<AppState> for JwtKeys {
    fn from_ref(state: &AppState) -> Self {
        state.jwt.clone()
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/auth/login", axum::routing::post(login))
        .route("/me", get(me))
        .merge(forms::routes())
        .merge(permissions::routes())
        .merge(permission_write::routes())
        .with_state(state)
}

async fn health(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> ApiResult<Json<serde_json::Value>> {
    state.db.health().await?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

// ── 登入 ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginBody {
    pub tenant_code: String,
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub user: UserInfo,
}

#[derive(Serialize)]
pub struct UserInfo {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub tenant_id: Uuid,
    pub roles: Vec<String>,
}

/// 登入
///
/// 登入前還不知道租戶，因此必須用不綁租戶的連線查詢。
/// 這是少數合法繞過 RLS 的場景，故以 tenant_code 為進入點，
/// 查到使用者後立刻切回綁定租戶的模式。
async fn login(
    axum::extract::State(state): axum::extract::State<AppState>,
    Json(body): Json<LoginBody>,
) -> ApiResult<Json<LoginResponse>> {
    let pool = state.db.pool_without_tenant_isolation();

    // tenant 表不帶 tenant_id，不受 RLS 約束
    let tenant: Option<(Uuid, String)> =
        sqlx::query_as("select id, status from tenant where code = $1")
            .bind(&body.tenant_code)
            .fetch_optional(pool)
            .await
            .map_err(|e| ApiError::Internal(format!("查詢租戶失敗：{e}")))?;

    let Some((tenant_id, status)) = tenant else {
        // 不透露租戶是否存在，統一回帳密錯誤
        return Err(ApiError::Unauthorized("帳號或密碼錯誤".into()));
    };
    if status != "ACTIVE" {
        return Err(ApiError::Forbidden("租戶已停用".into()));
    }

    // 切回綁定租戶，後續查詢受 RLS 保護
    let mut tx = state.db.tenant_tx(tenant_id).await?;

    let row: Option<(Uuid, String, String, String, String)> = sqlx::query_as(
        "select id, name, email, password_hash, status from app_user where email = $1",
    )
    .bind(&body.email)
    .fetch_optional(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("查詢使用者失敗：{e}")))?;

    let Some((user_id, name, email, password_hash, user_status)) = row else {
        return Err(ApiError::Unauthorized("帳號或密碼錯誤".into()));
    };

    if !auth::verify_password(&body.password, &password_hash) {
        return Err(ApiError::Unauthorized("帳號或密碼錯誤".into()));
    }
    if user_status != "ACTIVE" {
        return Err(ApiError::Forbidden("帳號已停用".into()));
    }

    let roles: Vec<String> = sqlx::query_scalar(
        "select r.code from user_role ur join role r on r.id = ur.role_id where ur.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(tx.executor())
    .await
    .map_err(|e| ApiError::Internal(format!("查詢角色失敗：{e}")))?;

    sqlx::query("update app_user set last_login_at = now() where id = $1")
        .bind(user_id)
        .execute(tx.executor())
        .await
        .map_err(|e| ApiError::Internal(format!("更新登入時間失敗：{e}")))?;

    tx.audit(
        persistence::AuditEvent::new("internal", "auth.login", "app_user")
            .actor(user_id.to_string(), name.clone())
            .target(user_id.to_string()),
    )
    .await?;

    tx.commit().await?;

    let token = state
        .jwt
        .issue(user_id, tenant_id, name.clone(), roles.clone())?;

    Ok(Json(LoginResponse {
        access_token: token,
        user: UserInfo {
            id: user_id,
            name,
            email,
            tenant_id,
            roles,
        },
    }))
}

async fn me(actor: Actor) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "id": actor.user_id,
        "tenant_id": actor.tenant_id,
        "name": actor.name,
        "roles": actor.roles,
    }))
}
