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
pub mod sandbox;
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

        // 沙箱屬性在這裡取一次，讓呼叫端不必各自查 tenant 表。
        //
        // 多一次 PK 查詢的代價換的是：「忘記判斷 is_sandbox」這種錯誤
        // 更難發生。通知的收件人改寫、標題前綴、授權開洞都要用它，
        // 靠每一處記得自己查，遲早漏一處——而漏掉的那一處
        // 會把沙箱的信寄給真實的同事。
        let is_sandbox: Option<bool> =
            sqlx::query_scalar("select is_sandbox from tenant where id = $1")
                .bind(tenant_id)
                .fetch_optional(&mut *tx)
                .await?;

        Ok(TenantTx {
            tx,
            tenant_id,
            is_sandbox: is_sandbox.unwrap_or(false),
        })
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
    is_sandbox: bool,
}

impl<'a> TenantTx<'a> {
    pub fn tenant_id(&self) -> Uuid {
        self.tenant_id
    }

    /// 這個交易是否在沙箱租戶內
    ///
    /// 通知的收件人改寫與標題前綴都看它。正式租戶永遠是 false，
    /// 所以那些改寫在正式環境不可能被觸發。
    pub fn is_sandbox(&self) -> bool {
        self.is_sandbox
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
                (tenant_id, actor_kind, actor_id, actor_name, acting_as,
                 action, target_type, target_id, payload, ip, user_agent, request_id)
            values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::inet, $11, $12)
            "#,
        )
        .bind(self.tenant_id)
        .bind(event.actor_kind)
        .bind(event.actor_id)
        .bind(event.actor_name)
        .bind(event.acting_as.as_deref().map(|s| s.parse::<Uuid>()).transpose()
              .map_err(|_| Error::Conflict("acting_as 必須是 uuid".into()))?)
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
    /// 模擬簽核時被扮演的對象。正式操作為 None。
    ///
    /// 與 actor_id 並存而非取代它：只記 acting_as 的話，
    /// 稽核會顯示張文華核准了某張單，而他根本不知道有這回事。
    pub acting_as: Option<String>,
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

    /// 標註這次操作是在扮演誰
    ///
    /// 新增 builder 而非改 `actor()` 的簽章：`actor()` 有 15 個呼叫點，
    /// 改簽章要全部動到，而其中 14 個與模擬簽核無關。
    pub fn acting_as(mut self, id: impl Into<String>) -> Self {
        self.acting_as = Some(id.into());
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
