//! 組織管理 API 整合測試
//!
//! 對真實 PostgreSQL 執行。
//!
//! 重點不在 CRUD 本身，而在三件會出事的事：
//!
//!   1. **欄位級 ownership**（需求 §3）：編輯過的欄位要進
//!      `platform_managed_fields`，否則日後同步會把管理員補的資料沖掉
//!   2. **成環防護**：部門樹與主管鏈成環會讓前端無限展開、
//!      多階簽核無限迴圈
//!   3. **權限**：改主管等於改簽核路徑，designer 不該動得了
//!
//! 前置：
//!   docker compose -f deploy/docker-compose.yml up -d postgres
//!   套用 server/migrations/*.sql

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
    tenant_id: Uuid,
    admin_token: String,
    /// 沒有 admin 角色。用來驗證組織編輯擋得住
    designer_token: String,
    sales_dept: Uuid,
    manager: Uuid,
    staff: Uuid,
}

impl Ctx {
    async fn new() -> Self {
        let db = Db::connect(&database_url())
            .await
            .expect("連線失敗。請確認容器已啟動且 migration 已套用");

        let admin_pool = sqlx::postgres::PgPool::connect(&admin_url())
            .await
            .expect("管理連線失敗");

        let tenant_id = Uuid::new_v4();
        let code = format!("orga-{}", &tenant_id.to_string()[..8]);

        sqlx::query("insert into tenant (id, code, name) values ($1, $2, $3)")
            .bind(tenant_id)
            .bind(&code)
            .bind("組織管理測試租戶")
            .execute(&admin_pool)
            .await
            .expect("建立租戶失敗");

        let jwt = JwtKeys::new(JWT_SECRET, 1);
        let mut tx = db.tenant_tx(tenant_id).await.expect("開啟交易失敗");

        for (c, n) in [("admin", "管理員"), ("designer", "設計者")] {
            sqlx::query("insert into role (tenant_id, code, name) values ($1, $2, $3)")
                .bind(tenant_id)
                .bind(c)
                .bind(n)
                .execute(tx.executor())
                .await
                .expect("建立角色失敗");
        }

        let sales_dept: Uuid = sqlx::query_scalar(
            "insert into department (tenant_id, code, name) values ($1, 'SALES', '業務部')
             returning id",
        )
        .bind(tenant_id)
        .fetch_one(tx.executor())
        .await
        .expect("建立部門失敗");

        let mut make_user = |email: &'static str, name: &'static str| {
            let tenant_id = tenant_id;
            let dept = sales_dept;
            async move {
                sqlx::query_scalar::<_, Uuid>(
                    "insert into app_user (tenant_id, email, name, password_hash, department_id)
                     values ($1, $2, $3, 'x', $4) returning id",
                )
                .bind(tenant_id)
                .bind(email)
                .bind(name)
                .bind(dept)
            }
        };

        let manager: Uuid = make_user("mgr@test.local", "陳經理")
            .await
            .fetch_one(tx.executor())
            .await
            .expect("建立主管失敗");
        let staff: Uuid = make_user("staff@test.local", "林小明")
            .await
            .fetch_one(tx.executor())
            .await
            .expect("建立員工失敗");

        sqlx::query("update app_user set manager_id = $2 where id = $1")
            .bind(staff)
            .bind(manager)
            .execute(tx.executor())
            .await
            .expect("設定主管失敗");

        tx.commit().await.expect("提交失敗");

        let admin_token = jwt
            .issue(manager, tenant_id, "陳經理".into(), vec!["admin".into()])
            .expect("簽發 token 失敗");
        let designer_token = jwt
            .issue(staff, tenant_id, "林小明".into(), vec!["designer".into()])
            .expect("簽發 token 失敗");

        Self {
            state: AppState {
                db,
                jwt,
                temporal: None,
                internal_token: Some("test-token".into()),
                mailer: http_public::Mailer::disabled(),
            },
            tenant_id,
            admin_token,
            designer_token,
            sales_dept,
            manager,
            staff,
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
            None => {
                req = req.header(header::CONTENT_TYPE, "application/json");
                req.body(Body::empty()).unwrap()
            }
        };

        let resp = http_public::router(self.state.clone())
            .oneshot(req)
            .await
            .expect("請求失敗");

        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .expect("讀取回應失敗");
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// 直接從資料庫讀 platform_managed_fields，不經 API
    async fn managed_fields(&self, table: &str, id: Uuid) -> Vec<String> {
        let mut tx = self
            .state
            .db
            .tenant_tx(self.tenant_id)
            .await
            .expect("開啟交易失敗");

        let sql = format!("select platform_managed_fields from {table} where id = $1");
        sqlx::query_scalar(&sql)
            .bind(id)
            .fetch_one(tx.executor())
            .await
            .expect("查詢失敗")
    }
}

// ── 讀取 ────────────────────────────────────────────────

#[tokio::test]
async fn lists_departments_with_member_count() {
    let ctx = Ctx::new().await;
    let (status, body) = ctx
        .send("GET", "/org/departments", &ctx.admin_token, None)
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body.as_array().expect("不是陣列");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["code"], "SALES");
    // 兩個人都在業務部。空部門要在樹上看得出來，所以計數不能省
    assert_eq!(rows[0]["member_count"], 2);
}

