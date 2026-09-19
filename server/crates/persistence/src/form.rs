//! 表單定義的資料存取
//!
//! 版本模型（見 migrations/0002）：
//!   草稿是 `version IS NULL` 的列，每個表單最多一份，由部分唯一索引保證。
//!   發布時分配版本號並轉為 PUBLISHED，之後內容不可再改，由 trigger 阻擋。
//!
//! 所有查詢都走 [`TenantTx`]，租戶隔離由 RLS 保證，
//! 因此 SQL 裡不需要也不應該再寫 `where tenant_id = ?`。
//! 重複條件會讓人誤以為隔離靠應用層，反而降低警覺。

use crate::{Error, Result, TenantTx};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FormDefinition {
    pub id: Uuid,
    pub form_key: String,
    pub business_object: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FormVersion {
    pub id: Uuid,
    pub form_id: Uuid,
    /// `None` 代表草稿
    pub version: Option<i32>,
    pub content: Value,
    pub status: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub published_by: Option<Uuid>,
    pub published_at: Option<DateTime<Utc>>,
}

impl FormVersion {
    pub fn is_draft(&self) -> bool {
        self.version.is_none()
    }
}

/// 表單摘要，供列表顯示
#[derive(Debug, Clone, Serialize)]
pub struct FormSummary {
    #[serde(flatten)]
    pub definition: FormDefinition,
    /// 最新已發布版本號
    pub published_version: Option<i32>,
    pub has_draft: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateForm {
    pub form_key: String,
    pub business_object: String,
    pub name: String,
    pub content: Value,
}

// ── 查詢 ────────────────────────────────────────────────

pub async fn list(tx: &mut TenantTx<'_>) -> Result<Vec<FormSummary>> {
    let rows = sqlx::query_as::<_, (Uuid, String, String, String, DateTime<Utc>, DateTime<Utc>, Option<i32>, bool)>(
        r#"
        select
            f.id, f.form_key, f.business_object, f.name, f.created_at, f.updated_at,
            (select max(v.version) from form_definition_version v
              where v.form_id = f.id and v.status = 'PUBLISHED'),
            exists (select 1 from form_definition_version v
                     where v.form_id = f.id and v.status = 'DRAFT')
        from form_definition f
        order by f.updated_at desc
        "#,
    )
    .fetch_all(tx.executor())
    .await
    .map_err(Error::from_db)?;

    Ok(rows
        .into_iter()
        .map(|(id, form_key, business_object, name, created_at, updated_at, published_version, has_draft)| {
            FormSummary {
                definition: FormDefinition { id, form_key, business_object, name, created_at, updated_at },
                published_version,
                has_draft,
            }
        })
        .collect())
}

pub async fn find_by_key(tx: &mut TenantTx<'_>, form_key: &str) -> Result<FormDefinition> {
    sqlx::query_as::<_, FormDefinition>(
        "select id, form_key, business_object, name, created_at, updated_at
         from form_definition where form_key = $1",
    )
    .bind(form_key)
    .fetch_optional(tx.executor())
    .await
    .map_err(Error::from_db)?
    .ok_or_else(|| Error::not_found("form", form_key))
}

/// 取草稿。不存在時回 `None`，不是錯誤。
pub async fn get_draft(tx: &mut TenantTx<'_>, form_id: Uuid) -> Result<Option<FormVersion>> {
    sqlx::query_as::<_, FormVersion>(
        "select * from form_definition_version where form_id = $1 and status = 'DRAFT'",
    )
    .bind(form_id)
    .fetch_optional(tx.executor())
    .await
    .map_err(Error::from_db)
}

/// 取最新已發布版本
pub async fn get_latest_published(
    tx: &mut TenantTx<'_>,
    form_id: Uuid,
) -> Result<Option<FormVersion>> {
    sqlx::query_as::<_, FormVersion>(
        "select * from form_definition_version
         where form_id = $1 and status = 'PUBLISHED'
         order by version desc limit 1",
    )
    .bind(form_id)
    .fetch_optional(tx.executor())
    .await
    .map_err(Error::from_db)
}

/// 取某業務物件當前的已發布表單版本
///
/// 供流程實例在建立時鎖定表單版本（workflow_instance.form_version_id）。
///
/// 方向是「表單指定業務物件」——表單自己宣告服務哪個業務物件，
/// 這裡以業務物件反查。不是「流程指定表單」，
/// 後者會讓同一張表單被多個流程各自宣告一次，遲早不一致。
///
/// 只認 PUBLISHED。草稿是設計中的半成品，鎖它等於沒鎖——
/// 草稿隨時會變，而版本鎖定的目的正是「不要變」。
///
/// 查無表單時回 `None` 而非錯誤：不是每個業務物件都有表單
/// （例如純系統觸發的流程），沒有表單不該讓啟動流程失敗。
///
/// 同一個業務物件有多張已發布表單時取**最近發布**的。
/// 乾淨的租戶不該出現這種情況（一個業務物件一張表單），
/// 但測試殘留會造成。選最近發布的而非隨機，是為了讓同一份資料
/// 每次都得到同樣的答案——不確定的結果會讓
/// 「為什麼這張單綁到那張表單」無法追查。
pub async fn find_published_by_business_object(
    tx: &mut TenantTx<'_>,
    business_object: &str,
) -> Result<Option<FormVersion>> {
    sqlx::query_as::<_, FormVersion>(
        "select v.* from form_definition_version v
         join form_definition f on f.id = v.form_id
         where f.business_object = $1 and v.status = 'PUBLISHED'
         order by v.published_at desc nulls last, v.version desc
         limit 1",
    )
    .bind(business_object)
    .fetch_optional(tx.executor())
    .await
    .map_err(Error::from_db)
}

pub async fn get_version(
    tx: &mut TenantTx<'_>,
    form_id: Uuid,
    version: i32,
) -> Result<FormVersion> {
    sqlx::query_as::<_, FormVersion>(
        "select * from form_definition_version where form_id = $1 and version = $2",
    )
    .bind(form_id)
    .bind(version)
    .fetch_optional(tx.executor())
    .await
    .map_err(Error::from_db)?
    .ok_or_else(|| Error::not_found("form_version", format!("{form_id}/v{version}")))
}

pub async fn list_versions(tx: &mut TenantTx<'_>, form_id: Uuid) -> Result<Vec<FormVersion>> {
    sqlx::query_as::<_, FormVersion>(
        "select * from form_definition_version
         where form_id = $1
         order by version desc nulls first",
    )
    .bind(form_id)
    .fetch_all(tx.executor())
    .await
    .map_err(Error::from_db)
}

// ── 異動 ────────────────────────────────────────────────

/// 建立表單並附上第一份草稿
pub async fn create(
    tx: &mut TenantTx<'_>,
    input: CreateForm,
    actor: Uuid,
) -> Result<(FormDefinition, FormVersion)> {
    let tenant_id = tx.tenant_id();

    let form = sqlx::query_as::<_, FormDefinition>(
        "insert into form_definition (tenant_id, form_key, business_object, name, created_by)
         values ($1, $2, $3, $4, $5)
         returning id, form_key, business_object, name, created_at, updated_at",
    )
    .bind(tenant_id)
    .bind(&input.form_key)
    .bind(&input.business_object)
    .bind(&input.name)
    .bind(actor)
    .fetch_one(tx.executor())
    .await
    .map_err(Error::from_db)?;

    let draft = sqlx::query_as::<_, FormVersion>(
        "insert into form_definition_version (tenant_id, form_id, content, status, created_by)
         values ($1, $2, $3, 'DRAFT', $4)
         returning *",
    )
    .bind(tenant_id)
    .bind(form.id)
    .bind(&input.content)
    .bind(actor)
    .fetch_one(tx.executor())
    .await
    .map_err(Error::from_db)?;

    Ok((form, draft))
}

/// 儲存草稿。沒有草稿時自動建立，內容取自最新發布版本。
///
/// 這個「自動建立」行為是刻意的。使用者在 UI 上看到的是
/// 「編輯這張表單」，不會意識到草稿是獨立的資料列。
/// 若要求先呼叫「建立草稿」再儲存，前端得多一次往返且容易漏。
pub async fn save_draft(
    tx: &mut TenantTx<'_>,
    form_id: Uuid,
    content: &Value,
    actor: Uuid,
) -> Result<FormVersion> {
    if let Some(draft) = get_draft(tx, form_id).await? {
        return sqlx::query_as::<_, FormVersion>(
            "update form_definition_version
             set content = $1, created_by = $2
             where id = $3
             returning *",
        )
        .bind(content)
        .bind(actor)
        .bind(draft.id)
        .fetch_one(tx.executor())
        .await
        .map_err(Error::from_db);
    }

    let tenant_id = tx.tenant_id();
    sqlx::query_as::<_, FormVersion>(
        "insert into form_definition_version (tenant_id, form_id, content, status, created_by)
         values ($1, $2, $3, 'DRAFT', $4)
         returning *",
    )
    .bind(tenant_id)
    .bind(form_id)
    .bind(content)
    .bind(actor)
    .fetch_one(tx.executor())
    .await
    .map_err(Error::from_db)
}

/// 發布草稿為新版本
///
/// 版本號取 `max(version) + 1`，在同一交易內計算。
/// 併發發布時，第二筆會因 `form_version_unique` 部分唯一索引而失敗，
/// 回傳 Conflict 讓前端重試，不會產生重號。
pub async fn publish(tx: &mut TenantTx<'_>, form_id: Uuid, actor: Uuid) -> Result<FormVersion> {
    let draft = get_draft(tx, form_id)
        .await?
        .ok_or_else(|| Error::Conflict("沒有可發布的草稿".into()))?;

    let next: i32 = sqlx::query_scalar(
        "select coalesce(max(version), 0) + 1 from form_definition_version where form_id = $1",
    )
    .bind(form_id)
    .fetch_one(tx.executor())
    .await
    .map_err(Error::from_db)?;

    sqlx::query_as::<_, FormVersion>(
        "update form_definition_version
         set version = $1, status = 'PUBLISHED', published_by = $2, published_at = now()
         where id = $3
         returning *",
    )
    .bind(next)
    .bind(actor)
    .bind(draft.id)
    .fetch_one(tx.executor())
    .await
    .map_err(Error::from_db)
}

/// 捨棄草稿，回到最新發布版本的狀態
pub async fn discard_draft(tx: &mut TenantTx<'_>, form_id: Uuid) -> Result<bool> {
    let affected = sqlx::query(
        "delete from form_definition_version where form_id = $1 and status = 'DRAFT'",
    )
    .bind(form_id)
    .execute(tx.executor())
    .await
    .map_err(Error::from_db)?
    .rows_affected();

    Ok(affected > 0)
}
