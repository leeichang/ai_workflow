//! API 錯誤
//!
//! 回傳結構固定為 `{ code, message, details? }`。
//! `code` 是機器可讀的常數，前端依它決定行為；
//! `message` 給人看，可能因語系或版本而變，前端不應依賴其內容。

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("{0}")]
    Unauthorized(String),

    #[error("{0}")]
    Forbidden(String),

    #[error("找不到{kind}：{id}")]
    NotFound { kind: String, id: String },

    #[error("{0}")]
    Conflict(String),

    #[error("{0}")]
    BadRequest(String),

    /// 表單定義不符合 schema。details 帶完整錯誤清單供前端逐條顯示。
    #[error("{0}")]
    ValidationFailed(String),

    #[error("已發布的版本不可修改")]
    ImmutablePublished,

    /// 外部依賴（Temporal）不可用。
    ///
    /// 與 Internal 分開：這是暫時性的，呼叫端重試有意義，
    /// 而 500 通常代表程式錯誤，重試沒用。
    #[error("{0}")]
    ServiceUnavailable(String),

    #[error("{0}")]
    Internal(String),
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl ApiError {
    pub fn not_found(kind: impl Into<String>, id: impl std::fmt::Display) -> Self {
        Self::NotFound {
            kind: kind.into(),
            id: id.to_string(),
        }
    }

    fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized(_) => "UNAUTHORIZED",
            Self::Forbidden(_) => "FORBIDDEN",
            Self::NotFound { .. } => "NOT_FOUND",
            Self::Conflict(_) => "CONFLICT",
            Self::BadRequest(_) => "BAD_REQUEST",
            Self::ValidationFailed(_) => "VALIDATION_FAILED",
            Self::ImmutablePublished => "IMMUTABLE_PUBLISHED",
            Self::ServiceUnavailable(_) => "SERVICE_UNAVAILABLE",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound { .. } => StatusCode::NOT_FOUND,
            Self::Conflict(_) | Self::ImmutablePublished => StatusCode::CONFLICT,
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::ValidationFailed(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code();

        // 內部錯誤不把細節吐給前端，避免洩漏實作資訊
        let message = match &self {
            Self::Internal(detail) => {
                tracing::error!(error = %detail, "內部錯誤");
                "伺服器內部錯誤".to_string()
            }
            other => other.to_string(),
        };

        let details = match &self {
            Self::ValidationFailed(msg) => Some(json!({
                "errors": msg.split('；').collect::<Vec<_>>()
            })),
            _ => None,
        };

        (
            status,
            Json(ErrorBody {
                code,
                message,
                details,
            }),
        )
            .into_response()
    }
}

/// persistence 層錯誤轉成 API 錯誤
impl From<persistence::Error> for ApiError {
    fn from(e: persistence::Error) -> Self {
        use persistence::Error as E;
        match e {
            E::NotFound { kind, id } => Self::NotFound {
                kind: kind.to_string(),
                id,
            },
            E::Conflict(msg) => Self::Conflict(msg),
            E::ImmutablePublished => Self::ImmutablePublished,
            E::InvalidSchema(msg) => Self::ValidationFailed(msg),
            // 連線用了 superuser。這是設定錯誤，不是使用者的問題。
            E::SuperuserConnection => Self::Internal(e.to_string()),
            E::Sqlx(err) => Self::Internal(format!("資料庫錯誤：{err}")),
            E::Json(err) => Self::Internal(format!("序列化錯誤：{err}")),
        }
    }
}

impl From<domain::form_schema::ValidationError> for ApiError {
    fn from(e: domain::form_schema::ValidationError) -> Self {
        use domain::form_schema::ValidationError as V;
        match e {
            V::Invalid(msg) => Self::ValidationFailed(msg),
            V::BadSchema(msg) => Self::Internal(format!("schema 損壞：{msg}")),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
