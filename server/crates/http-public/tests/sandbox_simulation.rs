//! 模擬簽核的授權邊界（S2）
//!
//! **這是整批工作最需要謹慎的地方。**
//!
//! `tasks.rs` 的 assignee 檢查會擋下模擬簽核（測試者不是 assignee），
//! 必須開洞——而開洞的地方正是安全邊界。
//!
//! 條件只能是「沙箱租戶 且 操作者是該沙箱的建立者本人」，
//! 不能做成「有某個角色就能扮演」。正式租戶誤給該角色
//! 等同開放冒名簽核。
//!
//! **沙箱身分不是一種權限，是一種環境狀態。**
//!
//! 本檔的第一條測試是永久回歸測試，防止日後有人為了方便把條件放寬。

use persistence::Db;
use uuid::Uuid;

fn database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://workflow_app:app_dev_only@localhost:5432/workflow".into())
}

fn admin_url() -> String {
    std::env::var("TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost:5432/workflow".into())
}

/// 建立正式租戶與一個使用者
async fn new_parent_tenant() -> (Db, Uuid, Uuid) {
    let db = Db::connect(&database_url()).await.expect("連線失敗");
    let admin = sqlx::postgres::PgPool::connect(&admin_url())
        .await
        .expect("管理連線失敗");

    let tenant_id = Uuid::new_v4();
    sqlx::query("insert into tenant (id, code, name) values ($1, $2, $3)")
        .bind(tenant_id)
        .bind(format!("sbx-{}", &tenant_id.to_string()[..8]))
        .bind("模擬測試租戶")
        .execute(&admin)
        .await
        .expect("建立租戶失敗");

    let mut tx = db.tenant_tx(tenant_id).await.unwrap();

    sqlx::query("insert into role (tenant_id, code, name) values ($1, 'approver', '簽核人')")
        .bind(tenant_id)
        .execute(tx.executor())
        .await
        .unwrap();

    let user_id: Uuid = sqlx::query_scalar(
        "insert into app_user (tenant_id, email, name, password_hash)
         values ($1, 'tester@sbx.local', '測試者', 'x') returning id",
    )
    .bind(tenant_id)
    .fetch_one(tx.executor())
    .await
    .unwrap();

    tx.commit().await.unwrap();
    (db, tenant_id, user_id)
}

// ── 環境狀態，不是權限 ────────────────────────────────

#[tokio::test]
async fn production_tenant_is_never_sandbox() {
    // 永久回歸測試。
    //
    // 模擬簽核的授權開洞綁在 is_sandbox 上。正式租戶的這個值
    // 必須永遠是 false——否則等同開放冒名簽核。
    //
    // 日後若有人為了方便把開洞條件改成「有某角色就能扮演」，
    // 這條測試不會擋住他，但 §授權 那幾條會。
    let (db, tenant_id, _) = new_parent_tenant().await;

    let flags = persistence::sandbox::flags_of(&db, tenant_id)
        .await
        .expect("查詢失敗");

    assert!(!flags.is_sandbox, "正式租戶的 is_sandbox 必須是 false");
}

#[tokio::test]
async fn production_tenant_has_no_active_session() {
    // 正式租戶查不到 sandbox_session，所以開洞的第二個條件
    // （操作者是建立者本人）根本無從成立
    let (db, tenant_id, _) = new_parent_tenant().await;

    let session = persistence::sandbox::active_session_of(&db, tenant_id)
        .await
        .expect("查詢失敗");

    assert!(session.is_none(), "正式租戶不該有沙箱 session");
}

// ── 沙箱建立 ──────────────────────────────────────────

