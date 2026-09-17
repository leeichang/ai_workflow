//! 權限矩陣寫回整合測試
//!
//! 驗證重點：修改確實落到 form content、計算欄位被略過、
//! 既有規則不被洗掉、租戶隔離。

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
    designer: String,
    viewer: String,
}

impl Ctx {
    async fn new() -> Self {
        let db = Db::connect(&database_url()).await.expect("連線失敗");
        let admin = sqlx::postgres::PgPool::connect(&admin_url()).await.unwrap();

        let tenant_id = Uuid::new_v4();
        sqlx::query("insert into tenant (id, code, name) values ($1, $2, $3)")
            .bind(tenant_id)
            .bind(format!("pw-{}", &tenant_id.to_string()[..8]))
            .bind("權限寫入測試")
            .execute(&admin)
            .await
            .unwrap();

        let jwt = JwtKeys::new(JWT_SECRET, 1);
        let mut tx = db.tenant_tx(tenant_id).await.unwrap();

        let mut tokens = Vec::new();
        for (code, name, email) in [
            ("designer", "設計者", "d@pw.local"),
            ("viewer", "檢視者", "v@pw.local"),
        ] {
            sqlx::query("insert into role (tenant_id, code, name) values ($1, $2, $3)")
                .bind(tenant_id)
                .bind(code)
                .bind(name)
                .execute(tx.executor())
                .await
                .unwrap();

            let uid: Uuid = sqlx::query_scalar(
                "insert into app_user (tenant_id, email, name, password_hash)
                 values ($1, $2, $3, 'x') returning id",
            )
            .bind(tenant_id)
            .bind(email)
            .bind(name)
            .fetch_one(tx.executor())
            .await
            .unwrap();

            tokens.push(
                jwt.issue(uid, tenant_id, name.into(), vec![code.into()])
                    .unwrap(),
            );
        }

        tx.commit().await.unwrap();

        Self {
            state: AppState {
                db,
                jwt,
                temporal: None,
                internal_token: Some("test-token".into()),
                // 測試不寄信
                mailer: http_public::Mailer::disabled(),
            },
            designer: tokens[0].clone(),
            viewer: tokens[1].clone(),
        }
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"));

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
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn seed(&self, key: &str) {
        let content = json!({
            "form_key": key,
            "version": 1,
            "business_object": "quotation",
            "fields": [
                {
                    "key": "customer_name",
                    "ui": { "component": "input", "label": "客戶名稱" },
                    "data": { "path": "quotation.customer_name", "type": "string" }
                },
                {
                    "key": "discount_rate",
                    "ui": { "component": "number", "label": "折扣率" },
                    "data": { "path": "quotation.discount_rate", "type": "decimal" },
                    "workflow": { "readonly_when": "node.id != 'start'" }
                },
                {
                    "key": "total",
                    "ui": { "component": "display", "label": "總計" },
                    "data": {
                        "path": "quotation.total",
                        "type": "decimal",
                        "computed": "sum(lines.amount)"
                    }
                }
            ]
        });

        self.send(
            "POST",
            "/forms",
            &self.designer,
            Some(json!({
                "form_key": key,
                "business_object": "quotation",
                "name": "權限寫入測試表單",
                "content": content,
            })),
        )
        .await;
    }

    /// 取草稿內容，驗證修改是否落地
    async fn draft_content(&self, key: &str) -> Value {
        let (_, detail) = self
            .send("GET", &format!("/forms/{key}"), &self.designer, None)
            .await;
        detail["draft"]["content"].clone()
    }
}

fn change(field: &str, role: &str, permission: &str) -> Value {
    json!({
        "field": field,
        "node_id": "start",
        "role": role,
        "permission": permission,
    })
}

// ── 基本寫入 ────────────────────────────────────────────

#[tokio::test]
async fn editable_writes_both_role_lists() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_editable").await;

