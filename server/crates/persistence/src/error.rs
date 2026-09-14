use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    /// 連線角色是 superuser，RLS 會被完全繞過。啟動時即拒絕。
    #[error(
        "資料庫連線使用 superuser，Row Level Security 會被繞過。\
         請改用非 superuser 角色（例如 workflow_app）連線。\
         詳見 server/migrations/README.md"
    )]
    SuperuserConnection,

    #[error("找不到資源：{kind} {id}")]
    NotFound { kind: &'static str, id: String },

    #[error("衝突：{0}")]
    Conflict(String),

    #[error("已發布的版本不可修改")]
    ImmutablePublished,

    #[error("表單定義不符合 schema：{0}")]
    InvalidSchema(String),

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub fn not_found(kind: &'static str, id: impl std::fmt::Display) -> Self {
        Self::NotFound {
            kind,
            id: id.to_string(),
        }
    }

    /// 將資料庫層的約束違反轉成有意義的錯誤
    ///
    /// trigger 丟出的 restrict_violation 與唯一索引衝突，
    /// 若直接回傳原始 SQL 錯誤訊息，前端無法判斷該怎麼處理。
    pub fn from_db(e: sqlx::Error) -> Self {
        let Some(db_err) = e.as_database_error() else {
            return Self::Sqlx(e);
        };

        match db_err.code().as_deref() {
            // unique_violation
            Some("23505") => Self::Conflict(
                db_err
                    .constraint()
                    .map(|c| format!("違反唯一約束：{c}"))
                    .unwrap_or_else(|| "資源已存在".into()),
            ),
            // restrict_violation，由 forbid_published_mutation 等 trigger 丟出
            Some("2F004") | Some("23001") => Self::ImmutablePublished,
            // check_violation
            Some("23514") => Self::Conflict(
                db_err
                    .constraint()
                    .map(|c| format!("違反檢查約束：{c}"))
                    .unwrap_or_else(|| "資料不符合約束".into()),
            ),
            _ => {
                // RLS policy 違反不會有專屬 code，靠訊息判斷
                let msg = db_err.message();
                if msg.contains("row-level security") || msg.contains("policy") {
                    Self::Conflict("操作被租戶隔離政策拒絕".into())
                } else if msg.contains("不可修改") || msg.contains("不可") {
                    Self::ImmutablePublished
                } else {
                    Self::Sqlx(e)
                }
            }
        }
    }
}
