//! 流程定義的資料存取
//!
//! 版本模型與 form 模組完全相同。兩者刻意保持一致，
//! 讓「設計後發布」的心智模型在整個平台統一。

use crate::{Error, Result, TenantTx};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WorkflowDefinition {
    pub id: Uuid,
    pub workflow_key: String,
    pub business_object: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WorkflowVersion {
    pub id: Uuid,
    pub workflow_id: Uuid,
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

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowSummary {
    #[serde(flatten)]
    pub definition: WorkflowDefinition,
    pub published_version: Option<i32>,
    pub has_draft: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkflow {
    pub workflow_key: String,
    pub business_object: String,
    pub name: String,
    pub description: Option<String>,
    pub content: Value,
}

// ── 查詢 ────────────────────────────────────────────────

pub async fn list(tx: &mut TenantTx<'_>) -> Result<Vec<WorkflowSummary>> {
    type Row = (
        Uuid,
        String,
        String,
        String,
        Option<String>,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<i32>,
        bool,
    );

    let rows = sqlx::query_as::<_, Row>(
        r#"
        select
            w.id, w.workflow_key, w.business_object, w.name, w.description,
            w.created_at, w.updated_at,
            (select max(v.version) from workflow_definition_version v
              where v.workflow_id = w.id and v.status = 'PUBLISHED'),
            exists (select 1 from workflow_definition_version v
                     where v.workflow_id = w.id and v.status = 'DRAFT')
        from workflow_definition w
        order by w.updated_at desc
        "#,
    )
    .fetch_all(tx.executor())
    .await
    .map_err(Error::from_db)?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                workflow_key,
                business_object,
                name,
                description,
                created_at,
                updated_at,
                published_version,
                has_draft,
            )| WorkflowSummary {
                definition: WorkflowDefinition {
                    id,
                    workflow_key,
                    business_object,
                    name,
                    description,
                    created_at,
                    updated_at,
                },
                published_version,
                has_draft,
            },
        )
        .collect())
}

pub async fn find_by_key(tx: &mut TenantTx<'_>, key: &str) -> Result<WorkflowDefinition> {
    sqlx::query_as::<_, WorkflowDefinition>(
        "select id, workflow_key, business_object, name, description, created_at, updated_at
         from workflow_definition where workflow_key = $1",
    )
    .bind(key)
    .fetch_optional(tx.executor())
    .await
    .map_err(Error::from_db)?
    .ok_or_else(|| Error::not_found("workflow", key))
}

pub async fn get_draft(tx: &mut TenantTx<'_>, workflow_id: Uuid) -> Result<Option<WorkflowVersion>> {
    sqlx::query_as::<_, WorkflowVersion>(
        "select * from workflow_definition_version
         where workflow_id = $1 and status = 'DRAFT'",
    )
    .bind(workflow_id)
    .fetch_optional(tx.executor())
    .await
    .map_err(Error::from_db)
}

pub async fn get_latest_published(
    tx: &mut TenantTx<'_>,
    workflow_id: Uuid,
) -> Result<Option<WorkflowVersion>> {
    sqlx::query_as::<_, WorkflowVersion>(
        "select * from workflow_definition_version
         where workflow_id = $1 and status = 'PUBLISHED'
         order by version desc limit 1",
    )
    .bind(workflow_id)
    .fetch_optional(tx.executor())
    .await
    .map_err(Error::from_db)
}

pub async fn get_version(
    tx: &mut TenantTx<'_>,
    workflow_id: Uuid,
    version: i32,
) -> Result<WorkflowVersion> {
    sqlx::query_as::<_, WorkflowVersion>(
        "select * from workflow_definition_version where workflow_id = $1 and version = $2",
    )
    .bind(workflow_id)
    .bind(version)
    .fetch_optional(tx.executor())
    .await
    .map_err(Error::from_db)?
    .ok_or_else(|| Error::not_found("workflow_version", format!("{workflow_id}/v{version}")))
}

pub async fn list_versions(
    tx: &mut TenantTx<'_>,
    workflow_id: Uuid,
) -> Result<Vec<WorkflowVersion>> {
    sqlx::query_as::<_, WorkflowVersion>(
        "select * from workflow_definition_version
         where workflow_id = $1
         order by version desc nulls first",
    )
    .bind(workflow_id)
    .fetch_all(tx.executor())
    .await
    .map_err(Error::from_db)
}

