//! 組織資料同步整合測試
//!
//! 對真實 PostgreSQL 執行。
//!
//! 需求 07 §7 說完整性閘是「最危險的部分」，所以測試的重點不是
//! 「能不能匯入」，而是**該擋的時候有沒有擋住**：
//!
//!   - 來源筆數驟減時整批中止，且不寫入任何變更
//!   - platform_managed_fields 的欄位不被覆蓋
//!   - 來源消失的人不被硬刪
//!   - preview 真的不寫入
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
    designer_token: String,
    source_id: Uuid,
}

impl Ctx {
    /// 建立租戶、一個部門、一條 FILE 連線與一個員工來源
    async fn new() -> Self {
        let db = Db::connect(&database_url())
            .await
            .expect("連線失敗。請確認容器已啟動且 migration 已套用");

        let admin_pool = sqlx::postgres::PgPool::connect(&admin_url())
            .await
            .expect("管理連線失敗");

        let tenant_id = Uuid::new_v4();
        let code = format!("intg-{}", &tenant_id.to_string()[..8]);

        sqlx::query("insert into tenant (id, code, name) values ($1, $2, $3)")
            .bind(tenant_id)
            .bind(&code)
            .bind("同步測試租戶")
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

        sqlx::query(
            "insert into department (tenant_id, code, name) values ($1, 'SALES', '業務部')",
        )
        .bind(tenant_id)
        .execute(tx.executor())
        .await
        .expect("建立部門失敗");

        let actor_id: Uuid = sqlx::query_scalar(
            "insert into app_user (tenant_id, email, name, password_hash)
             values ($1, 'admin@test.local', '管理員', 'x') returning id",
        )
        .bind(tenant_id)
        .fetch_one(tx.executor())
        .await
        .expect("建立管理員失敗");

        let connection_id: Uuid = sqlx::query_scalar(
            "insert into integration_connection (tenant_id, name, kind)
             values ($1, '人事系統匯出', 'FILE') returning id",
        )
        .bind(tenant_id)
        .fetch_one(tx.executor())
        .await
        .expect("建立連線失敗");

        let source_id: Uuid = sqlx::query_scalar(
            "insert into integration_source
                (tenant_id, connection_id, dataset, name, mapping_definition)
             values ($1, $2, 'employee', '員工主檔', $3) returning id",
        )
        .bind(tenant_id)
        .bind(connection_id)
        .bind(json!({
            "工號": "employee_no",
            "姓名": "name",
            "信箱": "email",
            "部門": "department_code",
            "職稱": "job_title",
            "主管工號": "manager_employee_no",
        }))
        .fetch_one(tx.executor())
        .await
        .expect("建立來源失敗");

        tx.commit().await.expect("提交失敗");

        let admin_token = jwt
            .issue(actor_id, tenant_id, "管理員".into(), vec!["admin".into()])
            .expect("簽發 token 失敗");
        let designer_token = jwt
            .issue(actor_id, tenant_id, "設計者".into(), vec!["designer".into()])
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
            source_id,
        }
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        token: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, "application/json");

        let req = match body {
            Some(b) => req.body(Body::from(b.to_string())).unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };

        let resp = http_public::router(self.state.clone())
            .oneshot(req)
            .await
            .expect("請求失敗");

        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 22)
            .await
            .expect("讀取回應失敗");
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn sync(&self, csv: &str) -> (StatusCode, Value) {
        self.send(
            "POST",
            &format!("/integration/sources/{}/sync", self.source_id),
            &self.admin_token,
            Some(json!({ "content": csv })),
        )
        .await
    }

    async fn preview(&self, csv: &str) -> (StatusCode, Value) {
        self.send(
            "POST",
            &format!("/integration/sources/{}/preview", self.source_id),
            &self.admin_token,
            Some(json!({ "content": csv })),
        )
        .await
    }

    /// 直接從資料庫查員工，不經 API
    async fn employee(&self, employee_no: &str) -> Option<(String, Option<String>, Vec<String>, String)> {
        let mut tx = self
            .state
            .db
            .tenant_tx(self.tenant_id)
            .await
            .expect("開啟交易失敗");

        sqlx::query_as(
            "select name, job_title, platform_managed_fields, sync_status
             from app_user where employee_no = $1",
        )
        .bind(employee_no)
        .fetch_optional(tx.executor())
        .await
        .expect("查詢失敗")
    }

    async fn employee_count(&self) -> i64 {
        let mut tx = self
            .state
            .db
            .tenant_tx(self.tenant_id)
            .await
            .expect("開啟交易失敗");

        sqlx::query_scalar("select count(*) from app_user where employee_no is not null")
            .fetch_one(tx.executor())
            .await
            .expect("查詢失敗")
    }
}