#[tokio::test]
async fn sandbox_is_flagged_and_has_session() {
    let (db, parent, tester) = new_parent_tenant().await;

    let (sandbox_id, session) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    let flags = persistence::sandbox::flags_of(&db, sandbox_id)
        .await
        .expect("查詢失敗");
    assert!(flags.is_sandbox, "沙箱租戶的 is_sandbox 應為 true");

    assert_eq!(session.created_by, tester, "session 要記得建立者是誰");
    assert_eq!(session.status, "ACTIVE");
    assert_eq!(session.sandbox_tenant_id, Some(sandbox_id));

    persistence::sandbox::retire(&db, sandbox_id).await.ok();
}

#[tokio::test]
async fn sandbox_copies_users_so_resolver_can_work() {
    // 使用者必須複製——resolver（role、manager_of、department_manager）
    // 全部查 app_user。不複製的話「真的解析簽核人」根本做不到，
    // 而那正是模擬簽核最大的價值。
    let (db, parent, tester) = new_parent_tenant().await;

    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    let mut tx = db.tenant_tx(sandbox_id).await.unwrap();
    let users: Vec<(Uuid, String)> = sqlx::query_as("select id, email from app_user")
        .fetch_all(tx.executor())
        .await
        .unwrap();

    assert!(!users.is_empty(), "沙箱要有使用者，否則 resolver 找不到人");

    // id 是重新產生的——app_user.id 是全域主鍵，沿用來源的值會撞鍵。
    // 要對回真實的人靠 email（(tenant_id, email) 唯一）。
    assert!(
        users.iter().any(|(_, email)| email == "tester@sbx.local"),
        "沙箱的使用者要能以 email 對回來源的人"
    );
    assert!(
        users.iter().all(|(id, _)| *id != tester),
        "沙箱的 id 必須重新產生，不可與來源相同"
    );

    // 收掉自己建的沙箱——每個沙箱都複製整份定義，不收會累積
    persistence::sandbox::retire(&db, sandbox_id).await.ok();
}

#[tokio::test]
async fn sandbox_copies_roles() {
    // role resolver 要用
    let (db, parent, tester) = new_parent_tenant().await;

    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    let mut tx = db.tenant_tx(sandbox_id).await.unwrap();
    let roles: Vec<(String,)> = sqlx::query_as("select code from role")
        .fetch_all(tx.executor())
        .await
        .unwrap();

    assert!(
        roles.iter().any(|(c,)| c == "approver"),
        "沙箱要有來源的角色"
    );

    // 收掉自己建的沙箱——每個沙箱都複製整份定義，不收會累積
    persistence::sandbox::retire(&db, sandbox_id).await.ok();
}

// ── 隔離（雙向）────────────────────────────────────────

#[tokio::test]
async fn sandbox_data_does_not_leak_into_parent() {
    // 在沙箱建資料，正式租戶看不到
    let (db, parent, tester) = new_parent_tenant().await;
    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    let mut tx = db.tenant_tx(sandbox_id).await.unwrap();
    sqlx::query("insert into role (tenant_id, code, name) values ($1, 'sandbox_only', '沙箱限定')")
        .bind(sandbox_id)
        .execute(tx.executor())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let mut tx = db.tenant_tx(parent).await.unwrap();
    let leaked: Vec<(String,)> = sqlx::query_as("select code from role where code = 'sandbox_only'")
        .fetch_all(tx.executor())
        .await
        .unwrap();

    assert!(leaked.is_empty(), "沙箱的資料不該出現在正式租戶");

    // 收掉自己建的沙箱——每個沙箱都複製整份定義，不收會累積
    persistence::sandbox::retire(&db, sandbox_id).await.ok();
}