#[tokio::test]
async fn lists_employees_with_department_and_manager_names() {
    let ctx = Ctx::new().await;
    let (status, body) = ctx
        .send("GET", "/org/employees", &ctx.admin_token, None)
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body.as_array().expect("不是陣列");
    assert_eq!(rows.len(), 2);

    // 前端要顯示「誰在管」，名字要一起帶回來，不該為此再查一次
    let staff = rows
        .iter()
        .find(|r| r["name"] == "林小明")
        .expect("找不到林小明");
    assert_eq!(staff["manager_name"], "陳經理");
    assert_eq!(staff["department_name"], "業務部");
}

/// 預設只列在職者
#[tokio::test]
async fn excludes_left_employees_by_default() {
    let ctx = Ctx::new().await;

    let (status, _) = ctx
        .send(
            "PATCH",
            &format!("/org/employees/{}", ctx.staff),
            &ctx.admin_token,
            Some(json!({ "fields": ["left_at"], "left_at": "2020-01-01" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (_, body) = ctx
        .send("GET", "/org/employees", &ctx.admin_token, None)
        .await;
    assert_eq!(body.as_array().unwrap().len(), 1, "{body}");

    // 要看離職的人必須明講
    let (_, body) = ctx
        .send(
            "GET",
            "/org/employees?include_inactive=true",
            &ctx.admin_token,
            None,
        )
        .await;
    assert_eq!(body.as_array().unwrap().len(), 2, "{body}");
}

#[tokio::test]
async fn filters_employees_by_keyword() {
    let ctx = Ctx::new().await;
    let (_, body) = ctx
        .send("GET", "/org/employees?q=小明", &ctx.admin_token, None)
        .await;

    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{body}");
    assert_eq!(rows[0]["name"], "林小明");
}

// ── 欄位級 ownership（需求 §3）──────────────────────────

/// 編輯過的欄位要進 platform_managed_fields
///
/// 這是整個需求 07 裡「唯一非做不可、且沒有別的做法」的機制。
/// 沒有它，管理員補的資料會在下一次同步被沖掉。
#[tokio::test]
async fn records_edited_fields_as_platform_managed() {
    let ctx = Ctx::new().await;

    assert!(
        ctx.managed_fields("app_user", ctx.staff).await.is_empty(),
        "編輯前不該有任何鎖定欄位"
    );

    let (status, body) = ctx
        .send(
            "PATCH",
            &format!("/org/employees/{}", ctx.staff),
            &ctx.admin_token,
            Some(json!({
                "fields": ["job_title", "phone"],
                "job_title": "資深工程師",
                "phone": "0912345678",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let mut fields = ctx.managed_fields("app_user", ctx.staff).await;
    fields.sort();
    assert_eq!(fields, vec!["job_title", "phone"]);
}

/// 同一欄位改兩次不該在陣列裡出現兩筆
#[tokio::test]
async fn does_not_duplicate_managed_fields() {
    let ctx = Ctx::new().await;

    for title in ["工程師", "資深工程師"] {
        ctx.send(
            "PATCH",
            &format!("/org/employees/{}", ctx.staff),
            &ctx.admin_token,
            Some(json!({ "fields": ["job_title"], "job_title": title })),
        )
        .await;
    }

    assert_eq!(
        ctx.managed_fields("app_user", ctx.staff).await,
        vec!["job_title"]
    );
}

/// 解除鎖定後該欄位改回跟隨來源系統（需求 §3 規則 3）
#[tokio::test]
async fn unlocks_fields_back_to_source() {
    let ctx = Ctx::new().await;

    ctx.send(
        "PATCH",
        &format!("/org/employees/{}", ctx.staff),
        &ctx.admin_token,
        Some(json!({
            "fields": ["job_title", "phone"],
            "job_title": "工程師",
            "phone": "0912345678",
        })),
    )
    .await;

    let (status, body) = ctx
        .send(
            "POST",
            &format!("/org/employees/{}/unlock-fields", ctx.staff),
            &ctx.admin_token,
            Some(json!({ "fields": ["job_title"] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // 只解 job_title，phone 仍鎖著
    assert_eq!(
        ctx.managed_fields("app_user", ctx.staff).await,
        vec!["phone"]
    );
}

/// 解除一個沒鎖的欄位是無害的
///
/// 前端可能在使用者連點兩次時送兩遍，不該因此報錯
#[tokio::test]
async fn unlocking_unlocked_field_is_harmless() {
    let ctx = Ctx::new().await;

    let (status, _) = ctx
        .send(
            "POST",
            &format!("/org/employees/{}/unlock-fields", ctx.staff),
            &ctx.admin_token,
            Some(json!({ "fields": ["job_title"] })),
        )
        .await;

    assert_eq!(status, StatusCode::OK);
    assert!(ctx.managed_fields("app_user", ctx.staff).await.is_empty());
}

// ── 成環防護 ────────────────────────────────────────────

/// 主管不能是自己
#[tokio::test]
async fn rejects_self_as_manager() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "PATCH",
            &format!("/org/employees/{}", ctx.staff),
            &ctx.admin_token,
            Some(json!({ "fields": ["manager_id"], "manager_id": ctx.staff })),
        )
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("不能是自己"),
        "{body}"
    );
}

/// 不能指定自己的下屬為主管
///
/// 林小明的主管是陳經理。把陳經理的主管設成林小明就成環，
/// 多階簽核會無限迴圈
#[tokio::test]
async fn rejects_manager_cycle() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "PATCH",
            &format!("/org/employees/{}", ctx.manager),
            &ctx.admin_token,
            Some(json!({ "fields": ["manager_id"], "manager_id": ctx.staff })),
        )
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body["message"].as_str().unwrap().contains("循環"), "{body}");

    // 擋下之後資料不該被改到一半
    let (_, employees) = ctx
        .send("GET", "/org/employees", &ctx.admin_token, None)
        .await;
    let mgr = employees
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "陳經理")
        .unwrap();
    assert!(mgr["manager_id"].is_null(), "{employees}");
}

/// 部門不能移到自己的下層底下
#[tokio::test]
async fn rejects_department_cycle() {
    let ctx = Ctx::new().await;

    let (_, created) = ctx
        .send(
            "POST",
            "/org/departments",
            &ctx.admin_token,
            Some(json!({
                "code": "SALES1",
                "name": "業務一課",
                "parent_id": ctx.sales_dept,
            })),
        )
        .await;
    let child = created["id"].as_str().expect("沒有回傳 id");

    // 把業務部移到業務一課底下
    let (status, body) = ctx
        .send(
            "PATCH",
            &format!("/org/departments/{}", ctx.sales_dept),
            &ctx.admin_token,
            Some(json!({ "fields": ["parent_id"], "parent_id": child })),
        )
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("下層部門"),
        "{body}"
    );
}

// ── 刪除部門 ────────────────────────────────────────────

/// 空部門刪得掉
///
/// 使用者手誤建了一個部門應該收得回去，否則樹上永遠留著垃圾
#[tokio::test]
async fn deletes_empty_department() {
    let ctx = Ctx::new().await;

    let (_, created) = ctx
        .send(
            "POST",
            "/org/departments",
            &ctx.admin_token,
            Some(json!({ "code": "TEMP", "name": "暫時的部門" })),
        )
        .await;
    let id = created["id"].as_str().expect("沒有回傳 id");

    let (status, body) = ctx
        .send(
            "DELETE",
            &format!("/org/departments/{id}"),
            &ctx.admin_token,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, list) = ctx
        .send("GET", "/org/departments", &ctx.admin_token, None)
        .await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
}

/// 有成員的部門刪不掉
///
/// 兩個 FK 都是 on delete set null。若放行，那些人的 department_id
/// 會被**靜默清空**，依部門解析的簽核人全部失效且沒有錯誤訊息
#[tokio::test]
async fn refuses_to_delete_department_with_members() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "DELETE",
            &format!("/org/departments/{}", ctx.sales_dept),
            &ctx.admin_token,
            None,
        )
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("2 位成員"),
        "訊息要說有幾個人，使用者才知道要處理多少：{body}"
    );

    // 擋下後部門與成員都還在
    let (_, employees) = ctx
        .send("GET", "/org/employees", &ctx.admin_token, None)
        .await;
    assert_eq!(employees.as_array().unwrap().len(), 2, "{employees}");
}

/// 有子部門的部門刪不掉
#[tokio::test]
async fn refuses_to_delete_department_with_children() {
    let ctx = Ctx::new().await;

    // 建一個空的父部門與它的子部門
    let (_, parent) = ctx
        .send(
            "POST",
            "/org/departments",
            &ctx.admin_token,
            Some(json!({ "code": "PARENT", "name": "上層" })),
        )
        .await;
    let parent_id = parent["id"].as_str().unwrap();

    ctx.send(
        "POST",
        "/org/departments",
        &ctx.admin_token,
        Some(json!({ "code": "CHILD", "name": "下層", "parent_id": parent_id })),
    )
    .await;

    let (status, body) = ctx
        .send(
            "DELETE",
            &format!("/org/departments/{parent_id}"),
            &ctx.admin_token,
            None,
        )
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("子部門"),
        "{body}"
    );
}