/// 三個人的標準 CSV
const CSV_THREE: &str = "工號,姓名,信箱,部門,職稱,主管工號\n\
E001,陳大明,a@test.local,SALES,經理,\n\
E002,林小華,b@test.local,SALES,工程師,E001\n\
E003,王美玲,c@test.local,SALES,專員,E001\n";

// ── 分析檔案 ────────────────────────────────────────────

#[tokio::test]
async fn analyzes_headers_and_suggests_mapping() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "POST",
            "/integration/analyze",
            &ctx.admin_token,
            Some(json!({ "content": CSV_THREE, "dataset": "employee" })),
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["row_count"], 3);
    assert_eq!(body["headers"][0], "工號");

    let suggestions = body["suggestions"].as_array().unwrap();
    assert_eq!(suggestions[0]["canonical_field"], "employee_no");
    assert_eq!(suggestions[1]["canonical_field"], "name");
    // 「主管工號」不可以被 employee_no 吃掉
    assert_eq!(suggestions[5]["canonical_field"], "manager_employee_no");
}

/// 樣本只回前 5 列
///
/// 回整份等於把全公司個資塞進一個 API 回應
#[tokio::test]
async fn analyze_limits_sample_rows() {
    let ctx = Ctx::new().await;

    let mut csv = String::from("工號,姓名\n");
    for i in 1..=20 {
        csv.push_str(&format!("E{i:03},員工{i}\n"));
    }

    let (_, body) = ctx
        .send(
            "POST",
            "/integration/analyze",
            &ctx.admin_token,
            Some(json!({ "content": csv, "dataset": "employee" })),
        )
        .await;

    assert_eq!(body["row_count"], 20);
    assert_eq!(body["sample_rows"].as_array().unwrap().len(), 5);
}

#[tokio::test]
async fn analyze_rejects_malformed_csv() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "POST",
            "/integration/analyze",
            &ctx.admin_token,
            Some(json!({ "content": "", "dataset": "employee" })),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

// ── 首次匯入 ────────────────────────────────────────────

#[tokio::test]
async fn imports_new_employees() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx.sync(CSV_THREE).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "SUCCESS", "{body}");
    assert_eq!(body["counts"]["inserted_count"], 3);
    assert_eq!(body["counts"]["validated_count"], 3);

    let (name, job_title, managed, sync_status) = ctx.employee("E002").await.unwrap();
    assert_eq!(name, "林小華");
    assert_eq!(job_title.as_deref(), Some("工程師"));
    assert_eq!(sync_status, "SYNCED");
    // 同步寫入的欄位不該進 platform_managed_fields——
    // 那是「平台維護」的標記，同步進來的正好相反
    assert!(managed.is_empty(), "{managed:?}");
}

/// 同步進來的人預設不可登入
///
/// 產線作業員、不用電腦的主管要存在（可能是別人的主管、
/// 可能被指派待辦）但不需要帳號（§2）
#[tokio::test]
async fn imported_employees_cannot_login() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let (can_login, has_password): (bool, bool) = sqlx::query_as(
        "select can_login, password_hash is not null from app_user where employee_no = 'E001'",
    )
    .fetch_one(tx.executor())
    .await
    .unwrap();

    assert!(!can_login);
    assert!(!has_password, "不可登入的人不該有密碼");
}

