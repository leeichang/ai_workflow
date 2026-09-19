//! 表單版本鎖定（P3）
//!
//! 為什麼需要這個功能：
//!   workflow_instance 已經鎖定流程版本，表單沒有。流程跑到一半改表單定義，
//!   進行中的實例會用到新版——欄位被刪掉、必填變選填、權限改了，
//!   都會直接影響還沒簽完的單。
//!
//! 哪一張表單：
//!   由表單自己宣告服務哪個業務物件（form_definition.business_object），
//!   啟動實例時以流程的 business_object 反查。
//!   方向是「表單指定業務物件」，不是「流程指定表單」——
//!   後者會讓同一張表單被多個流程各自宣告一次，遲早不一致。

use persistence::Db;
use serde_json::json;
use uuid::Uuid;

fn database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://workflow_app:app_dev_only@localhost:5432/workflow".into())
}

fn admin_url() -> String {
    std::env::var("TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost:5432/workflow".into())
}

/// 建立一個乾淨的測試租戶，連同一個使用者
///
/// 使用者是必要的：`form_definition.created_by` 有外鍵指向 `app_user`，
/// 隨便給一個 UUID 會被擋下。
async fn new_tenant() -> (Db, Uuid, Uuid) {
    let db = Db::connect(&database_url()).await.expect("連線失敗");
    let admin = sqlx::postgres::PgPool::connect(&admin_url())
        .await
        .expect("管理連線失敗");

    let tenant_id = Uuid::new_v4();
    sqlx::query("insert into tenant (id, code, name) values ($1, $2, $3)")
        .bind(tenant_id)
        .bind(format!("fvl-{}", &tenant_id.to_string()[..8]))
        .bind("表單版本鎖定測試租戶")
        .execute(&admin)
        .await
        .expect("建立租戶失敗");

    let mut tx = db.tenant_tx(tenant_id).await.unwrap();
    let user_id: Uuid = sqlx::query_scalar(
        "insert into app_user (tenant_id, email, name, password_hash)
         values ($1, 'fvl@test.local', '測試者', 'x') returning id",
    )
    .bind(tenant_id)
    .fetch_one(tx.executor())
    .await
    .expect("建立使用者失敗");
    tx.commit().await.unwrap();

    (db, tenant_id, user_id)
}

fn form_content(form_key: &str, business_object: &str) -> serde_json::Value {
    json!({
        "form_key": form_key,
        "version": 1,
        "business_object": business_object,
        "sections": [{ "key": "main", "title": "主要" }],
        "fields": [{
            "key": "number",
            "section": "main",
            "ui": { "component": "input", "label": "單號" },
            "data": { "path": format!("{business_object}.number"), "type": "string" }
        }]
    })
}

/// 建立表單並發布，回傳已發布版本的 id
async fn seed_published_form(
    db: &Db,
    tenant_id: Uuid,
    form_key: &str,
    business_object: &str,
    actor: Uuid,
) -> Uuid {
    let mut tx = db.tenant_tx(tenant_id).await.unwrap();

    let (definition, _draft) = persistence::form::create(
        &mut tx,
        persistence::form::CreateForm {
            form_key: form_key.into(),
            business_object: business_object.into(),
            name: form_key.into(),
            content: form_content(form_key, business_object),
        },
        actor,
    )
    .await
    .expect("建立表單失敗");

    let published = persistence::form::publish(&mut tx, definition.id, actor)
        .await
        .expect("發布表單失敗");

    tx.commit().await.unwrap();
    published.id
}

#[tokio::test]
async fn resolves_published_form_for_business_object() {
    // 正向：業務物件有一張已發布的表單，就鎖定它
    let (db, tenant_id, actor) = new_tenant().await;

    let version_id = seed_published_form(&db, tenant_id, "leave_form", "leave", actor).await;

    let mut tx = db.tenant_tx(tenant_id).await.unwrap();
    let found = persistence::form::find_published_by_business_object(&mut tx, "leave")
        .await
        .expect("查詢失敗");

    assert_eq!(
        found.map(|v| v.id),
        Some(version_id),
        "應找到該業務物件的已發布表單版本"
    );
}

#[tokio::test]
async fn returns_none_when_business_object_has_no_form() {
    // 沒有表單的業務物件不該讓啟動流程失敗——回 None，由呼叫端決定
    let (db, tenant_id, _actor) = new_tenant().await;

    let mut tx = db.tenant_tx(tenant_id).await.unwrap();
    let found = persistence::form::find_published_by_business_object(&mut tx, "nonexistent")
        .await
        .expect("查詢失敗");

    assert!(found.is_none(), "查無表單時應回 None 而非錯誤");
}

#[tokio::test]
async fn ignores_unpublished_forms() {
    // 只鎖已發布的版本。草稿是設計中的半成品，鎖它等於沒鎖——
    // 草稿隨時會變，而版本鎖定的目的正是「不要變」。
    let (db, tenant_id, actor) = new_tenant().await;

    let mut tx = db.tenant_tx(tenant_id).await.unwrap();
    persistence::form::create(
        &mut tx,
        persistence::form::CreateForm {
            form_key: "draft_only".into(),
            business_object: "draft_bo".into(),
            name: "只有草稿".into(),
            content: form_content("draft_only", "draft_bo"),
        },
        actor,
    )
    .await
    .expect("建立表單失敗");
    tx.commit().await.unwrap();

    let mut tx = db.tenant_tx(tenant_id).await.unwrap();
    let found = persistence::form::find_published_by_business_object(&mut tx, "draft_bo")
        .await
        .expect("查詢失敗");

    assert!(found.is_none(), "只有草稿時不該鎖定");
}