    let (status, res) = ctx
        .send(
            "PUT",
            "/forms/pw_editable/permissions",
            &ctx.designer,
            Some(json!({ "changes": [change("customer_name", "requester", "EDITABLE")] })),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{res}");
    assert_eq!(res["applied"], 1);

    let content = ctx.draft_content("pw_editable").await;
    let wf = &content["fields"][0]["workflow"];
    assert_eq!(wf["readable_roles"], json!(["requester"]));
    assert_eq!(wf["editable_roles"], json!(["requester"]));
}

#[tokio::test]
async fn readonly_removes_only_editable() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_readonly").await;

    ctx.send(
        "PUT",
        "/forms/pw_readonly/permissions",
        &ctx.designer,
        Some(json!({
            "changes": [
                change("customer_name", "requester", "EDITABLE"),
                change("customer_name", "requester", "READONLY"),
            ]
        })),
    )
    .await;

    let content = ctx.draft_content("pw_readonly").await;
    let wf = &content["fields"][0]["workflow"];
    assert_eq!(wf["readable_roles"], json!(["requester"]));
    // 空陣列必須保留。鍵不存在代表「不限制」，[] 代表「全部拒絕」，
    // 兩者語意相反，不可混用。
    assert_eq!(wf["editable_roles"], json!([]));
}

#[tokio::test]
async fn hidden_empties_both_role_lists() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_hidden").await;

    ctx.send(
        "PUT",
        "/forms/pw_hidden/permissions",
        &ctx.designer,
        Some(json!({
            "changes": [
                change("customer_name", "requester", "EDITABLE"),
                change("customer_name", "requester", "HIDDEN"),
            ]
        })),
    )
    .await;

    let content = ctx.draft_content("pw_hidden").await;
    let wf = &content["fields"][0]["workflow"];
    assert_eq!(wf["readable_roles"], json!([]), "隱藏代表無人可讀");
    assert_eq!(wf["editable_roles"], json!([]));
}

// ── 保護機制 ────────────────────────────────────────────

#[tokio::test]
async fn computed_field_is_skipped_with_reason() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_computed").await;

    let (status, res) = ctx
        .send(
            "PUT",
            "/forms/pw_computed/permissions",
            &ctx.designer,
            Some(json!({ "changes": [change("total", "requester", "EDITABLE")] })),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(res["applied"], 0);
    assert_eq!(res["skipped"][0]["field"], "total");
    assert!(
        res["skipped"][0]["reason"].as_str().unwrap().contains("計算"),
        "應說明為何被略過：{res}"
    );

    let content = ctx.draft_content("pw_computed").await;
    assert!(content["fields"][2].get("workflow").is_none(), "不該被修改");
}

#[tokio::test]
async fn existing_conditional_rules_are_preserved() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_preserve").await;

    ctx.send(
        "PUT",
        "/forms/pw_preserve/permissions",
        &ctx.designer,
        Some(json!({ "changes": [change("discount_rate", "approver", "READONLY")] })),
    )
    .await;

    let content = ctx.draft_content("pw_preserve").await;
    let wf = &content["fields"][1]["workflow"];

    assert_eq!(
        wf["readonly_when"], "node.id != 'start'",
        "既有條件式規則不該被權限修改洗掉"
    );
    assert_eq!(wf["readable_roles"], json!(["approver"]));
}

#[tokio::test]
async fn field_structure_is_not_touched() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_structure").await;

    let before = ctx.draft_content("pw_structure").await;

    ctx.send(
        "PUT",
        "/forms/pw_structure/permissions",
        &ctx.designer,
        Some(json!({ "changes": [change("customer_name", "requester", "READONLY")] })),
    )
    .await;

    let after = ctx.draft_content("pw_structure").await;

    // 矩陣只該改權限，不該動欄位結構
    assert_eq!(after["fields"][0]["ui"], before["fields"][0]["ui"]);
    assert_eq!(after["fields"][0]["data"], before["fields"][0]["data"]);
    assert_eq!(
        after["fields"].as_array().unwrap().len(),
        before["fields"].as_array().unwrap().len()
    );
}

