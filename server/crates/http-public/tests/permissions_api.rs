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
    /// 建立流程實例時需要（預覽 API 的 instance_id 測試）
    tenant_id: Uuid,
    user_id: Uuid,
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
            state: AppState {
                db,
                jwt,
                temporal: None,
                internal_token: Some("test-token".into()),
                // 測試不寄信
                mailer: http_public::Mailer::disabled(),
            },
            token,
            tenant_id,
            user_id: uid,
        }
    }

    /// 建立一個帶指定業務資料的流程實例
    ///
    /// 預覽 API 帶 instance_id 時要用這筆資料求值條件式。
    async fn seed_instance(&self, input: Value) -> Uuid {
        let mut tx = self.state.db.tenant_tx(self.tenant_id).await.unwrap();

        let wf_id: Uuid = sqlx::query_scalar(
            "insert into workflow_definition
                (tenant_id, workflow_key, business_object, name)
             values ($1, 'perm_preview_flow', 'quotation', '預覽測試流程')
             returning id",
        )
        .bind(self.tenant_id)
        .fetch_one(tx.executor())
        .await
        .unwrap();

        // PUBLISHED 需同時有 version 與 published_at
        // （wf_published_has_version 約束，0004:51）
        let version_id: Uuid = sqlx::query_scalar(
            "insert into workflow_definition_version
                (tenant_id, workflow_id, version, content, status, published_at)
             values ($1, $2, 1, '{}'::jsonb, 'PUBLISHED', now()) returning id",
        )
        .bind(self.tenant_id)
        .bind(wf_id)
        .fetch_one(tx.executor())
        .await
        .unwrap();

        let instance_id: Uuid = sqlx::query_scalar(
            "insert into workflow_instance
                (tenant_id, workflow_version_id, business_object, business_key,
                 temporal_workflow_id, started_by, input)
             values ($1, $2, 'quotation', 'QT-PREVIEW-1', $3, $4, $5) returning id",
        )
        .bind(self.tenant_id)
        .bind(version_id)
        .bind(format!("{}:quotation:QT-PREVIEW-1", self.tenant_id))
        .bind(self.user_id)
        .bind(&input)
        .fetch_one(tx.executor())
        .await
        .unwrap();

        tx.commit().await.unwrap();
        instance_id
    }

    /// 建立一張含「依業務資料決定顯示」欄位的表單
    ///
    /// 這正是預覽 API 傳空 data 時會判錯的情境：
    /// eval_bool 取不到 quotation.total 就當成 false，欄位被隱藏。
    async fn seed_form_with_data_condition(&self, key: &str) {
        let content = json!({
            "form_key": key,
            "version": 1,
            "business_object": "quotation",
            "sections": [{ "key": "main", "title": "主要" }],
            "fields": [
                {
                    "key": "big_deal_note",
                    "section": "main",
                    "ui": { "component": "textarea", "label": "大額交易說明" },
                    "data": { "path": "quotation.big_deal_note", "type": "text" },
                    "workflow": { "visible_when": "quotation.total > 1000000" }
                }
            ]
        });

        self.post(
            "/forms",
            json!({
                "form_key": key,
                "business_object": "quotation",
                "name": "條件顯示測試表單",
                "content": content,
            }),
        )
        .await;
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
                    "key": "total",
                    "section": "customer",
                    "ui": { "component": "display", "label": "報價總計" },
                    "data": {
                        "path": "quotation.total",
                        "type": "decimal",
                        "computed": "sum(quotation.lines.amount)"
                    }
                },
                {
                    "key": "margin_rate",
                    "section": "internal",
                    "ui": { "component": "display", "label": "預估毛利率", "external_visible": false },
                    "data": { "path": "quotation.margin_rate", "type": "decimal" }
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
        .find(|r| r["key"] == "total")
        .expect("應有 total 列");

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
        if key == "total" {
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
        .find(|r| r["key"] == "margin_rate")
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

    let margin = fields.iter().find(|f| f["key"] == "margin_rate").unwrap();
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

// ── 預覽 API 的 instance_id（模擬簽核的前置）──────────────

#[tokio::test]
async fn preview_without_instance_id_keeps_empty_data() {
    // 回歸：設計器預覽不傳 instance_id，行為必須與先前完全相同。
    // data 為空物件時 eval_bool 取不到 quotation.total，
    // visible_when 求值為 false，欄位隱藏。
    let ctx = Ctx::new().await;
    ctx.seed_form_with_data_condition("pv_nodata").await;

    let (status, fields) = ctx
        .get("/forms/pv_nodata/permissions/preview?roles=designer")
        .await;
    assert_eq!(status, StatusCode::OK, "{fields}");

    let note = fields
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "big_deal_note")
        .unwrap();
    assert_eq!(
        note["permission"], "HIDDEN",
        "沒有業務資料時條件不成立，欄位應隱藏"
    );
}

#[tokio::test]
async fn preview_with_instance_id_uses_real_data() {
    // 模擬簽核的核心：帶真實單據時條件式要用該單據的值求值。
    // 不修這個的話，凡是 visible_when 依賴欄位值的欄位在模擬畫面全部判錯，
    // 而沙箱的整個價值就是「看到的畫面與那個人會看到的一樣」。
    let ctx = Ctx::new().await;
    ctx.seed_form_with_data_condition("pv_withdata").await;

    let instance_id = ctx
        .seed_instance(json!({ "quotation": { "total": 2_000_000 } }))
        .await;

    let (status, fields) = ctx
        .get(&format!(
            "/forms/pv_withdata/permissions/preview?roles=designer&instance_id={instance_id}"
        ))
        .await;
    assert_eq!(status, StatusCode::OK, "{fields}");

    let note = fields
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "big_deal_note")
        .unwrap();
    assert_ne!(
        note["permission"], "HIDDEN",
        "總計 200 萬 > 100 萬，條件成立，欄位不該隱藏"
    );
}

#[tokio::test]
async fn preview_with_instance_below_threshold_hides_field() {
    // 同一張表單、同一個角色，只因單據金額不同而顯示不同——
    // 這是條件式權限真的吃到資料的證據。
    let ctx = Ctx::new().await;
    ctx.seed_form_with_data_condition("pv_small").await;

    let instance_id = ctx
        .seed_instance(json!({ "quotation": { "total": 500 } }))
        .await;

    let (_, fields) = ctx
        .get(&format!(
            "/forms/pv_small/permissions/preview?roles=designer&instance_id={instance_id}"
        ))
        .await;

    let note = fields
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["key"] == "big_deal_note")
        .unwrap();
    assert_eq!(
        note["permission"], "HIDDEN",
        "總計 500 未達門檻，欄位應隱藏"
    );
}

#[tokio::test]
async fn preview_with_unknown_instance_id_returns_404() {
    // 查不到就要回 404，不可默默退回空物件——
    // 默默退回會讓模擬畫面判錯而無人察覺。
    // 他租戶的 id 也落在這裡：RLS 讓查詢查不到。
    let ctx = Ctx::new().await;
    ctx.seed_form_with_data_condition("pv_404").await;

    let (status, _) = ctx
        .get(&format!(
            "/forms/pv_404/permissions/preview?roles=designer&instance_id={}",
            Uuid::new_v4()
        ))
        .await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "不存在的 instance_id 應回 404 而非空物件"
    );
}