#[tokio::test]
async fn picks_most_recently_published_when_ambiguous() {
    // 同一個業務物件有多張已發布表單時要有確定的結果。
    //
    // 這在乾淨的租戶不該發生（一個業務物件一張表單），
    // 但測試殘留會造成——demo 租戶的 expense_claim 就有兩張。
    // 選最近發布的而非隨機，是為了讓同一份資料每次都得到同樣的答案；
    // 不確定的結果會讓「為什麼這張單綁到那張表單」無法追查。
    let (db, tenant_id, actor) = new_tenant().await;

    seed_published_form(&db, tenant_id, "expense_a", "expense", actor).await;
    let later = seed_published_form(&db, tenant_id, "expense_b", "expense", actor).await;

    let mut tx = db.tenant_tx(tenant_id).await.unwrap();
    let found = persistence::form::find_published_by_business_object(&mut tx, "expense")
        .await
        .expect("查詢失敗");

    assert_eq!(
        found.map(|v| v.id),
        Some(later),
        "多張已發布時應取最近發布的"
    );
}

#[tokio::test]
async fn does_not_cross_tenants() {
    // RLS 已經擋住跨租戶，但這條是回歸測試——
    // 表單版本會被寫進 workflow_instance，鎖到別的租戶的版本
    // 等於把那個租戶的表單結構洩漏給這個租戶。
    let (db, tenant_a, actor) = new_tenant().await;
    let (_, tenant_b, _) = new_tenant().await;

    seed_published_form(&db, tenant_a, "shared_key", "shared_bo", actor).await;

    let mut tx = db.tenant_tx(tenant_b).await.unwrap();
    let found = persistence::form::find_published_by_business_object(&mut tx, "shared_bo")
        .await
        .expect("查詢失敗");

    assert!(found.is_none(), "不該看到他租戶的表單");
}

/// 建立一個流程實例，回傳 id
async fn seed_instance(
    db: &Db,
    tenant_id: Uuid,
    actor: Uuid,
    business_object: &str,
    form_version_id: Option<Uuid>,
) -> Uuid {
    let mut tx = db.tenant_tx(tenant_id).await.unwrap();

    let wf_id: Uuid = sqlx::query_scalar(
        "insert into workflow_definition (tenant_id, workflow_key, business_object, name)
         values ($1, $2, $3, '測試流程') returning id",
    )
    .bind(tenant_id)
    .bind(format!("wf_{}", &Uuid::new_v4().to_string()[..8]))
    .bind(business_object)
    .fetch_one(tx.executor())
    .await
    .unwrap();

    // PUBLISHED 需同時有 version 與 published_at（wf_published_has_version，0004:51）
    let version_id: Uuid = sqlx::query_scalar(
        "insert into workflow_definition_version
            (tenant_id, workflow_id, version, content, status, published_at)
         values ($1, $2, 1, '{}'::jsonb, 'PUBLISHED', now()) returning id",
    )
    .bind(tenant_id)
    .bind(wf_id)
    .fetch_one(tx.executor())
    .await
    .unwrap();

    let instance = persistence::instance::create(
        &mut tx,
        persistence::instance::CreateInstance {
            workflow_version_id: version_id,
            form_version_id,
            business_object: business_object.into(),
            business_key: format!("BK-{}", &Uuid::new_v4().to_string()[..8]),
            temporal_workflow_id: format!("{tenant_id}:{business_object}:x"),
            temporal_run_id: String::new(),
            input: json!({}),
        },
        actor,
    )
    .await
    .expect("建立實例失敗");

    tx.commit().await.unwrap();
    instance.id
}

#[tokio::test]
async fn instance_stores_and_reads_back_form_version() {
    // 建立時鎖定的版本，之後讀得回來
    let (db, tenant_id, actor) = new_tenant().await;
    let version_id = seed_published_form(&db, tenant_id, "bo_form", "bo", actor).await;

    let instance_id = seed_instance(&db, tenant_id, actor, "bo", Some(version_id)).await;

    let mut tx = db.tenant_tx(tenant_id).await.unwrap();
    let found = persistence::instance::find_by_id(&mut tx, instance_id)
        .await
        .expect("查詢失敗");

    assert_eq!(found.form_version_id, Some(version_id));
}

#[tokio::test]
async fn list_returns_form_version_column() {
    // 回歸：WorkflowInstance 加欄位時，list 的 select 也要跟著加。
    //
    // 加了欄位卻漏改 list 的 SQL，編譯會過、單元測試會過，
    // 但 GET /instances 在執行期直接 500——sqlx 的 FromRow
    // 找不到那一欄。這個缺口先前真的發生過，
    // 因為沒有任何測試呼叫到 list。
    let (db, tenant_id, actor) = new_tenant().await;
    let version_id = seed_published_form(&db, tenant_id, "list_form", "list_bo", actor).await;
    seed_instance(&db, tenant_id, actor, "list_bo", Some(version_id)).await;

    let mut tx = db.tenant_tx(tenant_id).await.unwrap();
    let rows = persistence::instance::list(
        &mut tx,
        persistence::instance::ListFilter {
            visible_to: None,
            business_object: Some("list_bo".into()),
            status: None,
            limit: 10,
        },
    )
    .await
    .expect("list 失敗——欄位與 select 不一致時會在這裡爆");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].form_version_id, Some(version_id));
}

#[tokio::test]
async fn instance_without_form_is_allowed() {
    // 沒有表單的業務物件仍可建立實例，不該擋
    let (db, tenant_id, actor) = new_tenant().await;
    let instance_id = seed_instance(&db, tenant_id, actor, "no_form_bo", None).await;

    let mut tx = db.tenant_tx(tenant_id).await.unwrap();
    let found = persistence::instance::find_by_id(&mut tx, instance_id)
        .await
        .expect("查詢失敗");

    assert!(found.form_version_id.is_none());
}
