//! 表單定義 API 整合測試
//!
//! 對真實 PostgreSQL 執行，驗證 SQL、RLS、版本狀態轉換。
//!
//! 前置：
//!   docker compose -f deploy/docker-compose.yml up -d postgres
//!   套用 server/migrations/*.sql
//!
//! 每個測試用獨立租戶，測試間互不干擾，可平行執行。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_public::{AppState, JwtKeys};
use persistence::Db;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

const JWT_SECRET: &[u8] = b"integration-test-secret-32-bytes!!!!";

fn database_url() -> String {
    std::env::var("TEST_DATABASE_URL").unwrap_or_else(|_| {
        "postgres://workflow_app:app_dev_only@localhost:5433/workflow".into()
    })
}

fn admin_url() -> String {
    std::env::var("TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://app:app_dev_only@localhost:5433/workflow".into())
}

/// 測試情境：一個租戶、一個使用者、一組 token
struct Ctx {
    state: AppState,
    tenant_id: Uuid,
    designer_token: String,
    viewer_token: String,
}

impl Ctx {
    /// 建立獨立租戶與兩種角色的使用者
    async fn new() -> Self {
        let db = Db::connect(&database_url())
            .await
            .expect("連線失敗。請確認容器已啟動且 migration 已套用");

        let admin = sqlx::postgres::PgPool::connect(&admin_url())
            .await
            .expect("管理連線失敗");

        let tenant_id = Uuid::new_v4();
        let code = format!("test-{}", &tenant_id.to_string()[..8]);

        sqlx::query("insert into tenant (id, code, name) values ($1, $2, $3)")
            .bind(tenant_id)
            .bind(&code)
            .bind("整合測試租戶")
            .execute(&admin)
            .await
            .expect("建立租戶失敗");

        let jwt = JwtKeys::new(JWT_SECRET, 1);
        let mut tx = db.tenant_tx(tenant_id).await.expect("開啟交易失敗");

        // 兩個角色
        for (c, n) in [("designer", "設計者"), ("viewer", "檢視者")] {
            sqlx::query("insert into role (tenant_id, code, name) values ($1, $2, $3)")
                .bind(tenant_id)
                .bind(c)
                .bind(n)
                .execute(tx.executor())
                .await
                .expect("建立角色失敗");
        }

        let mut tokens = Vec::new();
        for (email, name, role) in [
            ("designer@test.local", "設計者", "designer"),
            ("viewer@test.local", "檢視者", "viewer"),
        ] {
            let uid: Uuid = sqlx::query_scalar(
                "insert into app_user (tenant_id, email, name, password_hash)
                 values ($1, $2, $3, 'x') returning id",
            )
            .bind(tenant_id)
            .bind(email)
            .bind(name)
            .fetch_one(tx.executor())
            .await
            .expect("建立使用者失敗");

            sqlx::query(
                "insert into user_role (user_id, role_id, tenant_id)
                 select $1, id, $2 from role where tenant_id = $2 and code = $3",
            )
            .bind(uid)
            .bind(tenant_id)
            .bind(role)
            .execute(tx.executor())
            .await
            .expect("指派角色失敗");

            tokens.push(
                jwt.issue(uid, tenant_id, name.into(), vec![role.into()])
                    .expect("簽發 token 失敗"),
            );
        }

        tx.commit().await.expect("提交失敗");

        Self {
            state: AppState { db, jwt },
            tenant_id,
            designer_token: tokens[0].clone(),
            viewer_token: tokens[1].clone(),
        }
    }

    async fn send(&self, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }

        let req = match body {
            Some(b) => req
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(b.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };

        let resp = http_public::router(self.state.clone())
            .oneshot(req)
            .await
            .expect("請求失敗");

        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .expect("讀取回應失敗");
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    async fn designer(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        self.send(method, path, Some(&self.designer_token), body).await
    }
}

fn sample_form(key: &str) -> Value {
    json!({
        "form_key": key,
        "version": 1,
        "business_object": "quotation",
        "name": "測試報價單",
        "fields": [{
            "key": "amount",
            "ui": { "component": "number", "label": "金額", "precision": 2 },
            "data": { "path": "quotation.amount", "type": "decimal" }
        }]
    })
}

// ── 基本流程 ────────────────────────────────────────────

#[tokio::test]
async fn create_then_get_returns_draft() {
    let ctx = Ctx::new().await;

    let (status, created) = ctx
        .designer("POST", "/forms", Some(json!({
            "form_key": "form_one",
            "business_object": "quotation",
            "name": "測試表單",
            "content": sample_form("form_one"),
        })))
        .await;
    assert_eq!(status, StatusCode::CREATED, "建立失敗：{created}");

    let (status, detail) = ctx.designer("GET", "/forms/form_one", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!detail["draft"].is_null(), "應有草稿");
    assert!(detail["published"].is_null(), "尚未發布不應有 published");
    assert!(detail["draft"]["version"].is_null(), "草稿的 version 應為 null");
}

#[tokio::test]
async fn publish_assigns_version_one_then_two() {
    let ctx = Ctx::new().await;
    ctx.designer("POST", "/forms", Some(json!({
        "form_key": "form_two", "business_object": "quotation",
        "name": "版本測試", "content": sample_form("form_two"),
    })))
    .await;

    let (status, v1) = ctx.designer("POST", "/forms/form_two/draft/publish", None).await;
    assert_eq!(status, StatusCode::OK, "第一次發布失敗：{v1}");
    assert_eq!(v1["version"], 1);
    assert_eq!(v1["status"], "PUBLISHED");

    // 再存一次草稿後發布，版本應為 2
    let mut changed = sample_form("form_two");
    changed["fields"][0]["ui"]["label"] = json!("修改後金額");
    ctx.designer("PUT", "/forms/form_two/draft", Some(json!({ "content": changed })))
        .await;

    let (status, v2) = ctx.designer("POST", "/forms/form_two/draft/publish", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v2["version"], 2, "第二次發布應為版本 2");

    let (_, versions) = ctx.designer("GET", "/forms/form_two/versions", None).await;
    assert_eq!(versions.as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn published_version_content_is_immutable() {
    let ctx = Ctx::new().await;
    ctx.designer("POST", "/forms", Some(json!({
        "form_key": "form_three", "business_object": "quotation",
        "name": "不可變測試", "content": sample_form("form_three"),
    })))
    .await;
    ctx.designer("POST", "/forms/form_three/draft/publish", None).await;

    // 直接從資料層嘗試修改已發布版本，應被 trigger 擋下
    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let form = persistence::form::find_by_key(&mut tx, "form_three").await.unwrap();
    let published = persistence::form::get_latest_published(&mut tx, form.id)
        .await
        .unwrap()
        .unwrap();

    let result = sqlx::query("update form_definition_version set content = '{}' where id = $1")
        .bind(published.id)
        .execute(tx.executor())
        .await;

    assert!(result.is_err(), "已發布版本的內容不應可修改");
}

#[tokio::test]
async fn publish_without_draft_is_conflict() {
    let ctx = Ctx::new().await;
    ctx.designer("POST", "/forms", Some(json!({
        "form_key": "form_four", "business_object": "quotation",
        "name": "無草稿測試", "content": sample_form("form_four"),
    })))
    .await;
    ctx.designer("POST", "/forms/form_four/draft/publish", None).await;

    // 草稿已用掉，再發布應失敗
    let (status, err) = ctx.designer("POST", "/forms/form_four/draft/publish", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(err["code"], "CONFLICT");
}

#[tokio::test]
async fn discard_draft_keeps_published_version() {
    let ctx = Ctx::new().await;
    ctx.designer("POST", "/forms", Some(json!({
        "form_key": "form_five", "business_object": "quotation",
        "name": "捨棄測試", "content": sample_form("form_five"),
    })))
    .await;
    ctx.designer("POST", "/forms/form_five/draft/publish", None).await;

    let mut changed = sample_form("form_five");
    changed["fields"][0]["ui"]["label"] = json!("草稿中的改動");
    ctx.designer("PUT", "/forms/form_five/draft", Some(json!({ "content": changed })))
        .await;

    let (status, _) = ctx.designer("DELETE", "/forms/form_five/draft", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, detail) = ctx.designer("GET", "/forms/form_five", None).await;
    assert!(detail["draft"].is_null(), "草稿應已刪除");
    assert_eq!(detail["published"]["version"], 1, "已發布版本應保留");
}

// ── 驗證 ────────────────────────────────────────────────

#[tokio::test]
async fn create_rejects_invalid_schema() {
    let ctx = Ctx::new().await;
    let mut bad = sample_form("bad_form");
    bad["fields"][0]["ui"]["component"] = json!("richtext"); // 不在允許清單

    let (status, err) = ctx
        .designer("POST", "/forms", Some(json!({
            "form_key": "bad_form", "business_object": "quotation",
            "name": "壞表單", "content": bad,
        })))
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(err["code"], "VALIDATION_FAILED");
    assert!(err["details"]["errors"].is_array(), "應附上錯誤清單供前端顯示");
}

#[tokio::test]
async fn draft_allows_incomplete_but_publish_rejects() {
    let ctx = Ctx::new().await;

    // 兩個欄位綁同一資料路徑：結構合法，語意有問題
    let mut form = sample_form("form_six");
    let mut dup = form["fields"][0].clone();
    dup["key"] = json!("amount_again");
    form["fields"].as_array_mut().unwrap().push(dup);

    let (status, _) = ctx
        .designer("POST", "/forms", Some(json!({
            "form_key": "form_six", "business_object": "quotation",
            "name": "語意測試", "content": form,
        })))
        .await;
    assert_eq!(status, StatusCode::CREATED, "草稿階段應允許存檔");

    let (status, err) = ctx.designer("POST", "/forms/form_six/draft/publish", None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "發布時應擋下");
    assert!(
        err["message"].as_str().unwrap().contains("同一資料路徑"),
        "錯誤訊息應指出問題：{err}"
    );
}

#[tokio::test]
async fn validate_endpoint_reports_without_publishing() {
    let ctx = Ctx::new().await;
    let mut form = sample_form("form_seven");
    form["sections"] = json!([{ "key": "basic", "title": "基本" }]);
    form["fields"][0]["section"] = json!("nonexistent");

    ctx.designer("POST", "/forms", Some(json!({
        "form_key": "form_seven", "business_object": "quotation",
        "name": "驗證端點測試", "content": form,
    })))
    .await;

    let (status, result) = ctx.designer("POST", "/forms/form_seven/draft/validate", None).await;
    assert_eq!(status, StatusCode::OK, "驗證端點本身應成功回應");
    assert_eq!(result["valid"], false);
    assert!(!result["errors"].as_array().unwrap().is_empty());

    // 驗證不應改變狀態
    let (_, detail) = ctx.designer("GET", "/forms/form_seven", None).await;
    assert!(detail["published"].is_null(), "驗證不應發布");
}

// ── 權限 ────────────────────────────────────────────────

#[tokio::test]
async fn viewer_can_read_but_not_modify() {
    let ctx = Ctx::new().await;
    ctx.designer("POST", "/forms", Some(json!({
        "form_key": "form_eight", "business_object": "quotation",
        "name": "權限測試", "content": sample_form("form_eight"),
    })))
    .await;

    let (status, _) = ctx
        .send("GET", "/forms/form_eight", Some(&ctx.viewer_token), None)
        .await;
    assert_eq!(status, StatusCode::OK, "viewer 應可讀取");

    let (status, err) = ctx
        .send("PUT", "/forms/form_eight/draft", Some(&ctx.viewer_token),
              Some(json!({ "content": sample_form("form_eight") })))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "viewer 不應可修改");
    assert_eq!(err["code"], "FORBIDDEN");

    let (status, _) = ctx
        .send("POST", "/forms/form_eight/draft/publish", Some(&ctx.viewer_token), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "viewer 不應可發布");
}

#[tokio::test]
async fn no_token_is_unauthorized() {
    let ctx = Ctx::new().await;
    let (status, err) = ctx.send("GET", "/forms", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(err["code"], "UNAUTHORIZED");
}

#[tokio::test]
async fn token_from_other_secret_is_rejected() {
    let ctx = Ctx::new().await;
    let forged = JwtKeys::new(b"attacker-secret-32-bytes-long!!!!!!!", 1)
        .issue(Uuid::new_v4(), ctx.tenant_id, "駭客".into(), vec!["admin".into()])
        .unwrap();

    let (status, _) = ctx.send("GET", "/forms", Some(&forged), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "偽造的 token 必須被拒");
}

// ── 租戶隔離（最關鍵）────────────────────────────────────

#[tokio::test]
async fn tenant_cannot_see_other_tenant_forms() {
    let a = Ctx::new().await;
    let b = Ctx::new().await;

    a.designer("POST", "/forms", Some(json!({
        "form_key": "secret_form", "business_object": "quotation",
        "name": "A 租戶的機密表單", "content": sample_form("secret_form"),
    })))
    .await;

    // B 用自己的 token 查詢，不應看到 A 的表單
    let (status, list) = b.designer("GET", "/forms", None).await;
    assert_eq!(status, StatusCode::OK);
    let keys: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["form_key"].as_str())
        .collect();
    assert!(!keys.contains(&"secret_form"), "B 不應看到 A 的表單：{keys:?}");

    // 直接指名查詢也要回 404，不能回 403 或洩漏存在性
    let (status, _) = b.designer("GET", "/forms/secret_form", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "跨租戶查詢應回 404");
}

#[tokio::test]
async fn token_with_forged_tenant_id_sees_nothing() {
    let a = Ctx::new().await;
    a.designer("POST", "/forms", Some(json!({
        "form_key": "target", "business_object": "quotation",
        "name": "目標表單", "content": sample_form("target"),
    })))
    .await;

    // 攻擊情境：取得合法 token 但把 tenant_id 換成別人的。
    // 由於 token 由伺服器簽發，此處模擬伺服器金鑰外洩的最壞情況。
    // 即使如此，RLS 仍以 token 內的 tenant_id 為準，
    // 攻擊者只能看到他宣稱的那個租戶，無法跨租戶存取。
    let other_tenant = Uuid::new_v4();
    let forged = a
        .state
        .jwt
        .issue(Uuid::new_v4(), other_tenant, "攻擊者".into(), vec!["admin".into()])
        .unwrap();

    let (status, list) = a.send("GET", "/forms", Some(&forged), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        list.as_array().unwrap().is_empty(),
        "不存在的租戶應看不到任何資料：{list}"
    );
}

// ── 稽核 ────────────────────────────────────────────────

#[tokio::test]
async fn publish_writes_audit_event() {
    let ctx = Ctx::new().await;
    ctx.designer("POST", "/forms", Some(json!({
        "form_key": "form_nine", "business_object": "quotation",
        "name": "稽核測試", "content": sample_form("form_nine"),
    })))
    .await;
    ctx.designer("POST", "/forms/form_nine/draft/publish", None).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let actions: Vec<String> =
        sqlx::query_scalar("select action from audit_event order by at")
            .fetch_all(tx.executor())
            .await
            .unwrap();

    assert!(actions.contains(&"form.create".to_string()), "建立應寫稽核");
    assert!(actions.contains(&"form.publish".to_string()), "發布應寫稽核");
}

#[tokio::test]
async fn audit_events_cannot_be_modified() {
    let ctx = Ctx::new().await;
    ctx.designer("POST", "/forms", Some(json!({
        "form_key": "form_ten", "business_object": "quotation",
        "name": "稽核不可變", "content": sample_form("form_ten"),
    })))
    .await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();

    let updated = sqlx::query("update audit_event set action = 'tampered' where action = 'form.create'")
        .execute(tx.executor())
        .await;
    assert!(updated.is_err(), "稽核事件不應可修改");
}