/// 主管關係要在第二階段回填
///
/// §5.3：一次 upsert 會踩到還不存在的 id。E002 的主管是 E001，
/// 而 E001 在檔案裡排在前面；但 E001 的資料寫入時 E002 還不存在
#[tokio::test]
async fn backfills_manager_relationship() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let manager_name: Option<String> = sqlx::query_scalar(
        "select m.name from app_user u join app_user m on m.id = u.manager_id
         where u.employee_no = 'E002'",
    )
    .fetch_optional(tx.executor())
    .await
    .unwrap();

    assert_eq!(manager_name.as_deref(), Some("陳大明"));
}

/// 主管排在部屬後面也要回填得到
#[tokio::test]
async fn backfills_manager_listed_after_subordinate() {
    let ctx = Ctx::new().await;

    let csv = "工號,姓名,信箱,部門,職稱,主管工號\n\
               E002,林小華,b@test.local,SALES,工程師,E001\n\
               E001,陳大明,a@test.local,SALES,經理,\n";
    ctx.sync(csv).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let manager_name: Option<String> = sqlx::query_scalar(
        "select m.name from app_user u join app_user m on m.id = u.manager_id
         where u.employee_no = 'E002'",
    )
    .fetch_optional(tx.executor())
    .await
    .unwrap();

    assert_eq!(manager_name.as_deref(), Some("陳大明"));
}

#[tokio::test]
async fn resolves_department_by_code() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let dept: Option<String> = sqlx::query_scalar(
        "select d.name from app_user u join department d on d.id = u.department_id
         where u.employee_no = 'E001'",
    )
    .fetch_optional(tx.executor())
    .await
    .unwrap();

    assert_eq!(dept.as_deref(), Some("業務部"));
}

/// 部門代碼查不到要留下問題明細
///
/// 靜默忽略的話那個人會沒有部門，依部門解析的簽核人就找不到
#[tokio::test]
async fn reports_unknown_department_code() {
    let ctx = Ctx::new().await;

    let csv = "工號,姓名,信箱,部門,職稱,主管工號\n\
               E001,陳大明,a@test.local,NOSUCH,經理,\n";
    let (_, body) = ctx.sync(csv).await;

    assert_eq!(body["status"], "PARTIAL_SUCCESS", "{body}");
    let issues = body["issues"].as_array().unwrap();
    assert!(
        issues.iter().any(|i| i["message"]
            .as_str()
            .unwrap()
            .contains("NOSUCH")),
        "{body}"
    );
}

// ── 完整性閘（§7 規則 2，最危險的部分）────────────────

/// 來源筆數驟減時整批中止
///
/// 來源 3 人只回 1 人。若照寫，消失的 2 人會被當成離職，
/// 其中只要有一個是主管，全公司相關流程立刻卡死
#[tokio::test]
async fn aborts_when_source_shrinks() {
    let ctx = Ctx::new().await;

    // 第一次：3 人成功
    let (_, first) = ctx.sync(CSV_THREE).await;
    assert_eq!(first["status"], "SUCCESS");

    // 第二次：只剩 1 人
    let shrunk = "工號,姓名,信箱,部門,職稱,主管工號\n\
                  E001,陳大明,a@test.local,SALES,經理,\n";
    let (status, body) = ctx.sync(shrunk).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "ABORTED_INCOMPLETE", "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("整批中止"),
        "{body}"
    );
}

/// 中止時**不寫入任何變更**
///
/// 這是完整性閘的重點。若已經改了一半才中止，資料會處於
/// 比同步前更糟的狀態
#[tokio::test]
async fn aborted_sync_writes_nothing() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    let before = ctx.employee("E002").await.unwrap();

    let shrunk = "工號,姓名,信箱,部門,職稱,主管工號\n\
                  E001,改過的名字,a@test.local,SALES,新職稱,\n";
    let (_, body) = ctx.sync(shrunk).await;
    assert_eq!(body["status"], "ABORTED_INCOMPLETE");

    // E001 沒有被改
    let (name, job_title, _, _) = ctx.employee("E001").await.unwrap();
    assert_eq!(name, "陳大明", "中止時不該寫入任何變更");
    assert_eq!(job_title.as_deref(), Some("經理"));

    // E002、E003 也沒有被標成 MISSING_IN_SOURCE
    let after = ctx.employee("E002").await.unwrap();
    assert_eq!(after.3, before.3);
    assert_eq!(after.3, "SYNCED");
}

