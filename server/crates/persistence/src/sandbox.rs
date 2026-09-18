//! 開發模式（沙箱）租戶與 session
//!
//! 沙箱不是另一個環境，是一個附身模式：測試者始終是同一個真實使用者，
//! 只是在該次簽核操作上標註自己扮演誰。
//!
//! 隔離靠既有的 RLS + `tenant_tx()`，不需要在查詢加 `is_sandbox` 過濾。
//! 建一個沙箱租戶，隔離就自動成立。

use crate::{Db, Error, Result, TenantTx};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

/// 沙箱預設存活時間
///
/// 已有 574 個測試租戶的前例，不設期限會無限膨脹。
/// 七天足夠走完一輪設計與驗證，過期由回收程序清掉。
const DEFAULT_TTL_DAYS: i64 = 7;

#[derive(Debug, Serialize, FromRow)]
pub struct SandboxSession {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub sandbox_tenant_id: Option<Uuid>,
    pub change_request_id: Option<Uuid>,
    pub created_by: Uuid,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// 租戶的沙箱屬性
///
/// 由 `tenant_tx()` 帶回，讓呼叫端不必各自查 `tenant` 表。
/// 集中在單一進入點也讓「忘記判斷 is_sandbox」更難發生。
#[derive(Debug, Clone, Copy, Default)]
pub struct TenantFlags {
    pub is_sandbox: bool,
}

/// 讀取租戶的沙箱屬性
///
/// 用 admin 連線讀而非 tenant_tx：這是 tenant_tx 自己要用的，
/// 會造成循環。且 tenant 表沒有 RLS policy（它是租戶清單本身）。
pub async fn flags_of(db: &Db, tenant_id: Uuid) -> Result<TenantFlags> {
    let pool = &db.pool;
    let row: Option<(bool,)> = sqlx::query_as("select is_sandbox from tenant where id = $1")
        .bind(tenant_id)
        .fetch_optional(pool)
        .await
        .map_err(Error::from_db)?;

    Ok(TenantFlags {
        is_sandbox: row.map(|r| r.0).unwrap_or(false),
    })
}

/// 這個沙箱租戶的 ACTIVE session
///
/// 模擬簽核的授權開洞要用它判斷「操作者是不是沙箱的建立者本人」。
/// 查不到就不是合法的沙箱環境，授權不開洞。
pub async fn active_session_of(
    db: &Db,
    sandbox_tenant_id: Uuid,
) -> Result<Option<SandboxSession>> {
    let pool = &db.pool;
    sqlx::query_as::<_, SandboxSession>(
        "select * from sandbox_session
         where sandbox_tenant_id = $1 and status = 'ACTIVE'
         order by created_at desc limit 1",
    )
    .bind(sandbox_tenant_id)
    .fetch_optional(pool)
    .await
    .map_err(Error::from_db)
}

/// 建立沙箱租戶與 session
///
/// 用 admin 連線：要寫 `tenant` 表並跨兩個租戶複製資料，
/// 不是單一租戶內的操作，`tenant_tx` 的語意不適用。
///
/// 複製哪些主檔：`role`、`department`、`app_user`。
///
/// **使用者必須複製**——雖然測試者不會登入成他們，但 resolver
/// （`role`、`manager_of`、`department_manager`）全部查 `app_user`。
/// 不複製的話「真的解析簽核人」這件事根本做不到，
/// 而那正是模擬簽核最大的價值。
pub async fn create(
    db: &Db,
    parent_tenant_id: Uuid,
    created_by: Uuid,
) -> Result<(Uuid, SandboxSession)> {
    let mut tx = db.pool.begin().await.map_err(Error::from_db)?;

    let sandbox_id = Uuid::new_v4();
    let expires_at = Utc::now() + Duration::days(DEFAULT_TTL_DAYS);

    // 租戶代碼帶來源與亂數，方便從清單一眼看出是誰的沙箱
    let parent_code: String = sqlx::query_scalar("select code from tenant where id = $1")
        .bind(parent_tenant_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(Error::from_db)?;

    let suffix = &sandbox_id.to_string()[..6];

    sqlx::query(
        "insert into tenant (id, code, name, timezone, currency,
                             parent_tenant_id, is_sandbox, expires_at)
         select $1, $2, name || '（開發模式）', timezone, currency, id, true, $3
         from tenant where id = $4",
    )
    .bind(sandbox_id)
    .bind(format!("{parent_code}__dev_{suffix}"))
    .bind(expires_at)
    .bind(parent_tenant_id)
    .execute(&mut *tx)
    .await
    .map_err(Error::from_db)?;


    let session = sqlx::query_as::<_, SandboxSession>(
        "insert into sandbox_session
            (tenant_id, sandbox_tenant_id, created_by, expires_at)
         values ($1, $2, $3, $4)
         returning *",
    )
    .bind(parent_tenant_id)
    .bind(sandbox_id)
    .bind(created_by)
    .bind(expires_at)
    .fetch_one(&mut *tx)
    .await
    .map_err(Error::from_db)?;

    tx.commit().await.map_err(Error::from_db)?;

    // 主檔複製走 tenant_tx 而非原始連線。
    //
    // 直接用連線池複製會**靜默失敗**：應用程式角色是非 superuser，
    // 沒設 app.tenant_id 時 RLS 讓來源的 SELECT 回 0 筆，
    // INSERT ... SELECT 於是插入 0 列，沒有任何錯誤。
    // 沙箱看起來建好了，但裡面沒有人——直到模擬簽核解析不到人才會發現。
    //
    // 分兩段讀寫的代價是不在同一個交易裡。沙箱建到一半失敗會留下
    // 空的沙箱租戶，那是可回收的垃圾；反過來若為了原子性而繞過 RLS，
    // 換來的是跨租戶寫入的能力。寧可留垃圾。
    copy_master_data(db, parent_tenant_id, sandbox_id).await?;

    Ok((sandbox_id, session))
}

/// 複製 resolver 需要的主檔
///
/// 順序有相依：department 要先於 app_user（外鍵），
/// app_user 的 manager_id 與 department 的 manager_user_id
/// 都要等兩邊都在了才補。
///
/// id 一律沿用來源的值。這讓「沙箱裡的張文華」與「正式的張文華」
/// 是同一個 uuid，模擬簽核的 acting_as 才指得回真實的人。
async fn copy_master_data(db: &Db, from: Uuid, to: Uuid) -> Result<()> {
    use std::collections::HashMap;

    // ── 讀：以來源租戶的身分 ──────────────────────────
    let mut src = db.tenant_tx(from).await?;

    let roles: Vec<(String, String, bool)> =
        sqlx::query_as("select code, name, is_system from role")
            .fetch_all(src.executor())
            .await
            .map_err(Error::from_db)?;

    let departments: Vec<(Uuid, String, String, Option<Uuid>, Option<Uuid>)> =
        sqlx::query_as("select id, code, name, parent_id, manager_user_id from department")
            .fetch_all(src.executor())
            .await
            .map_err(Error::from_db)?;

    let users: Vec<(Uuid, String, String, String, Option<Uuid>, Option<Uuid>, String)> =
        sqlx::query_as(
            "select id, email, name, password_hash, department_id, manager_id, status
             from app_user",
        )
        .fetch_all(src.executor())
        .await
        .map_err(Error::from_db)?;

    let user_roles: Vec<(Uuid, String)> = sqlx::query_as(
        "select ur.user_id, r.code from user_role ur join role r on r.id = ur.role_id",
    )
    .fetch_all(src.executor())
    .await
    .map_err(Error::from_db)?;

    src.commit().await?;

    // 空的來源代表 RLS 擋住了讀取，或來源根本沒有主檔。
    // 兩種都會讓模擬簽核解析不到人，與其安靜地建出空沙箱，
    // 不如當場失敗——靜默的錯誤比會爆炸的錯誤危險。
    if users.is_empty() {
        return Err(Error::Conflict(
            "來源租戶沒有可複製的使用者，沙箱的 resolver 會解析不到任何人".into(),
        ));
    }

    // 沙箱的 id 必須重新產生：app_user.id 與 department.id 都是全域主鍵，
    // 沿用來源的值會撞鍵。
    //
    // 代價是「沙箱的張文華」與「正式的張文華」不是同一個 uuid，
    // 所以 acting_as 記的是沙箱裡的 id。要對回真實的人靠 email——
    // 那是 (tenant_id, email) 唯一的，對得起來。
    let dept_map: HashMap<Uuid, Uuid> =
        departments.iter().map(|(id, ..)| (*id, Uuid::new_v4())).collect();
    let user_map: HashMap<Uuid, Uuid> =
        users.iter().map(|(id, ..)| (*id, Uuid::new_v4())).collect();

    // ── 寫：以沙箱租戶的身分 ──────────────────────────
    let mut dst = db.tenant_tx(to).await?;

    for (code, name, is_system) in &roles {
        sqlx::query("insert into role (tenant_id, code, name, is_system) values ($1,$2,$3,$4)")
            .bind(to)
            .bind(code)
            .bind(name)
            .bind(is_system)
            .execute(dst.executor())
            .await
            .map_err(Error::from_db)?;
    }

    // 先不帶 parent_id 與 manager_user_id，避免自我參照的順序問題
    for (id, code, name, _, _) in &departments {
        sqlx::query("insert into department (id, tenant_id, code, name) values ($1,$2,$3,$4)")
            .bind(dept_map[id])
            .bind(to)
            .bind(code)
            .bind(name)
            .execute(dst.executor())
            .await
            .map_err(Error::from_db)?;
    }

    for (id, email, name, hash, dept, _, status) in &users {
        sqlx::query(
            "insert into app_user (id, tenant_id, email, name, password_hash,
                                   department_id, status)
             values ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(user_map[id])
        .bind(to)
        .bind(email)
        .bind(name)
        .bind(hash)
        .bind(dept.and_then(|d| dept_map.get(&d).copied()))
        .bind(status)
        .execute(dst.executor())
        .await
        .map_err(Error::from_db)?;
    }

    // 兩邊都在了，補回自我參照（一律經過對照表換成沙箱的 id）
    for (id, _, _, parent, manager) in &departments {
        sqlx::query("update department set parent_id = $1, manager_user_id = $2 where id = $3")
            .bind(parent.and_then(|p| dept_map.get(&p).copied()))
            .bind(manager.and_then(|m| user_map.get(&m).copied()))
            .bind(dept_map[id])
            .execute(dst.executor())
            .await
            .map_err(Error::from_db)?;
    }

    for (id, _, _, _, _, manager, _) in &users {
        sqlx::query("update app_user set manager_id = $1 where id = $2")
            .bind(manager.and_then(|m| user_map.get(&m).copied()))
            .bind(user_map[id])
            .execute(dst.executor())
            .await
            .map_err(Error::from_db)?;
    }

    for (user_id, role_code) in &user_roles {
        let Some(mapped) = user_map.get(user_id) else {
            continue;
        };
        sqlx::query(
            "insert into user_role (tenant_id, user_id, role_id)
             select $1, $2, id from role where tenant_id = $1 and code = $3",
        )
        .bind(to)
        .bind(mapped)
        .bind(role_code)
        .execute(dst.executor())
        .await
        .map_err(Error::from_db)?;
    }

    dst.commit().await?;
    Ok(())
}

/// 退役沙箱
///
/// **刻意是軟退役，不真的刪資料。**
///
/// 應用程式角色沒有 `audit_event` 的 DELETE 權限——那是 0003 刻意的
/// 設計（稽核 append-only），而 `tenant` 的外鍵是 RESTRICT，
/// 有稽核就刪不掉租戶。
///
/// 要在請求路徑上真的刪，就得讓應用程式角色拿到刪稽核的權限。
/// 為了回收垃圾而開這個權限不划算——那等於讓任何一支 API
/// 都有抹掉稽核軌跡的能力。
///
/// 因此這裡只做兩件事：把 session 標成 DISCARDED、把租戶設為已過期。
/// 實際清除交給以維運身分執行的回收程序（`expires_at` 就是給它看的）。
pub async fn retire(db: &Db, sandbox_tenant_id: Uuid) -> Result<()> {
    let mut tx = db.pool.begin().await.map_err(Error::from_db)?;

    sqlx::query("update sandbox_session set status = 'DISCARDED' where sandbox_tenant_id = $1")
        .bind(sandbox_tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(Error::from_db)?;

    // 設為已過期。回收程序據此清除，前端據此不再顯示。
    sqlx::query("update tenant set expires_at = now() where id = $1 and is_sandbox")
        .bind(sandbox_tenant_id)
        .execute(&mut *tx)
        .await
        .map_err(Error::from_db)?;

    tx.commit().await.map_err(Error::from_db)?;
    Ok(())
}

/// 沙箱是否已退役（過期或無 ACTIVE session）
///
/// 授權開洞要用：退役後條件不再成立，模擬簽核立刻失效。
pub async fn is_retired(db: &Db, sandbox_tenant_id: Uuid) -> Result<bool> {
    let row: Option<(Option<DateTime<Utc>>,)> =
        sqlx::query_as("select expires_at from tenant where id = $1 and is_sandbox")
            .bind(sandbox_tenant_id)
            .fetch_optional(&db.pool)
            .await
            .map_err(Error::from_db)?;

    match row {
        None => Ok(true),
        Some((Some(expires),)) => Ok(expires <= Utc::now()),
        Some((None,)) => Ok(false),
    }
}

/// 列出某租戶目前有效的沙箱
pub async fn list_active(tx: &mut TenantTx<'_>, tenant_id: Uuid) -> Result<Vec<SandboxSession>> {
    sqlx::query_as::<_, SandboxSession>(
        "select * from sandbox_session
         where tenant_id = $1 and status = 'ACTIVE'
         order by created_at desc",
    )
    .bind(tenant_id)
    .fetch_all(tx.executor())
    .await
    .map_err(Error::from_db)
}
