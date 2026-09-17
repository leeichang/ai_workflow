//! 簽核人解析
//!
//! DSL 的 resolver 只描述「怎麼找」，實際的組織結構在資料庫。
//! 這裡把 resolver 的意圖翻譯成查詢。
//!
//! 全部查詢都在 TenantTx 內，因此自動受 RLS 保護——
//! 不可能解析到其他租戶的使用者。

use crate::{Result, TenantTx};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ResolvedUser {
    pub id: Uuid,
    pub name: String,
    pub role: Option<String>,
}

pub async fn by_id(tx: &mut TenantTx<'_>, id: Uuid) -> Result<Vec<ResolvedUser>> {
    let rows = sqlx::query_as::<_, ResolvedUser>(
        "select id, name, null::text as role from app_user where id = $1",
    )
    .bind(id)
    .fetch_all(tx.executor())
    .await?;

    Ok(rows)
}

/// 依角色代碼找人
///
/// 回傳該角色的所有人。並簽或搶單由 join policy 決定，
/// 這裡不做取捨。
pub async fn by_role(tx: &mut TenantTx<'_>, role_code: &str) -> Result<Vec<ResolvedUser>> {
    let rows = sqlx::query_as::<_, ResolvedUser>(
        r#"
        select u.id, u.name, r.code as role
        from app_user u
        join user_role ur on ur.user_id = u.id
        join role r on r.id = ur.role_id
        where r.code = $1
        order by u.name
        "#,
    )
    .bind(role_code)
    .fetch_all(tx.executor())
    .await?;

    Ok(rows)
}

/// 找某人的直屬主管
///
/// 沒有主管時回空陣列而非拋錯。Interpreter 會以
/// 「找不到參與者」明確失敗，訊息比「查無資料」有用。
pub async fn manager_of(tx: &mut TenantTx<'_>, user_id: Uuid) -> Result<Vec<ResolvedUser>> {
    let rows = sqlx::query_as::<_, ResolvedUser>(
        r#"
        select m.id, m.name, null::text as role
        from app_user u
        join app_user m on m.id = u.manager_id
        where u.id = $1
        "#,
    )
    .bind(user_id)
    .fetch_all(tx.executor())
    .await?;

    Ok(rows)
}

/// 找某人所屬部門的主管
///
/// 與 manager_of 的差別：後者是個人的直屬上司（可能跨部門），
/// 這個是部門的負責人。組織調整時兩者可能不同。
pub async fn department_manager_of(
    tx: &mut TenantTx<'_>,
    user_id: Uuid,
) -> Result<Vec<ResolvedUser>> {
    let rows = sqlx::query_as::<_, ResolvedUser>(
        r#"
        select m.id, m.name, null::text as role
        from app_user u
        join department d on d.id = u.department_id
        join app_user m on m.id = d.manager_user_id
        where u.id = $1 and m.id <> u.id
        "#,
    )
    .bind(user_id)
    .fetch_all(tx.executor())
    .await?;

    Ok(rows)
}

/// 取出使用者的 email
///
/// 通知要寄到信箱，但流程傳的是 user id——參與者解析回傳的是
/// 身分而非聯絡方式，兩者刻意分開：同一個人的通知管道可能變，
/// 流程定義不該因此要改。
///
/// 停用的帳號不回傳。離職者的信箱通常已停用，寄過去只會退信。
///
/// 找不到的 id 靜默略過而非報錯：外部聯絡人的 id 就是 email 本身，
/// 不在 app_user 裡，由呼叫端處理。
pub async fn emails_of(
    tx: &mut TenantTx<'_>,
    user_ids: &[Uuid],
) -> Result<Vec<String>> {
    if user_ids.is_empty() {
        return Ok(Vec::new());
    }

    let rows: Vec<(String,)> = sqlx::query_as(
        "select email from app_user
         where id = any($1) and status = 'ACTIVE' and email <> ''",
    )
    .bind(user_ids)
    .fetch_all(tx.executor())
    .await?;

    Ok(rows.into_iter().map(|(e,)| e).collect())
}