/// 中止不更新比較基準
///
/// 用一次不完整的同步當基準，下一次的完整性閘就失效了
#[tokio::test]
async fn aborted_sync_does_not_update_baseline() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    let shrunk = "工號,姓名,信箱,部門,職稱,主管工號\n\
                  E001,陳大明,a@test.local,SALES,經理,\n";
    ctx.sync(shrunk).await;

    let (_, source) = ctx
        .send(
            "GET",
            &format!("/integration/sources/{}", ctx.source_id),
            &ctx.admin_token,
            None,
        )
        .await;

    // 仍是第一次的 3，不是中止那次的 1
    assert_eq!(source["last_success_count"], 3, "{source}");
}

/// 首次匯入沒有比較基準，閘不生效
#[tokio::test]
async fn first_import_passes_gate() {
    let ctx = Ctx::new().await;

    let single = "工號,姓名,信箱,部門,職稱,主管工號\n\
                  E001,陳大明,a@test.local,SALES,經理,\n";
    let (_, body) = ctx.sync(single).await;

    assert_eq!(body["status"], "SUCCESS", "{body}");
}

// ── 欄位級 ownership（§3）──────────────────────────────

/// platform_managed_fields 的欄位不被同步覆蓋
///
/// 沒有這個機制，管理員在平台補的資料會在下一次同步被沖掉——
/// D-14「ERP 優先、平台補缺」就是空話
#[tokio::test]
async fn does_not_overwrite_platform_managed_fields() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    // 管理員在平台改職稱，該欄位被標成平台維護
    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    sqlx::query(
        "update app_user
         set job_title = '平台改過的職稱',
             platform_managed_fields = array['job_title']
         where employee_no = 'E002'",
    )
    .execute(tx.executor())
    .await
    .unwrap();
    tx.commit().await.unwrap();

    // 同一份檔案再同步一次
    let (_, body) = ctx.sync(CSV_THREE).await;

    let (_, job_title, _, _) = ctx.employee("E002").await.unwrap();
    assert_eq!(
        job_title.as_deref(),
        Some("平台改過的職稱"),
        "平台維護的欄位不該被同步覆蓋"
    );

    assert_eq!(body["counts"]["skipped_by_ownership_count"], 1, "{body}");
}

/// 跳過的欄位要留下問題明細
///
/// 管理員要知道差異在哪，而不只是一個總數
#[tokio::test]
async fn records_skipped_fields_as_issues() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    sqlx::query(
        "update app_user set platform_managed_fields = array['job_title', 'phone']
         where employee_no = 'E002'",
    )
    .execute(tx.executor())
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (_, body) = ctx.sync(CSV_THREE).await;

    let issues = body["issues"].as_array().unwrap();
    let skipped: Vec<&Value> = issues
        .iter()
        .filter(|i| i["kind"] == "SKIPPED_BY_OWNERSHIP")
        .collect();

    // 只有 job_title 被跳過——CSV 沒有電話欄，phone 本來就不會被寫
    assert_eq!(skipped.len(), 1, "{body}");
    assert_eq!(skipped[0]["field"], "job_title");
}

// ── 來源消失（§7 規則 1）──────────────────────────────