#[tokio::test]
async fn parent_data_does_not_leak_into_sandbox_after_creation() {
    // 雙向都要測——單向會漏掉「沙箱看得到正式的新資料」。
    //
    // 建立時複製是一次性的快照；建立之後正式租戶的異動
    // 不該流進沙箱，否則沙箱裡看到的就不是當初那份設定。
    let (db, parent, tester) = new_parent_tenant().await;
    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    let mut tx = db.tenant_tx(parent).await.unwrap();
    sqlx::query("insert into role (tenant_id, code, name) values ($1, 'added_later', '後來加的')")
        .bind(parent)
        .execute(tx.executor())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let mut tx = db.tenant_tx(sandbox_id).await.unwrap();
    let leaked: Vec<(String,)> = sqlx::query_as("select code from role where code = 'added_later'")
        .fetch_all(tx.executor())
        .await
        .unwrap();

    assert!(leaked.is_empty(), "建立後正式租戶的異動不該流進沙箱");

    // 收掉自己建的沙箱——每個沙箱都複製整份定義，不收會累積
    persistence::sandbox::retire(&db, sandbox_id).await.ok();
}

// ── 回收 ──────────────────────────────────────────────

#[tokio::test]
async fn retire_keeps_session_record() {
    // 租戶是資源，session 是行為紀錄。
    // 退役之後「某人在某時建過沙箱」這件事要留著。
    let (db, parent, tester) = new_parent_tenant().await;
    let (sandbox_id, session) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    persistence::sandbox::retire(&db, sandbox_id)
        .await
        .expect("退役失敗");

    let mut tx = db.tenant_tx(parent).await.unwrap();
    let rows: Vec<(String,)> = sqlx::query_as("select status from sandbox_session where id = $1")
        .bind(session.id)
        .fetch_all(tx.executor())
        .await
        .unwrap();

    assert_eq!(rows.len(), 1, "session 紀錄要保留");
    assert_eq!(rows[0].0, "DISCARDED");
}

#[tokio::test]
async fn retired_sandbox_has_no_active_session() {
    // 退役後授權開洞的第二個條件（有 ACTIVE session）不再成立
    let (db, parent, tester) = new_parent_tenant().await;
    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    persistence::sandbox::retire(&db, sandbox_id)
        .await
        .expect("退役失敗");

    let session = persistence::sandbox::active_session_of(&db, sandbox_id)
        .await
        .expect("查詢失敗");

    assert!(session.is_none(), "已退役的沙箱不該有 ACTIVE session");
}

#[tokio::test]
async fn retired_sandbox_is_marked_expired() {
    // 退役是軟的：租戶還在但已過期，實際清除交給維運身分的回收程序。
    // 在請求路徑上真的刪要讓應用程式角色拿到刪稽核的權限，
    // 那等於讓任何一支 API 都能抹掉稽核軌跡，不划算。
    let (db, parent, tester) = new_parent_tenant().await;
    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    assert!(
        !persistence::sandbox::is_retired(&db, sandbox_id).await.unwrap(),
        "剛建立的沙箱不該是已退役"
    );

    persistence::sandbox::retire(&db, sandbox_id)
        .await
        .expect("退役失敗");

    assert!(
        persistence::sandbox::is_retired(&db, sandbox_id).await.unwrap(),
        "退役後應標記為已過期"
    );
}

// ── 授權開洞的邊界（永久回歸）────────────────────────
//
// 這幾條驗證的是 tasks.rs 的 simulation_allowed()。
// 它們存在的目的是防止日後有人為了方便把條件放寬成
// 「有某個角色就能扮演」——那會變成正式環境的提權路徑。

/// 重現 `simulation_allowed()` 的條件
///
/// 刻意複寫一份而不是呼叫 http-public 的私有函式：
/// 這幾條測的是「條件本身」，複寫一份能讓條件被偷改時
/// 測試與實作分歧而爆出來。
async fn simulation_allowed(db: &Db, tenant_id: Uuid, actor: Uuid) -> bool {
    let flags = persistence::sandbox::flags_of(db, tenant_id).await.unwrap();
    if !flags.is_sandbox {
        return false;
    }
    // 比對沙箱內的 id——登入沙箱後 JWT 帶的是沙箱租戶的 user id
    persistence::sandbox::active_session_of(db, tenant_id)
        .await
        .unwrap()
        .is_some_and(|s| s.created_by_in_sandbox == Some(actor))
}