#[tokio::test]
async fn deleting_missing_department_returns_not_found() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "DELETE",
            &format!("/org/departments/{}", Uuid::new_v4()),
            &ctx.admin_token,
            None,
        )
        .await;

    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// 稽核要留得下名字
///
/// 部門已經刪掉了，光有 uuid 事後查不出刪的是誰
#[tokio::test]
async fn audits_deleted_department_name() {
    let ctx = Ctx::new().await;

    let (_, created) = ctx
        .send(
            "POST",
            "/org/departments",
            &ctx.admin_token,
            Some(json!({ "code": "TEMP", "name": "暫時的部門" })),
        )
        .await;
    let id = created["id"].as_str().unwrap();

    ctx.send(
        "DELETE",
        &format!("/org/departments/{id}"),
        &ctx.admin_token,
        None,
    )
    .await;

    let mut tx = ctx
        .state
        .db
        .tenant_tx(ctx.tenant_id)
        .await
        .expect("開啟交易失敗");

    let payload: Value = sqlx::query_scalar(
        "select payload from audit_event
         where action = 'org.department.delete' and target_id = $1",
    )
    .bind(id)
    .fetch_one(tx.executor())
    .await
    .expect("查不到稽核紀錄");

    assert_eq!(payload["name"], "暫時的部門");
}