/// 來源消失的人不被硬刪
///
/// 硬刪會讓進行中流程的 assignee 變成孤兒
#[tokio::test]
async fn never_hard_deletes_missing_employees() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    // 門檻放寬，讓少一個人也能通過閘
    ctx.send(
        "PATCH",
        &format!("/integration/sources/{}", ctx.source_id),
        &ctx.admin_token,
        Some(json!({ "completeness_threshold": 0.5 })),
    )
    .await;

    let two = "工號,姓名,信箱,部門,職稱,主管工號\n\
               E001,陳大明,a@test.local,SALES,經理,\n\
               E002,林小華,b@test.local,SALES,工程師,E001\n";
    let (_, body) = ctx.sync(two).await;

    assert_eq!(body["counts"]["missing_in_source_count"], 1, "{body}");

    // E003 還在，只是被標記
    let (name, _, _, sync_status) = ctx.employee("E003").await.expect("E003 不該被刪除");
    assert_eq!(name, "王美玲");
    assert_eq!(sync_status, "MISSING_IN_SOURCE");

    // status 維持 ACTIVE——來源缺漏與離職是兩回事
    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let status: String =
        sqlx::query_scalar("select status from app_user where employee_no = 'E003'")
            .fetch_one(tx.executor())
            .await
            .unwrap();
    assert_eq!(status, "ACTIVE", "來源消失不等於離職，不該停用");
}

/// 消失的人進待停用清單並累計次數
#[tokio::test]
async fn records_pending_deactivation() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    ctx.send(
        "PATCH",
        &format!("/integration/sources/{}", ctx.source_id),
        &ctx.admin_token,
        Some(json!({ "completeness_threshold": 0.5 })),
    )
    .await;

    let two = "工號,姓名,信箱,部門,職稱,主管工號\n\
               E001,陳大明,a@test.local,SALES,經理,\n\
               E002,林小華,b@test.local,SALES,工程師,E001\n";

    // 連續兩次消失
    ctx.sync(two).await;
    ctx.sync(two).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let miss_count: i32 = sqlx::query_scalar(
        "select p.miss_count from integration_pending_deactivation p
         join app_user u on u.id = p.user_id
         where u.employee_no = 'E003'",
    )
    .fetch_one(tx.executor())
    .await
    .expect("應該進待停用清單");

    assert_eq!(miss_count, 2, "連續消失要累計次數（§7 規則 3）");
}

/// 消失的人重新出現時要恢復
///
/// 一個人可能只是某次匯出漏掉了。他回來時資料可能一個字都沒變，
/// 若狀態卡在 MISSING_IN_SOURCE，他會一直留在待停用清單上，
/// 管理員最後真的會把他停用掉
#[tokio::test]
async fn returning_employee_is_restored() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    ctx.send(
        "PATCH",
        &format!("/integration/sources/{}", ctx.source_id),
        &ctx.admin_token,
        Some(json!({ "completeness_threshold": 0.5 })),
    )
    .await;

    // E003 消失
    let two = "工號,姓名,信箱,部門,職稱,主管工號\n\
               E001,陳大明,a@test.local,SALES,經理,\n\
               E002,林小華,b@test.local,SALES,工程師,E001\n";
    ctx.sync(two).await;

    let (_, _, _, status) = ctx.employee("E003").await.unwrap();
    assert_eq!(status, "MISSING_IN_SOURCE");

    // 下一次他又回來了，資料完全沒變
    ctx.sync(CSV_THREE).await;

    let (_, _, _, status) = ctx.employee("E003").await.unwrap();
    assert_eq!(status, "SYNCED", "重新出現的人要恢復狀態");

    // 待停用清單也要清掉
    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let pending: i64 = sqlx::query_scalar(
        "select count(*) from integration_pending_deactivation p
         join app_user u on u.id = p.user_id
         where u.employee_no = 'E003'",
    )
    .fetch_one(tx.executor())
    .await
    .unwrap();

    assert_eq!(pending, 0, "回來的人不該留在待停用清單上");
}