/// 建立者在沙箱租戶內的 user id
async fn creator_in_sandbox(db: &Db, sandbox_id: Uuid) -> Uuid {
    persistence::sandbox::active_session_of(db, sandbox_id)
        .await
        .unwrap()
        .and_then(|s| s.created_by_in_sandbox)
        .expect("session 應記錄建立者在沙箱內的 id")
}

#[tokio::test]
async fn production_tenant_never_allows_simulation() {
    // **最重要的一條。** 正式租戶不論操作者是誰、有什麼角色，
    // 都不可以走模擬路徑。
    //
    // 這條紅了代表正式環境開放了冒名簽核。
    let (db, parent, tester) = new_parent_tenant().await;

    assert!(
        !simulation_allowed(&db, parent, tester).await,
        "正式租戶必定不允許模擬簽核"
    );

    // 換任何其他人也一樣
    let stranger = Uuid::new_v4();
    assert!(
        !simulation_allowed(&db, parent, stranger).await,
        "正式租戶對任何操作者都不允許模擬簽核"
    );
}

#[tokio::test]
async fn sandbox_creator_is_allowed() {
    let (db, parent, tester) = new_parent_tenant().await;
    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    let inside = creator_in_sandbox(&db, sandbox_id).await;
    assert_ne!(
        inside, tester,
        "沙箱內的 id 與正式租戶不同——app_user.id 是全域主鍵"
    );
    assert!(
        simulation_allowed(&db, sandbox_id, inside).await,
        "沙箱的建立者本人可以模擬簽核"
    );

    // 用正式租戶的 id 比對必定不成立，這正是先前的缺陷
    assert!(
        !simulation_allowed(&db, sandbox_id, tester).await,
        "用正式租戶的 id 比對不該成立"
    );

    // 收掉自己建的沙箱——每個沙箱都複製整份定義，不收會累積
    persistence::sandbox::retire(&db, sandbox_id).await.ok();
}

#[tokio::test]
async fn sandbox_non_creator_is_blocked() {
    // 第二個條件：必須是建立者「本人」。
    // 不是「任何能進這個沙箱的人」，也不是「有某角色的人」。
    let (db, parent, tester) = new_parent_tenant().await;
    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    let someone_else = Uuid::new_v4();
    assert!(
        !simulation_allowed(&db, sandbox_id, someone_else).await,
        "沙箱的非建立者不可模擬簽核"
    );

    // 收掉自己建的沙箱——每個沙箱都複製整份定義，不收會累積
    persistence::sandbox::retire(&db, sandbox_id).await.ok();
}

#[tokio::test]
async fn retired_sandbox_blocks_simulation() {
    // 退役後條件自動不成立，不需要額外的檢查點
    let (db, parent, tester) = new_parent_tenant().await;
    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    let inside = creator_in_sandbox(&db, sandbox_id).await;
    assert!(simulation_allowed(&db, sandbox_id, inside).await);

    persistence::sandbox::retire(&db, sandbox_id)
        .await
        .expect("退役失敗");

    assert!(
        !simulation_allowed(&db, sandbox_id, inside).await,
        "退役後不可再模擬簽核"
    );
}

// ═══════════════════════════════════════════════════════════
// 權限過濾畫面：角色來源
//
// 使用者原話的第三件事：
//   「系統依據權限設定顯示每個簽核人員的畫面」
//
// `resolve_form` 依角色判定可見性（domain 已有
// `role_not_in_readable_list_gets_hidden` 蓋住這件事）。
// 這裡要測的是本端點獨有的風險：**角色從哪裡來**。
//
// 傳測試者的角色，畫面會是「測試者眼中的表單」——
// 流程照樣會動、測試照樣全綠，但模擬出來的畫面
// 與那個人真正簽核時看到的不同，整個功能就失去意義。
// ═══════════════════════════════════════════════════════════