// ── 輸入驗證 ────────────────────────────────────────────

/// 不在白名單的欄位要擋下
///
/// email 是登入帳號，改它等於換人。欄位名還會進
/// platform_managed_fields 存進資料庫，等同 API 契約
#[tokio::test]
async fn rejects_non_editable_field() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "PATCH",
            &format!("/org/employees/{}", ctx.staff),
            &ctx.admin_token,
            Some(json!({ "fields": ["email"], "name": "改名" })),
        )
        .await;

    // ValidationFailed 映射到 422，與既有 API 一致
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("email"),
        "訊息要指出是哪個欄位：{body}"
    );

    // 擋下時不該有任何欄位被寫入
    assert!(ctx.managed_fields("app_user", ctx.staff).await.is_empty());
}

/// 不在 fields 裡的值一律不動，即使物件帶了它
#[tokio::test]
async fn ignores_values_not_listed_in_fields() {
    let ctx = Ctx::new().await;

    ctx.send(
        "PATCH",
        &format!("/org/employees/{}", ctx.staff),
        &ctx.admin_token,
        Some(json!({
            "fields": ["job_title"],
            "job_title": "工程師",
            "phone": "0912345678",
        })),
    )
    .await;

    let (_, body) = ctx
        .send("GET", "/org/employees?q=小明", &ctx.admin_token, None)
        .await;
    let row = &body.as_array().unwrap()[0];

    assert_eq!(row["job_title"], "工程師");
    assert!(row["phone"].is_null(), "phone 不在 fields 裡，不該被寫入");
}