#[tokio::test]
async fn unknown_field_skipped_not_error() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_unknown").await;

    let (status, res) = ctx
        .send(
            "PUT",
            "/forms/pw_unknown/permissions",
            &ctx.designer,
            Some(json!({
                "changes": [
                    change("customer_name", "requester", "READONLY"),
                    change("deleted_field", "requester", "EDITABLE"),
                ]
            })),
        )
        .await;

    // 矩陣可能與已變更的表單不同步，略過比整批失敗合理
    assert_eq!(status, StatusCode::OK);
    assert_eq!(res["applied"], 1, "有效的修改仍應套用");
    assert_eq!(res["skipped"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn empty_changes_is_noop() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_empty").await;

    let (status, res) = ctx
        .send(
            "PUT",
            "/forms/pw_empty/permissions",
            &ctx.designer,
            Some(json!({ "changes": [] })),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(res["applied"], 0);
}

// ── 權限與隔離 ──────────────────────────────────────────

#[tokio::test]
async fn viewer_cannot_save_matrix() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_forbidden").await;

    let (status, err) = ctx
        .send(
            "PUT",
            "/forms/pw_forbidden/permissions",
            &ctx.viewer,
            Some(json!({ "changes": [change("customer_name", "requester", "EDITABLE")] })),
        )
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(err["code"], "FORBIDDEN");
}

#[tokio::test]
async fn cannot_write_other_tenant_form() {
    let a = Ctx::new().await;
    let b = Ctx::new().await;

    a.seed("pw_secret").await;

    let (status, _) = b
        .send(
            "PUT",
            "/forms/pw_secret/permissions",
            &b.designer,
            Some(json!({ "changes": [change("customer_name", "requester", "HIDDEN")] })),
        )
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "跨租戶應回 404");
}

#[tokio::test]
async fn save_writes_audit_event() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_audit").await;

    ctx.send(
        "PUT",
        "/forms/pw_audit/permissions",
        &ctx.designer,
        Some(json!({ "changes": [change("customer_name", "requester", "READONLY")] })),
    )
    .await;

    let tenant_id = ctx
        .state
        .jwt
        .verify(&ctx.designer)
        .unwrap()
        .tenant_id;

    let mut tx = ctx.state.db.tenant_tx(tenant_id).await.unwrap();
    let actions: Vec<String> = sqlx::query_scalar("select action from audit_event")
        .fetch_all(tx.executor())
        .await
        .unwrap();

    assert!(
        actions.contains(&"form.permissions.save".to_string()),
        "權限修改應寫稽核：{actions:?}"
    );
}

// ── 矩陣與儲存往返 ──────────────────────────────────────

#[tokio::test]
async fn saved_permission_reflects_in_matrix() {
    let ctx = Ctx::new().await;
    ctx.seed("pw_roundtrip").await;

    // 儲存前：requester 對客戶名稱可編輯（無限制）
    let (_, before) = ctx
        .send(
            "GET",
            "/forms/pw_roundtrip/permissions?mode=by_role&role=requester",
            &ctx.designer,
            None,
        )
        .await;
    let cell = &before["rows"][0]["cells"]["start"];
    assert_eq!(cell["permission"], "EDITABLE");

    // 設為唯讀
    ctx.send(
        "PUT",
        "/forms/pw_roundtrip/permissions",
        &ctx.designer,
        Some(json!({ "changes": [change("customer_name", "requester", "READONLY")] })),
    )
    .await;

    // 儲存後矩陣應反映變更
    let (_, after) = ctx
        .send(
            "GET",
            "/forms/pw_roundtrip/permissions?mode=by_role&role=requester",
            &ctx.designer,
            None,
        )
        .await;
    let cell = &after["rows"][0]["cells"]["start"];
    assert_eq!(cell["permission"], "READONLY", "儲存後應變唯讀");
}
