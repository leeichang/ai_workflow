//! 人工待辦的持久化
//!
//! 待辦是收件匣的資料來源，也是「流程卡在誰身上」的唯一答案。
//! Temporal 知道流程在等 Signal，但不知道要等誰——那是業務資訊。

use crate::{Result, TenantTx};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct HumanTask {
    pub id: Uuid,
    pub instance_id: Uuid,
    pub node_id: String,
    pub node_label: Option<String>,
    pub assignee_user_id: Option<Uuid>,
    pub assignee_role: Option<String>,
    pub participant_kind: String,
    pub form_key: Option<String>,
    pub status: String,
    pub decision: Option<String>,
    pub comment: Option<String>,
    pub due_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct CreateTask {
    pub instance_id: Uuid,
    pub node_id: String,
    pub node_label: Option<String>,
    pub assignee_user_id: Option<Uuid>,
    pub assignee_role: Option<String>,
    pub participant_kind: String,
    pub form_key: Option<String>,
    pub due_at: Option<DateTime<Utc>>,
}

pub async fn create(tx: &mut TenantTx<'_>, input: CreateTask) -> Result<HumanTask> {
    let row = sqlx::query_as::<_, HumanTask>(
        r#"
        insert into human_task (
            tenant_id, instance_id, node_id, node_label,
            assignee_user_id, assignee_role, participant_kind, form_key, due_at
        )
        values (current_setting('app.tenant_id')::uuid, $1, $2, $3, $4, $5, $6, $7, $8)
        returning id, instance_id, node_id, node_label, assignee_user_id,
                  assignee_role, participant_kind, form_key, status, decision,
                  comment, due_at, created_at, decided_at
        "#,
    )
    .bind(input.instance_id)
    .bind(&input.node_id)
    .bind(&input.node_label)
    .bind(input.assignee_user_id)
    .bind(&input.assignee_role)
    .bind(&input.participant_kind)
    .bind(&input.form_key)
    .bind(input.due_at)
    .fetch_one(tx.executor())
    .await?;

    Ok(row)
}

pub async fn find_by_id(tx: &mut TenantTx<'_>, id: Uuid) -> Result<HumanTask> {
    sqlx::query_as::<_, HumanTask>(
        r#"
        select id, instance_id, node_id, node_label, assignee_user_id,
               assignee_role, participant_kind, form_key, status, decision,
               comment, due_at, created_at, decided_at
        from human_task where id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(tx.executor())
    .await?
    .ok_or_else(|| crate::Error::not_found("human_task", id))
}

/// 收件匣查詢
///
/// 同時比對使用者與其角色：待辦可能指派給特定人，
/// 也可能指派給角色（例如「財務主管」）由該角色的任一人處理。
pub async fn inbox(
    tx: &mut TenantTx<'_>,
    user_id: Uuid,
    roles: &[String],
    status: &str,
    limit: i64,
) -> Result<Vec<HumanTask>> {
    let rows = sqlx::query_as::<_, HumanTask>(
        r#"
        select id, instance_id, node_id, node_label, assignee_user_id,
               assignee_role, participant_kind, form_key, status, decision,
               comment, due_at, created_at, decided_at
        from human_task
        where status = $3
          and (assignee_user_id = $1 or assignee_role = any($2))
        order by created_at desc
        limit $4
        "#,
    )
    .bind(user_id)
    .bind(roles)
    .bind(status)
    .bind(limit.clamp(1, 200))
    .fetch_all(tx.executor())
    .await?;

    Ok(rows)
}

/// 記錄決策
///
/// 只允許從 PENDING 轉出，回傳是否真的更新了。
/// 兩人同時點同一張待辦時，第二個人會得到 false 而非覆蓋第一個人的決定。
pub async fn decide(
    tx: &mut TenantTx<'_>,
    id: Uuid,
    decision: &str,
    comment: &str,
    decided_by: Uuid,
) -> Result<bool> {
    let affected = sqlx::query(
        r#"
        update human_task
        set status = case when $2 = 'APPROVE' then 'APPROVED' else 'REJECTED' end,
            decision = $2,
            comment = $3,
            decided_by = $4,
            decided_at = now()
        where id = $1 and status = 'PENDING'
        "#,
    )
    .bind(id)
    .bind(decision)
    .bind(comment)
    .bind(decided_by)
    .execute(tx.executor())
    .await?
    .rows_affected();

    Ok(affected > 0)
}

/// 批次取消
///
/// 只取消 PENDING 的。已決策的不動——那是使用者真實做過的事，
/// 覆蓋掉會讓稽核紀錄失真。
pub async fn cancel_many(tx: &mut TenantTx<'_>, ids: &[Uuid], reason: &str) -> Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }

    let affected = sqlx::query(
        r#"
        update human_task
        set status = 'CANCELLED', comment = coalesce(nullif($2, ''), comment)
        where id = any($1) and status = 'PENDING'
        "#,
    )
    .bind(ids)
    .bind(reason)
    .execute(tx.executor())
    .await?
    .rows_affected();

    Ok(affected)
}

/// 取消某流程所有未完成的待辦
///
/// 流程被強制終止時 Interpreter 不會執行清理，
/// 這是收尾的最後一道保險。
pub async fn cancel_by_instance(
    tx: &mut TenantTx<'_>,
    instance_id: Uuid,
    reason: &str,
) -> Result<u64> {
    let affected = sqlx::query(
        r#"
        update human_task
        set status = 'CANCELLED', comment = coalesce(nullif($2, ''), comment)
        where instance_id = $1 and status = 'PENDING'
        "#,
    )
    .bind(instance_id)
    .bind(reason)
    .execute(tx.executor())
    .await?
    .rows_affected();

    Ok(affected)
}

/// 列出流程的所有待辦
///
/// 監控頁要回答「卡在誰身上」，那是待辦而非流程狀態才知道的事。
pub async fn list_by_instance(
    tx: &mut TenantTx<'_>,
    instance_id: Uuid,
) -> Result<Vec<HumanTask>> {
    let rows = sqlx::query_as::<_, HumanTask>(
        r#"
        select id, instance_id, node_id, node_label, assignee_user_id,
               assignee_role, participant_kind, form_key, status, decision,
               comment, due_at, created_at, decided_at
        from human_task
        where instance_id = $1
        order by created_at
        "#,
    )
    .bind(instance_id)
    .fetch_all(tx.executor())
    .await?;

    Ok(rows)
}

/// 改派待辦給別人
///
/// 只動 PENDING 的。已決策的不能改派——那會讓稽核紀錄裡
/// 「誰簽的」與「指派給誰」對不起來。
///
/// 改派同時清掉 `assignee_role`：原本指派給角色的待辦改派給特定人
/// 之後，若 role 還留著，該角色的其他人在收件匣仍看得到它
/// （`inbox` 查詢是 `assignee_user_id = $1 or assignee_role = any($2)`）。
/// 那等於沒有改派。
pub async fn reassign(
    tx: &mut TenantTx<'_>,
    id: Uuid,
    to_user_id: Uuid,
) -> Result<bool> {
    let affected = sqlx::query(
        r#"
        update human_task
        set assignee_user_id = $2,
            assignee_role = null
        where id = $1 and status = 'PENDING'
        "#,
    )
    .bind(id)
    .bind(to_user_id)
    .execute(tx.executor())
    .await?
    .rows_affected();

    Ok(affected > 0)
}

/// 這個人是否在這筆流程裡有（或曾有）待辦
///
/// 流程監控的可見範圍用它判定：那張單本來就在你的收件匣裡，
/// 讓你看得到它的存在不算新增洩漏。
pub async fn is_participant(
    tx: &mut TenantTx<'_>,
    instance_id: Uuid,
    user_id: Uuid,
) -> Result<bool> {
    let found: Option<i32> = sqlx::query_scalar(
        "select 1 from human_task
         where instance_id = $1 and assignee_user_id = $2 limit 1",
    )
    .bind(instance_id)
    .bind(user_id)
    .fetch_optional(tx.executor())
    .await?;

    Ok(found.is_some())
}
