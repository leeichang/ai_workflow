//! 資料存取層
//!
//! 租戶隔離的關鍵在 [`tenant_tx`]。它是取得資料庫連線的**唯一**入口，
//! 保證每筆查詢都在設定好 `app.tenant_id` 的交易內執行。
//!
//! 設計上刻意不提供「拿到裸連線」的方法。若提供了，總有一天會有人
//! 為了圖方便而繞過租戶設定，那時 RLS 就形同虛設，且不會有任何錯誤訊息。

pub mod error;
pub mod form;
pub mod human_task;
pub mod instance;
pub mod participant;
pub mod workflow;

use sqlx::postgres::{PgPoolOptions, PgQueryResult};
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Duration;
use uuid::Uuid;

pub use error::{Error, Result};

/// 資料庫連線池
///
/// 連線使用者必須是非 superuser 的 `workflow_app`。
/// superuser 會完全繞過 RLS，連 `FORCE ROW LEVEL SECURITY` 都擋不住。
#[derive(Clone, Debug)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .min_connections(2)
            .acquire_timeout(Duration::from_secs(5))
            .idle_timeout(Duration::from_secs(600))
            .connect(url)
            .await?;

        let db = Self { pool };
        db.assert_not_superuser().await?;
        Ok(db)
    }

    /// 啟動時檢查連線角色不是 superuser。
    ///
    /// 這個檢查很重要。開發初期曾誤用容器預設的 superuser 連線，
    /// 導致租戶 A 查得到租戶 B 的資料，而 policy 定義完全正確、
    /// 沒有任何錯誤訊息。與其等到出事，不如啟動就拒絕。
    async fn assert_not_superuser(&self) -> Result<()> {
        let is_super: Option<bool> =
            sqlx::query_scalar("select usesuper from pg_user where usename = current_user")
                .fetch_optional(&self.pool)
                .await?
                .flatten();

        match is_super {
            Some(true) => Err(Error::SuperuserConnection),
            _ => Ok(()),
        }
    }

    /// 開啟綁定租戶的交易。**取得連線的唯一入口。**
    ///
    /// 用 `SET LOCAL` 而非 `SET`：前者只在交易內有效，交易結束自動清除。
    /// 若用 `SET`，設定會留在連線上，回到連線池後污染下一個請求，
    /// 造成跨租戶資料外洩。
    pub async fn tenant_tx(&self, tenant_id: Uuid) -> Result<TenantTx<'_>> {
        let mut tx = self.pool.begin().await?;

        // set_config 的第三個參數 true 等同 SET LOCAL
        sqlx::query("select set_config('app.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *tx)
            .await?;

        Ok(TenantTx { tx, tenant_id })
    }

    /// 不綁租戶的連線。僅供登入查詢與平台管理使用。
    ///
    /// 刻意命名得刺眼，讓 code review 時容易注意到。
    /// 用它查詢帶 `tenant_id` 的表會因 RLS 而回空。
    pub fn pool_without_tenant_isolation(&self) -> &PgPool {
        &self.pool
    }

    pub async fn health(&self) -> Result<()> {
        sqlx::query("select 1").execute(&self.pool).await?;
        Ok(())
    }
}

/// 綁定租戶的交易
///
/// 未呼叫 [`commit`](Self::commit) 即 drop 時自動 rollback，
/// 這是 sqlx 的預設行為。
pub struct TenantTx<'a> {
    tx: Transaction<'a, Postgres>,
    tenant_id: Uuid,
}

impl<'a> TenantTx<'a> {
    pub fn tenant_id(&self) -> Uuid {
        self.tenant_id
    }

    /// 取得底層執行器供查詢使用
    pub fn executor(&mut self) -> &mut sqlx::PgConnection {
        &mut self.tx
    }

    pub async fn commit(self) -> Result<()> {
        self.tx.commit().await?;
        Ok(())
    }

    pub async fn rollback(self) -> Result<()> {
        self.tx.rollback().await?;
        Ok(())
    }

    /// 寫入稽核事件
    ///
    /// 與業務操作在同一交易內，確保「操作成功但稽核漏記」不可能發生。
    pub async fn audit(&mut self, event: AuditEvent<'_>) -> Result<PgQueryResult> {
        sqlx::query(
            r#"
            insert into audit_event
                (tenant_id, actor_kind, actor_id, actor_name,
                 action, target_type, target_id, payload, ip, user_agent, request_id)
            values ($1, $2, $3, $4, $5, $6, $7, $8, $9::inet, $10, $11)
            "#,
        )
        .bind(self.tenant_id)
        .bind(event.actor_kind)
        .bind(event.actor_id)
        .bind(event.actor_name)
        .bind(event.action)
        .bind(event.target_type)
        .bind(event.target_id)
        .bind(event.payload)
        .bind(event.ip)
        .bind(event.user_agent)
        .bind(event.request_id)
        .execute(&mut *self.tx)
        .await
        .map_err(Into::into)
    }
}

/// 稽核事件
#[derive(Debug, Default)]
pub struct AuditEvent<'a> {
    /// internal、external 或 system
    pub actor_kind: &'a str,
    pub actor_id: Option<String>,
    /// 當下的顯示名。人員日後被刪除時，稽核紀錄仍可讀。
    pub actor_name: Option<String>,
    /// 例如 form.publish、task.approve
    pub action: &'a str,
    pub target_type: &'a str,
    pub target_id: Option<String>,
    pub payload: serde_json::Value,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub request_id: Option<String>,
}

impl<'a> AuditEvent<'a> {
    pub fn new(actor_kind: &'a str, action: &'a str, target_type: &'a str) -> Self {
        Self {
            actor_kind,
            action,
            target_type,
            payload: serde_json::json!({}),
            ..Default::default()
        }
    }

    pub fn actor(mut self, id: impl Into<String>, name: impl Into<String>) -> Self {
        self.actor_id = Some(id.into());
        self.actor_name = Some(name.into());
        self
    }

    pub fn target(mut self, id: impl Into<String>) -> Self {
        self.target_id = Some(id.into());
        self
    }

    pub fn payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = payload;
        self
    }
}
