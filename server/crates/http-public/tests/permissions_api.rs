//! 權限矩陣 API 整合測試
//!
//! 對應設計稿 _7 的功能。重點驗證：
//!   矩陣結構正確、三態判定符合規則、鎖定欄位不可修改、租戶隔離。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_public::{AppState, JwtKeys};
use persistence::Db;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

const JWT_SECRET: &[u8] = b"integration-test-secret-32-bytes!!!!";

fn database_url() -> String {
    std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://workflow_app:app_dev_only@localhost:5432/workflow".into())
}

fn admin_url() -> String {
    std::env::var("TEST_ADMIN_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost:5432/workflow".into())
}

struct Ctx {
    state: AppState,
    token: String,
}

impl Ctx {
    async fn new() -> Self {
        let db = Db::connect(&database_url()).await.expect("連線失敗");
        let admin = sqlx::postgres::PgPool::connect(&admin_url())
            .await
            .expect("管理連線失敗");

        let tenant_id = Uuid::new_v4();
        sqlx::query("insert into tenant (id, code, name) values ($1, $2, $3)")
            .bind(tenant_id)
            .bind(format!("perm-{}", &tenant_id.to_string()[..8]))
            .bind("權限測試租戶")
            .execute(&admin)
            .await
            .expect("建立租戶失敗");

        let jwt = JwtKeys::new(JWT_SECRET, 1);
        let mut tx = db.tenant_tx(tenant_id).await.unwrap();

        sqlx::query("insert into role (tenant_id, code, name) values ($1, 'designer', '設計者')")
            .bind(tenant_id)
            .execute(tx.executor())
            .await
            .unwrap();

        let uid: Uuid = sqlx::query_scalar(
            "insert into app_user (tenant_id, email, name, password_hash)
             values ($1, 'd@test.local', '設計者', 'x') returning id",
        )
        .bind(tenant_id)
        .fetch_one(tx.executor())
        .await
        .unwrap();

        tx.commit().await.unwrap();

        let token = jwt
            .issue(uid, tenant_id, "設計者".into(), vec!["designer".into()])
            .unwrap();

        Self {
            state: AppState { db, jwt },
            token,
        }
    }

    async fn get(&self, path: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("GET")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token))
            .body(Body::empty())
            .unwrap();

        let resp = http_public::router(self.state.clone())
            .oneshot(req)
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn post(&self, path: &str, body: Value) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();

        let resp = http_public::router(self.state.clone())
            .oneshot(req)
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// 建立一張涵蓋各種權限情境的表單
    async fn seed_form(&self, key: &str) {
        let content = json!({
            "form_key": key,
            "version": 1,
            "business_object": "quotation",
            "sections": [
                { "key": "customer", "title": "客戶資訊" },
                { "key": "internal", "title": "內部資訊" }
            ],
            "fields": [
                {
                    "key": "customer_name",
                    "section": "customer",
                    "ui": { "component": "input", "label": "客戶名稱" },
                    "data": { "path": "quotation.customer_name", "type": "string" }
                },
                {
                    "key": "discount_rate",
                    "section": "customer",
                    "ui": { "component": "number", "label": "表頭折扣率" },
                    "data": { "path": "quotation.discount_rate", "type": "decimal" },
                    "workflow": { "readonly_when": "node.id not in ['start','revise']" }
                },
                {
                    "key": "total_amount",
                    "section": "customer",
                    "ui": { "component": "display", "label": "報價總計" },
                    "data": {
                        "path": "quotation.total_amount",
                        "type": "decimal",
                        "computed": "sum(quotation.lines.amount)"
                    }
                },
                {
                    "key": "gross_margin",
                    "section": "internal",
                    "ui": { "component": "display", "label": "預估毛利率", "external_visible": false },
                    "data": { "path": "quotation.gross_margin", "type": "decimal" }
                },
                {
                    "key": "cost_detail",
                    "section": "internal",
                    "ui": { "component": "number", "label": "成本明細" },
                    "data": { "path": "quotation.cost_detail", "type": "decimal" },
                    "workflow": { "readable_roles": ["admin", "finance_manager"] }
                }
            ]
        });

        self.post(
            "/forms",
            json!({
                "form_key": key,
                "business_object": "quotation",
                "name": "權限測試表單",
                "content": content,
            }),
        )
        .await;
    }
}

// ── 矩陣結構 ────────────────────────────────────────────

#[tokio::test]
async fn matrix_by_role_returns_all_fields_as_rows() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_matrix").await;

    let (status, m) = ctx
        .get("/forms/perm_matrix/permissions?mode=by_role&role=requester")
        .await;

    assert_eq!(status, StatusCode::OK, "取得矩陣失敗：{m}");
    assert_eq!(m["rows"].as_array().unwrap().len(), 5, "五個欄位應各為一列");
    assert_eq!(m["version"], Value::Null, "應使用草稿");

    let first = &m["rows"][0];
    assert_eq!(first["key"], "customer_name");
    assert_eq!(first["data_path"], "quotation.customer_name");
    assert_eq!(first["section"], "customer");
}

#[tokio::test]
async fn matrix_includes_section_summary() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_sections").await;

    let (_, m) = ctx
        .get("/forms/perm_sections/permissions?mode=by_role&role=requester")
        .await;

    let sections = m["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 2);

    let customer = sections.iter().find(|s| s["key"] == "customer").unwrap();
    assert_eq!(customer["title"], "客戶資訊");
    assert_eq!(customer["field_count"], 3, "客戶資訊有三個欄位");
}