// ── 異動 ────────────────────────────────────────────────

pub async fn create(
    tx: &mut TenantTx<'_>,
    input: CreateWorkflow,
    actor: Uuid,
) -> Result<(WorkflowDefinition, WorkflowVersion)> {
    let tenant_id = tx.tenant_id();

    let definition = sqlx::query_as::<_, WorkflowDefinition>(
        "insert into workflow_definition
            (tenant_id, workflow_key, business_object, name, description, created_by)
         values ($1, $2, $3, $4, $5, $6)
         returning id, workflow_key, business_object, name, description, created_at, updated_at",
    )
    .bind(tenant_id)
    .bind(&input.workflow_key)
    .bind(&input.business_object)
    .bind(&input.name)
    .bind(&input.description)
    .bind(actor)
    .fetch_one(tx.executor())
    .await
    .map_err(Error::from_db)?;

    let draft = sqlx::query_as::<_, WorkflowVersion>(
        "insert into workflow_definition_version
            (tenant_id, workflow_id, content, status, created_by)
         values ($1, $2, $3, 'DRAFT', $4)
         returning *",
    )
    .bind(tenant_id)
    .bind(definition.id)
    .bind(&input.content)
    .bind(actor)
    .fetch_one(tx.executor())
    .await
    .map_err(Error::from_db)?;

    Ok((definition, draft))
}

pub async fn save_draft(
    tx: &mut TenantTx<'_>,
    workflow_id: Uuid,
    content: &Value,
    actor: Uuid,
) -> Result<WorkflowVersion> {
    if let Some(draft) = get_draft(tx, workflow_id).await? {
        return sqlx::query_as::<_, WorkflowVersion>(
            "update workflow_definition_version
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
    sqlx::query_as::<_, WorkflowVersion>(
        "insert into workflow_definition_version
            (tenant_id, workflow_id, content, status, created_by)
         values ($1, $2, $3, 'DRAFT', $4)
         returning *",
    )
    .bind(tenant_id)
    .bind(workflow_id)
    .bind(content)
    .bind(actor)
    .fetch_one(tx.executor())
    .await
    .map_err(Error::from_db)
}

pub async fn publish(
    tx: &mut TenantTx<'_>,
    workflow_id: Uuid,
    actor: Uuid,
) -> Result<WorkflowVersion> {
    let draft = get_draft(tx, workflow_id)
        .await?
        .ok_or_else(|| Error::Conflict("沒有可發布的草稿".into()))?;

    let next: i32 = sqlx::query_scalar(
        "select coalesce(max(version), 0) + 1
         from workflow_definition_version where workflow_id = $1",
    )
    .bind(workflow_id)
    .fetch_one(tx.executor())
    .await
    .map_err(Error::from_db)?;

    sqlx::query_as::<_, WorkflowVersion>(
        "update workflow_definition_version
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

pub async fn discard_draft(tx: &mut TenantTx<'_>, workflow_id: Uuid) -> Result<bool> {
    let affected = sqlx::query(
        "delete from workflow_definition_version where workflow_id = $1 and status = 'DRAFT'",
    )
    .bind(workflow_id)
    .execute(tx.executor())
    .await
    .map_err(Error::from_db)?
    .rows_affected();

    Ok(affected > 0)
}

// ── 驗證用的租戶資料 ────────────────────────────────────

/// 載入圖結構驗證所需的租戶資料
pub async fn load_validation_data(tx: &mut TenantTx<'_>) -> Result<(Vec<String>, Vec<String>)> {
    let roles: Vec<String> = sqlx::query_scalar("select code from role")
        .fetch_all(tx.executor())
        .await
        .map_err(Error::from_db)?;

    // Action Registry 目前是硬編碼清單。Activity 實作完成後改為查表。
    let actions = vec![
        "quotation.publish".to_string(),
        "odoo.create_sale_order".to_string(),
        "odoo.create_purchase_order".to_string(),
        "notify.email".to_string(),
        "notify.line".to_string(),
    ];

    Ok((roles, actions))
}