/// 平台自建的人不會被標成來源消失
///
/// 他們本來就不在 ERP 裡
#[tokio::test]
async fn platform_only_users_are_not_marked_missing() {
    let ctx = Ctx::new().await;

    // 平台自建一個人
    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    sqlx::query(
        "insert into app_user (tenant_id, email, name, password_hash, employee_no)
         values ($1, 'manual@test.local', '手動建立', 'x', 'M999')",
    )
    .bind(ctx.tenant_id)
    .execute(tx.executor())
    .await
    .unwrap();
    tx.commit().await.unwrap();

    ctx.sync(CSV_THREE).await;

    let (_, _, _, sync_status) = ctx.employee("M999").await.unwrap();
    assert_eq!(sync_status, "PLATFORM_ONLY", "平台自建的人不該被標記");
}

// ── preview（§12.1「preview 是必備」）──────────────────

/// 試跑不寫入任何資料
#[tokio::test]
async fn preview_writes_nothing() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx.preview(CSV_THREE).await;

    assert_eq!(status, StatusCode::OK, "{body}");
    // 計數照算
    assert_eq!(body["counts"]["inserted_count"], 3, "{body}");
    // 但資料庫裡沒有人
    assert_eq!(ctx.employee_count().await, 0, "試跑不該寫入");
}

/// 試跑仍留下一筆 run
///
/// 管理員回頭要看得到「我那天試跑的結果是什麼」
#[tokio::test]
async fn preview_records_a_run() {
    let ctx = Ctx::new().await;
    ctx.preview(CSV_THREE).await;

    let (_, runs) = ctx
        .send("GET", "/integration/sync-runs", &ctx.admin_token, None)
        .await;

    let list = runs.as_array().unwrap();
    assert_eq!(list.len(), 1, "{runs}");
    assert_eq!(list[0]["dry_run"], true);
}

/// 試跑不更新比較基準
#[tokio::test]
async fn preview_does_not_update_baseline() {
    let ctx = Ctx::new().await;
    ctx.preview(CSV_THREE).await;

    let (_, source) = ctx
        .send(
            "GET",
            &format!("/integration/sources/{}", ctx.source_id),
            &ctx.admin_token,
            None,
        )
        .await;

    assert!(source["last_success_count"].is_null(), "{source}");
}

// ── 更新既有資料 ────────────────────────────────────────

#[tokio::test]
async fn updates_existing_employees() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    let changed = "工號,姓名,信箱,部門,職稱,主管工號\n\
                   E001,陳大明,a@test.local,SALES,資深經理,\n\
                   E002,林小華,b@test.local,SALES,工程師,E001\n\
                   E003,王美玲,c@test.local,SALES,專員,E001\n";
    let (_, body) = ctx.sync(changed).await;

    assert_eq!(body["counts"]["inserted_count"], 0, "{body}");
    assert_eq!(body["counts"]["updated_count"], 3, "{body}");

    let (_, job_title, _, _) = ctx.employee("E001").await.unwrap();
    assert_eq!(job_title.as_deref(), Some("資深經理"));
}

/// 同步兩次不會產生兩份
#[tokio::test]
async fn syncing_twice_does_not_duplicate() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;
    ctx.sync(CSV_THREE).await;

    assert_eq!(ctx.employee_count().await, 3);
}

/// 缺少員工編號的列被擋下，但其他列照樣寫入
#[tokio::test]
async fn rejects_rows_without_employee_no() {
    let ctx = Ctx::new().await;

    let csv = "工號,姓名,信箱,部門,職稱,主管工號\n\
               E001,陳大明,a@test.local,SALES,經理,\n\
               ,沒有工號的人,x@test.local,SALES,專員,\n";
    let (_, body) = ctx.sync(csv).await;

    assert_eq!(body["counts"]["rejected_count"], 1, "{body}");
    assert_eq!(body["counts"]["inserted_count"], 1, "{body}");
    assert_eq!(ctx.employee_count().await, 1);
}

// ── 設定驗證 ────────────────────────────────────────────