#[tokio::test]
async fn by_node_mode_transposes_to_roles() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_bynode").await;

    let (status, m) = ctx
        .get("/forms/perm_bynode/permissions?mode=by_node&node_id=start")
        .await;

    assert_eq!(status, StatusCode::OK, "{m}");
    // 列變成角色
    let keys: Vec<&str> = m["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["key"].as_str())
        .collect();
    assert!(keys.contains(&"admin"));
    assert!(keys.contains(&"requester"));

    // 欄變成欄位
    let cols: Vec<&str> = m["columns"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["key"].as_str())
        .collect();
    assert!(cols.contains(&"customer_name"));
}

#[tokio::test]
async fn by_role_requires_role_param() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_noparam").await;

    let (status, err) = ctx.get("/forms/perm_noparam/permissions?mode=by_role").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(err["code"], "BAD_REQUEST");
}

// ── 權限判定 ────────────────────────────────────────────

#[tokio::test]
async fn computed_field_is_locked_readonly() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_computed").await;

    let (_, m) = ctx
        .get("/forms/perm_computed/permissions?mode=by_role&role=admin")
        .await;

    let total = m["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == "total_amount")
        .expect("應有 total_amount 列");

    let cell = &total["cells"]["start"];
    assert_eq!(cell["permission"], "READONLY", "計算欄位應唯讀");
    assert_eq!(cell["locked"], true, "計算欄位在 UI 應鎖定不可點擊");
    assert_eq!(cell["reason"], "系統計算欄位");
}

#[tokio::test]
async fn role_outside_readable_list_sees_hidden() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_hidden").await;

    let (_, m) = ctx
        .get("/forms/perm_hidden/permissions?mode=by_role&role=requester")
        .await;

    let cost = m["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == "cost_detail")
        .unwrap();

    assert_eq!(
        cost["cells"]["start"]["permission"], "HIDDEN",
        "requester 不在 readable_roles 內應隱藏"
    );
}

#[tokio::test]
async fn admin_sees_everything_editable() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_admin").await;

    let (_, m) = ctx
        .get("/forms/perm_admin/permissions?mode=by_role&role=admin")
        .await;

    for row in m["rows"].as_array().unwrap() {
        let p = row["cells"]["start"]["permission"].as_str().unwrap();
        let key = row["key"].as_str().unwrap();
        // 計算欄位例外，admin 也改不了
        if key == "total_amount" {
            assert_eq!(p, "READONLY", "計算欄位對 admin 仍唯讀");
        } else {
            assert_eq!(p, "EDITABLE", "admin 應可編輯 {key}");
        }
    }
}

#[tokio::test]
async fn external_visible_false_marked_in_row() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_external").await;

    let (_, m) = ctx
        .get("/forms/perm_external/permissions?mode=by_role&role=requester")
        .await;

    let margin = m["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["key"] == "gross_margin")
        .unwrap();

    assert_eq!(
        margin["external_visible"], false,
        "UI 需依此顯示「客戶看不到」標記"
    );
}

// ── 預覽 ────────────────────────────────────────────────

#[tokio::test]
async fn preview_resolves_for_given_context() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_preview").await;

    // 在 start 節點，折扣率可編輯
    let (status, at_start) = ctx
        .get("/forms/perm_preview/permissions/preview?node_id=start&roles=requester")
        .await;
    assert_eq!(status, StatusCode::OK, "{at_start}");

    let discount = at_start
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "discount_rate")
        .unwrap();
    assert_eq!(discount["permission"], "EDITABLE");

    // 在簽核節點，折扣率唯讀
    let (_, at_approval) = ctx
        .get("/forms/perm_preview/permissions/preview?node_id=manager_approval&roles=requester")
        .await;

    let discount = at_approval
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "discount_rate")
        .unwrap();
    assert_eq!(
        discount["permission"], "READONLY",
        "readonly_when 應在簽核節點生效"
    );
}

#[tokio::test]
async fn preview_for_external_hides_internal_fields() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_ext_preview").await;

    let (_, result) = ctx
        .get("/forms/perm_ext_preview/permissions/preview?node_id=customer_review&participant_kind=external")
        .await;

    let fields = result.as_array().unwrap();

    let margin = fields.iter().find(|f| f["key"] == "gross_margin").unwrap();
    assert_eq!(margin["permission"], "HIDDEN", "毛利率不給客戶看");

    let name = fields.iter().find(|f| f["key"] == "customer_name").unwrap();
    assert_eq!(name["permission"], "READONLY", "客戶只簽核不改內容");
}

// ── 租戶隔離 ────────────────────────────────────────────

#[tokio::test]
async fn cannot_read_other_tenant_matrix() {
    let a = Ctx::new().await;
    let b = Ctx::new().await;

    a.seed_form("perm_secret").await;

    let (status, _) = b
        .get("/forms/perm_secret/permissions?mode=by_role&role=admin")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "跨租戶應回 404");
}

#[tokio::test]
async fn matrix_requires_authentication() {
    let ctx = Ctx::new().await;
    ctx.seed_form("perm_auth").await;

    let req = Request::builder()
        .method("GET")
        .uri("/forms/perm_auth/permissions?mode=by_role&role=admin")
        .body(Body::empty())
        .unwrap();

    let resp = http_public::router(ctx.state.clone())
        .oneshot(req)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