/// 重現 `task_form()` 取角色的查詢
///
/// 同樣刻意複寫：查詢若被改成以 actor 為準，這裡會分歧而爆出來。
async fn roles_of(db: &Db, tenant_id: Uuid, user_id: Uuid) -> Vec<String> {
    let mut tx = db.tenant_tx(tenant_id).await.unwrap();
    let roles = sqlx::query_scalar(
        "select r.code from user_role ur join role r on r.id = ur.role_id
         where ur.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(tx.executor())
    .await
    .unwrap();
    tx.commit().await.ok();
    roles
}

#[tokio::test]
async fn acting_as_roles_come_from_the_impersonated_person() {
    // 沙箱裡兩個角色不同的人，查出來的角色必須不同。
    // 相同就代表查詢沒有以被扮演者為準。
    let (db, parent, tester) = new_parent_tenant().await;
    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    let creator = creator_in_sandbox(&db, sandbox_id).await;

    // 造一個角色明顯不同的人
    let other = Uuid::new_v4();
    let mut tx = db.tenant_tx(sandbox_id).await.unwrap();
    sqlx::query(
        "insert into app_user (id, tenant_id, email, name, password_hash, status)
         values ($1, $2, 'other@sbx.local', '另一個人', 'x', 'ACTIVE')",
    )
    .bind(other)
    .bind(sandbox_id)
    .execute(tx.executor())
    .await
    .unwrap();

    let role_id: Uuid = sqlx::query_scalar(
        "insert into role (id, tenant_id, code, name)
         values ($1, $2, 'finance_only', '只有財務') returning id",
    )
    .bind(Uuid::new_v4())
    .bind(sandbox_id)
    .fetch_one(tx.executor())
    .await
    .unwrap();

    // user_role 自己帶 tenant_id，RLS 靠它——漏掉會被 policy 擋下
    sqlx::query("insert into user_role (user_id, role_id, tenant_id) values ($1, $2, $3)")
        .bind(other)
        .bind(role_id)
        .bind(sandbox_id)
        .execute(tx.executor())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let creator_roles = roles_of(&db, sandbox_id, creator).await;
    let other_roles = roles_of(&db, sandbox_id, other).await;

    assert_eq!(
        other_roles,
        vec!["finance_only".to_string()],
        "被扮演者的角色要是他自己的"
    );
    assert!(
        !creator_roles.contains(&"finance_only".to_string()),
        "測試者不該拿到被扮演者的角色"
    );
    assert_ne!(
        creator_roles, other_roles,
        "兩個人的角色必須不同，否則證明不了畫面是依被扮演者重算的"
    );

    persistence::sandbox::retire(&db, sandbox_id).await.ok();
}

#[tokio::test]
async fn user_with_no_roles_yields_empty_not_error() {
    // 沒掛角色是合法狀態（例如剛建好的帳號）。
    // 這時 resolve_form 會把限制角色的欄位判成 HIDDEN，
    // 畫面應該照實呈現，而不是查詢爆掉或退回「不限制」。
    let (db, parent, tester) = new_parent_tenant().await;
    let (sandbox_id, _) = persistence::sandbox::create(&db, parent, tester)
        .await
        .expect("建立沙箱失敗");

    let naked = Uuid::new_v4();
    let mut tx = db.tenant_tx(sandbox_id).await.unwrap();
    sqlx::query(
        "insert into app_user (id, tenant_id, email, name, password_hash, status)
         values ($1, $2, 'naked@sbx.local', '沒有角色的人', 'x', 'ACTIVE')",
    )
    .bind(naked)
    .bind(sandbox_id)
    .execute(tx.executor())
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert!(
        roles_of(&db, sandbox_id, naked).await.is_empty(),
        "沒有角色要回空陣列，不是錯誤"
    );

    persistence::sandbox::retire(&db, sandbox_id).await.ok();
}