/// mapping 必須含 employee_no
///
/// 沒有它無法判斷是新人還是既有員工，每次都會新建
#[tokio::test]
async fn source_requires_employee_no_mapping() {
    let ctx = Ctx::new().await;

    let (_, conn) = ctx
        .send(
            "POST",
            "/integration/connections",
            &ctx.admin_token,
            Some(json!({ "name": "另一條連線", "kind": "FILE" })),
        )
        .await;

    let (status, body) = ctx
        .send(
            "POST",
            "/integration/sources",
            &ctx.admin_token,
            Some(json!({
                "connection_id": conn["id"],
                "dataset": "employee",
                "name": "缺了工號",
                "mapping": { "姓名": "name" },
            })),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("員工編號"),
        "{body}"
    );
}

/// 列得出既有來源
///
/// 匯入精靈靠它重用來源。每次都建新的話，完整性閘會被當成
/// 「首次匯入」而永遠不生效——最糟的失效方式，因為它安靜
#[tokio::test]
async fn lists_existing_sources() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send("GET", "/integration/sources", &ctx.admin_token, None)
        .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 1, "{body}");
    assert_eq!(rows[0]["dataset"], "employee");
    assert_eq!(rows[0]["id"], ctx.source_id.to_string());
}

/// 同一個 connection 的同一個 dataset 只能有一個來源
///
/// 這正是匯入精靈必須重用而非每次新建的原因
#[tokio::test]
async fn rejects_duplicate_source_for_same_dataset() {
    let ctx = Ctx::new().await;

    let connection_id: Uuid = {
        let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
        sqlx::query_scalar("select connection_id from integration_source where id = $1")
            .bind(ctx.source_id)
            .fetch_one(tx.executor())
            .await
            .unwrap()
    };

    let (status, body) = ctx
        .send(
            "POST",
            "/integration/sources",
            &ctx.admin_token,
            Some(json!({
                "connection_id": connection_id,
                "dataset": "employee",
                "name": "重複的來源",
                "mapping": { "工號": "employee_no" },
            })),
        )
        .await;

    assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

/// 來源刪得掉，同步歷史跟著刪
///
/// 換了人事系統時要能重新設定。歷史的意義是「這個來源同步過什麼」，
/// 來源沒了之後那些計數對不上任何東西
#[tokio::test]
async fn deletes_source_and_its_runs() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    let (_, runs) = ctx
        .send("GET", "/integration/sync-runs", &ctx.admin_token, None)
        .await;
    assert_eq!(runs.as_array().unwrap().len(), 1);

    let (status, body) = ctx
        .send(
            "DELETE",
            &format!("/integration/sources/{}", ctx.source_id),
            &ctx.admin_token,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (_, sources) = ctx
        .send("GET", "/integration/sources", &ctx.admin_token, None)
        .await;
    assert_eq!(sources.as_array().unwrap().len(), 0, "{sources}");

    // 歷史是 cascade
    let (_, runs) = ctx
        .send("GET", "/integration/sync-runs", &ctx.admin_token, None)
        .await;
    assert_eq!(runs.as_array().unwrap().len(), 0, "{runs}");

    // 但同步進來的員工還在——刪來源不等於刪資料
    assert_eq!(ctx.employee_count().await, 3);
}

/// mapping 不可指向不認得的欄位
#[tokio::test]
async fn source_rejects_unknown_canonical_field() {
    let ctx = Ctx::new().await;

    let (_, conn) = ctx
        .send(
            "POST",
            "/integration/connections",
            &ctx.admin_token,
            Some(json!({ "name": "第三條連線", "kind": "FILE" })),
        )
        .await;

    let (status, body) = ctx
        .send(
            "POST",
            "/integration/sources",
            &ctx.admin_token,
            Some(json!({
                "connection_id": conn["id"],
                "dataset": "employee",
                "name": "亂對應",
                "mapping": { "工號": "employee_no", "密碼": "password_hash" },
            })),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

/// 完整性門檻不可設為 0
///
/// 那等於關掉整個同步最重要的保護
#[tokio::test]
async fn rejects_zero_threshold() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "PATCH",
            &format!("/integration/sources/{}", ctx.source_id),
            &ctx.admin_token,
            Some(json!({ "completeness_threshold": 0.0 })),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

/// 目前只支援 FILE
#[tokio::test]
async fn rejects_unimplemented_connection_kind() {
    let ctx = Ctx::new().await;

    let (status, body) = ctx
        .send(
            "POST",
            "/integration/connections",
            &ctx.admin_token,
            Some(json!({ "name": "Odoo", "kind": "REST" })),
        )
        .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(
        body["message"].as_str().unwrap().contains("尚未實作"),
        "{body}"
    );
}

// ── 權限與隔離 ──────────────────────────────────────────

/// designer 不能碰同步
///
/// 同步會改動整份組織資料，而組織決定簽核路徑
#[tokio::test]
async fn designer_cannot_sync() {
    let ctx = Ctx::new().await;

    for (method, path) in [
        ("POST", "/integration/analyze".to_string()),
        ("POST", "/integration/connections".to_string()),
        (
            "POST",
            format!("/integration/sources/{}/sync", ctx.source_id),
        ),
        (
            "POST",
            format!("/integration/sources/{}/preview", ctx.source_id),
        ),
        ("GET", "/integration/sync-runs".to_string()),
    ] {
        let (status, body) = ctx
            .send(
                method,
                &path,
                &ctx.designer_token,
                Some(json!({ "content": "a\n1\n", "dataset": "employee", "name": "x", "kind": "FILE" })),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {path}：{body}");
    }
}

#[tokio::test]
async fn requires_authentication() {
    let ctx = Ctx::new().await;

    let req = Request::builder()
        .method("GET")
        .uri("/integration/sync-runs")
        .body(Body::empty())
        .unwrap();

    let resp = http_public::router(ctx.state.clone())
        .oneshot(req)
        .await
        .expect("請求失敗");

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// 同步不會跨租戶
#[tokio::test]
async fn does_not_leak_across_tenants() {
    let ctx_a = Ctx::new().await;
    let ctx_b = Ctx::new().await;

    ctx_a.sync(CSV_THREE).await;

    // B 租戶看不到 A 的員工
    assert_eq!(ctx_b.employee_count().await, 0);

    // B 也看不到 A 的同步紀錄
    let (_, runs) = ctx_b
        .send("GET", "/integration/sync-runs", &ctx_b.admin_token, None)
        .await;
    assert_eq!(runs.as_array().unwrap().len(), 0, "{runs}");
}

// ── 問題明細 ────────────────────────────────────────────

#[tokio::test]
async fn lists_issues_for_a_run() {
    let ctx = Ctx::new().await;

    let csv = "工號,姓名,信箱,部門,職稱,主管工號\n\
               E001,陳大明,a@test.local,NOSUCH,經理,\n";
    ctx.sync(csv).await;

    let (_, runs) = ctx
        .send("GET", "/integration/sync-runs", &ctx.admin_token, None)
        .await;
    let run_id = runs[0]["id"].as_str().unwrap();

    let (status, issues) = ctx
        .send(
            "GET",
            &format!("/integration/sync-runs/{run_id}/issues"),
            &ctx.admin_token,
            None,
        )
        .await;

    assert_eq!(status, StatusCode::OK, "{issues}");
    let list = issues.as_array().unwrap();
    assert!(!list.is_empty(), "{issues}");
    assert_eq!(list[0]["kind"], "VALIDATION_FAILED");
}

/// 稽核記計數而非資料
///
/// 組織資料含個資，稽核紀錄不該變成另一份副本
#[tokio::test]
async fn audits_counts_not_data() {
    let ctx = Ctx::new().await;
    ctx.sync(CSV_THREE).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let payload: Value = sqlx::query_scalar(
        "select payload from audit_event where action = 'integration.sync'",
    )
    .fetch_one(tx.executor())
    .await
    .expect("查不到稽核紀錄");

    assert_eq!(payload["counts"]["inserted_count"], 3);
    // 姓名與 email 不該出現在稽核紀錄裡
    let text = payload.to_string();
    assert!(!text.contains("陳大明"), "稽核不該含個資：{payload}");
    assert!(!text.contains("a@test.local"), "稽核不該含個資：{payload}");
}