#[tokio::test]
async fn rejects_blank_department_code() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "POST",
            "/org/departments",
            &ctx.admin_token,
            Some(json!({ "code": "  ", "name": "空白代碼" })),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

/// 部門代碼重複要回可讀的訊息，不是原始 SQL 錯誤
#[tokio::test]
async fn reports_duplicate_department_code() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "POST",
            "/org/departments",
            &ctx.admin_token,
            Some(json!({ "code": "SALES", "name": "重複的業務部" })),
        )
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

// ── 權限 ────────────────────────────────────────────────

/// designer 不能改組織
///
/// 改主管等於改簽核路徑。designer 管的是表單與流程定義，不是人事
#[tokio::test]
async fn designer_cannot_edit_organization() {
    let ctx = Ctx::new().await;

    for (method, path, body) in [
        (
            "PATCH",
            format!("/org/employees/{}", ctx.staff),
            json!({ "fields": ["job_title"], "job_title": "偷改" }),
        ),
        (
            "PATCH",
            format!("/org/departments/{}", ctx.sales_dept),
            json!({ "fields": ["name"], "name": "偷改" }),
        ),
        (
            "POST",
            "/org/departments".to_string(),
            json!({ "code": "X", "name": "偷建" }),
        ),
        (
            "POST",
            format!("/org/employees/{}/unlock-fields", ctx.staff),
            json!({ "fields": ["job_title"] }),
        ),
        (
            "DELETE",
            format!("/org/departments/{}", ctx.sales_dept),
            json!({}),
        ),
    ] {
        let (status, resp) = ctx
            .send(method, &path, &ctx.designer_token, Some(body))
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}：{resp}");
    }
}

/// 但 designer 看得到組織
///
/// 下拉選單本來就查得到人與部門，讀取沒有必要限制
#[tokio::test]
async fn designer_can_read_organization() {
    let ctx = Ctx::new().await;

    for path in ["/org/departments", "/org/employees", "/org/health"] {
        let (status, body) = ctx.send("GET", path, &ctx.designer_token, None).await;
        assert_eq!(status, StatusCode::OK, "{path}：{body}");
    }
}

#[tokio::test]
async fn requires_authentication() {
    let ctx = Ctx::new().await;

    let req = Request::builder()
        .method("GET")
        .uri("/org/departments")
        .body(Body::empty())
        .unwrap();

    let resp = http_public::router(ctx.state.clone())
        .oneshot(req)
        .await
        .expect("請求失敗");

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// 只看得到自己租戶的組織
#[tokio::test]
async fn does_not_leak_across_tenants() {
    let ctx_a = Ctx::new().await;
    let ctx_b = Ctx::new().await;

    let (_, body) = ctx_a
        .send("GET", "/org/employees", &ctx_a.admin_token, None)
        .await;

    let ids: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();

    assert!(!ids.contains(&ctx_b.staff.to_string().as_str()), "{body}");
    assert_eq!(ids.len(), 2, "{body}");
}

/// 稽核記錄改了哪些欄位，但不記錄改成什麼值
///
/// 組織資料含個資，稽核紀錄不該變成另一份個資副本
#[tokio::test]
async fn audits_field_names_not_values() {
    let ctx = Ctx::new().await;

    ctx.send(
        "PATCH",
        &format!("/org/employees/{}", ctx.staff),
        &ctx.admin_token,
        Some(json!({ "fields": ["phone"], "phone": "0912345678" })),
    )
    .await;

    let mut tx = ctx
        .state
        .db
        .tenant_tx(ctx.tenant_id)
        .await
        .expect("開啟交易失敗");

    let payload: Value = sqlx::query_scalar(
        "select payload from audit_event
         where action = 'org.employee.update' and target_id = $1",
    )
    .bind(ctx.staff.to_string())
    .fetch_one(tx.executor())
    .await
    .expect("查不到稽核紀錄");

    assert_eq!(payload["fields"][0], "phone");
    // 電話號碼不該出現在稽核紀錄裡
    assert!(
        !payload.to_string().contains("0912345678"),
        "稽核紀錄不該含個資：{payload}"
    );
}
