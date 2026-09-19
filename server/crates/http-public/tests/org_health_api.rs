//! 組織健康檢查整合測試
//!
//! 對真實 PostgreSQL 執行。每個測試自己建一個租戶與一組組織結構，
//! 然後驗證檢查有沒有抓到該抓的問題。
//!
//! 測試刻意**先建一個健康的組織**，再逐項破壞。
//! 只測「有問題時會報」不夠——更容易出錯的是「沒問題時誤報」，
//! 那會讓報表變成噪音，使用者就開始忽略它。
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
    token: String,
    /// 業務部
    sales_dept: Uuid,
    /// 業務部主管，本人隸屬於管理部（避免自己管自己的情況）
    manager: Uuid,
    /// 業務部員工，主管是 manager
    staff: Uuid,
}

impl Ctx {
    /// 建立一個**只有一項預期問題**的組織：
    ///
    ///   董事會（主管 陳經理）  ── 員工 王執行長
    ///   管理部（主管 王執行長）── 員工 陳經理（直屬主管 王執行長）
    ///   業務部（主管 陳經理）  ── 員工 林小明（直屬主管 陳經理）
    ///
    /// 主管一律**不隸屬於自己管的部門**——否則
    /// `department_manager_of` 的 `m.id <> u.id` 會把他排除，
    /// 他自己送單就解析不到簽核人。這是刻意的設計：
    /// 測試的基準組織要真的健康，不能自帶問題。
    ///
    /// 王執行長是頂點，沒有主管——這是唯一預期存在的 HIGH。
    /// 任何組織都有頂點，這不是資料錯誤，但 `manager_of` 確實
    /// 解析不到，所以照報，讓使用者知道他送單會卡。
    async fn new() -> Self {
        let db = Db::connect(&database_url())
            .await
            .expect("連線失敗。請確認容器已啟動且 migration 已套用");

        let admin = sqlx::postgres::PgPool::connect(&admin_url())
            .await
            .expect("管理連線失敗");

        let tenant_id = Uuid::new_v4();
        let code = format!("orgh-{}", &tenant_id.to_string()[..8]);

        sqlx::query("insert into tenant (id, code, name) values ($1, $2, $3)")
            .bind(tenant_id)
            .bind(&code)
            .bind("組織健康檢查測試租戶")
            .execute(&admin)
            .await
            .expect("建立租戶失敗");

        let jwt = JwtKeys::new(JWT_SECRET, 1);
        let mut tx = db.tenant_tx(tenant_id).await.expect("開啟交易失敗");

        let board_dept = create_dept(&mut tx, tenant_id, "BOARD", "董事會").await;
        let admin_dept = create_dept(&mut tx, tenant_id, "ADM", "管理部").await;
        let sales_dept = create_dept(&mut tx, tenant_id, "SALES", "業務部").await;

        let ceo = create_user(&mut tx, tenant_id, "ceo@test.local", "王執行長", board_dept).await;
        let manager =
            create_user(&mut tx, tenant_id, "mgr@test.local", "陳經理", admin_dept).await;
        let staff = create_user(&mut tx, tenant_id, "staff@test.local", "林小明", sales_dept).await;

        set_manager(&mut tx, manager, ceo).await;
        set_manager(&mut tx, staff, manager).await;

        // 每個部門的主管都不隸屬於自己管的部門，
        // 避開 DEPARTMENT_MANAGER_IS_SELF
        set_dept_manager(&mut tx, board_dept, manager).await;
        set_dept_manager(&mut tx, admin_dept, ceo).await;
        set_dept_manager(&mut tx, sales_dept, manager).await;

        let token = jwt
            .issue(ceo, tenant_id, "王執行長".into(), vec![])
            .expect("簽發 token 失敗");

        tx.commit().await.expect("提交失敗");

        Self {
            state: AppState {
                db,
                jwt,
                temporal: None,
                internal_token: Some("test-token".into()),
                mailer: http_public::Mailer::disabled(),
            },
            tenant_id,
            token,
            sales_dept,
            manager,
            staff,
        }
    }

    async fn tx(&self) -> persistence::TenantTx<'_> {
        self.state
            .db
            .tenant_tx(self.tenant_id)
            .await
            .expect("開啟交易失敗")
    }

    async fn get_health(&self) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("GET")
            .uri("/org/health")
            .header(header::AUTHORIZATION, format!("Bearer {}", self.token))
            .body(Body::empty())
            .unwrap();

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
}

async fn create_dept(
    tx: &mut persistence::TenantTx<'_>,
    tenant_id: Uuid,
    code: &str,
    name: &str,
) -> Uuid {
    sqlx::query_scalar(
        "insert into department (tenant_id, code, name) values ($1, $2, $3) returning id",
    )
    .bind(tenant_id)
    .bind(code)
    .bind(name)
    .fetch_one(tx.executor())
    .await
    .expect("建立部門失敗")
}

async fn create_user(
    tx: &mut persistence::TenantTx<'_>,
    tenant_id: Uuid,
    email: &str,
    name: &str,
    department_id: Uuid,
) -> Uuid {
    sqlx::query_scalar(
        "insert into app_user (tenant_id, email, name, password_hash, department_id)
         values ($1, $2, $3, 'x', $4) returning id",
    )
    .bind(tenant_id)
    .bind(email)
    .bind(name)
    .bind(department_id)
    .fetch_one(tx.executor())
    .await
    .expect("建立使用者失敗")
}

async fn set_manager(tx: &mut persistence::TenantTx<'_>, user: Uuid, manager: Uuid) {
    sqlx::query("update app_user set manager_id = $2 where id = $1")
        .bind(user)
        .bind(manager)
        .execute(tx.executor())
        .await
        .expect("設定主管失敗");
}

async fn set_dept_manager(tx: &mut persistence::TenantTx<'_>, dept: Uuid, manager: Uuid) {
    sqlx::query("update department set manager_user_id = $2 where id = $1")
        .bind(dept)
        .bind(manager)
        .execute(tx.executor())
        .await
        .expect("設定部門主管失敗");
}

/// 取出某個代碼的所有問題
fn issues_of<'a>(body: &'a Value, code: &str) -> Vec<&'a Value> {
    body["issues"]
        .as_array()
        .expect("issues 不是陣列")
        .iter()
        .filter(|i| i["code"] == code)
        .collect()
}

// ── 測試 ────────────────────────────────────────────────

/// 健康的組織只該報 CEO 沒有主管這一項
///
/// 這是最重要的測試：誤報會讓報表失去公信力
#[tokio::test]
async fn healthy_org_reports_only_the_root() {
    let ctx = Ctx::new().await;
    let (status, body) = ctx.get_health().await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["employee_count"], 3);
    assert_eq!(body["department_count"], 3);

    // 唯一的 HIGH 是 CEO 沒有主管
    let no_manager = issues_of(&body, "EMPLOYEE_NO_MANAGER");
    assert_eq!(no_manager.len(), 1, "只有 CEO 沒有主管：{body}");
    assert_eq!(no_manager[0]["subject_name"], "王執行長");

    assert_eq!(body["high_count"], 1, "不該有其他 HIGH：{body}");
    assert_eq!(body["medium_count"], 0, "不該有 MEDIUM：{body}");
}

/// 主管離職後不再是「解析失敗」，而是「解析到收不到信的人」
///
/// participant::manager_of 的 join 完全不看主管的狀態，
/// 所以流程照走、任務照建，只是沒有人會處理
#[tokio::test]
async fn detects_inactive_manager() {
    let ctx = Ctx::new().await;

    let mut tx = ctx.tx().await;
    sqlx::query("update app_user set left_at = current_date - 1 where id = $1")
        .bind(ctx.manager)
        .execute(tx.executor())
        .await
        .expect("設定離職失敗");
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;

    let inactive = issues_of(&body, "MANAGER_INACTIVE");
    assert_eq!(inactive.len(), 1, "林小明的主管已離職：{body}");
    assert_eq!(inactive[0]["subject_name"], "林小明");
    assert_eq!(inactive[0]["severity"], "HIGH");
    assert_eq!(inactive[0]["detail"], "主管：陳經理");

    // 離職的陳經理自己不再列入檢查——他不會送單
    let names: Vec<&Value> = body["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| &i["subject_name"])
        .collect();
    assert!(!names.contains(&&Value::String("陳經理".into())), "{body}");
    assert_eq!(body["employee_count"], 2, "離職者不算在分母：{body}");
}

/// 部門主管自己送單時 department_manager_of 解析回空
///
/// 需求 §9 的表格沒列到這項，但它是實務上最常見的卡點
#[tokio::test]
async fn detects_department_manager_managing_own_department() {
    let ctx = Ctx::new().await;

    // 把陳經理調到業務部。他是業務部主管，於是變成自己管自己
    let mut tx = ctx.tx().await;
    sqlx::query("update app_user set department_id = $2 where id = $1")
        .bind(ctx.manager)
        .bind(ctx.sales_dept)
        .execute(tx.executor())
        .await
        .expect("調動部門失敗");
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;

    let self_managed = issues_of(&body, "DEPARTMENT_MANAGER_IS_SELF");
    assert_eq!(self_managed.len(), 1, "{body}");
    assert_eq!(self_managed[0]["subject_name"], "陳經理");
    assert_eq!(self_managed[0]["severity"], "HIGH");
    assert_eq!(self_managed[0]["detail"], "部門：業務部");
}

/// 部門沒有主管
#[tokio::test]
async fn detects_department_without_manager() {
    let ctx = Ctx::new().await;

    let mut tx = ctx.tx().await;
    sqlx::query("update department set manager_user_id = null where id = $1")
        .bind(ctx.sales_dept)
        .execute(tx.executor())
        .await
        .expect("清除部門主管失敗");
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;

    let no_mgr = issues_of(&body, "DEPARTMENT_NO_MANAGER");
    assert_eq!(no_mgr.len(), 1, "{body}");
    assert_eq!(no_mgr[0]["subject_type"], "department");
    assert_eq!(no_mgr[0]["subject_name"], "業務部");
}

/// 主管鏈成環
///
/// 遞迴 CTE 必須在偵測到環時停下來，不能無限展開
#[tokio::test]
async fn detects_manager_cycle() {
    let ctx = Ctx::new().await;

    // 陳經理的主管改成林小明，而林小明的主管是陳經理
    let mut tx = ctx.tx().await;
    sqlx::query("update app_user set manager_id = $2 where id = $1")
        .bind(ctx.manager)
        .bind(ctx.staff)
        .execute(tx.executor())
        .await
        .expect("製造循環失敗");
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;

    let cycles = issues_of(&body, "MANAGER_CYCLE");
    // 環上的兩個人都會被標出來
    assert_eq!(cycles.len(), 2, "{body}");
    assert_eq!(cycles[0]["severity"], "HIGH");
}

/// 沒有部門、沒有 email——兩項 MEDIUM
///
/// 不測「email 重複」：`app_user` 有 `unique (tenant_id, email)`，
/// 重複在資料庫層就進不來
#[tokio::test]
async fn detects_medium_issues() {
    let ctx = Ctx::new().await;

    let mut tx = ctx.tx().await;
    sqlx::query("update app_user set department_id = null where id = $1")
        .bind(ctx.staff)
        .execute(tx.executor())
        .await
        .expect("清除部門失敗");

    // emails_of 用 email <> '' 過濾，空字串等同沒有信箱
    sqlx::query("update app_user set email = '' where id = $1")
        .bind(ctx.manager)
        .execute(tx.executor())
        .await
        .expect("清空 email 失敗");
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;

    assert_eq!(issues_of(&body, "EMPLOYEE_NO_DEPARTMENT").len(), 1, "{body}");
    assert_eq!(issues_of(&body, "EMPLOYEE_NO_EMAIL").len(), 1, "{body}");

    // MEDIUM 不影響 HIGH 的計數
    assert_eq!(body["medium_count"], 2, "{body}");
}

/// HIGH 排在 MEDIUM 前面
///
/// 使用者通常只修最上面幾項，順序決定他先修到什麼
#[tokio::test]
async fn high_issues_sort_first() {
    let ctx = Ctx::new().await;

    let mut tx = ctx.tx().await;
    sqlx::query("update app_user set department_id = null where id = $1")
        .bind(ctx.staff)
        .execute(tx.executor())
        .await
        .expect("清除部門失敗");
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;
    let issues = body["issues"].as_array().unwrap();

    let first_medium = issues.iter().position(|i| i["severity"] == "MEDIUM");
    let last_high = issues.iter().rposition(|i| i["severity"] == "HIGH");

    if let (Some(m), Some(h)) = (first_medium, last_high) {
        assert!(h < m, "HIGH 應該全部排在 MEDIUM 之前：{body}");
    } else {
        panic!("測試資料應該同時有 HIGH 與 MEDIUM：{body}");
    }
}

/// 已發布的流程引用了沒有成員的角色
///
/// demo 租戶的 sales_director 掛 0 人，而 manager_approval 的 P2D
/// 逾時要升級給它——加簽在實機上靜默不發生，Worker 只留一行
/// WARNING（總結 04 的 N2）。這是健康檢查該抓到的
#[tokio::test]
async fn detects_role_with_no_members() {
    let ctx = Ctx::new().await;

    let mut tx = ctx.tx().await;
    // 建一個沒有人的角色
    sqlx::query("insert into role (tenant_id, code, name) values ($1, 'empty_role', '空角色')")
        .bind(ctx.tenant_id)
        .execute(tx.executor())
        .await
        .expect("建立角色失敗");

    publish_flow_referencing(&mut tx, ctx.tenant_id, "empty_role", "resolver").await;
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;

    let issues = issues_of(&body, "ROLE_HAS_NO_MEMBERS");
    assert_eq!(issues.len(), 1, "{body}");
    assert_eq!(issues[0]["subject_name"], "空角色");
    assert_eq!(issues[0]["subject_type"], "role");
    assert_eq!(issues[0]["severity"], "HIGH");
}

/// timeout.to 的升級對象也要檢查
///
/// 這個位置比 resolver 更危險：解析不到時流程不會卡，
/// 而是**繼續等待**——畫面與稽核都看不到加簽沒發生
#[tokio::test]
async fn detects_empty_role_in_timeout_escalation() {
    let ctx = Ctx::new().await;

    let mut tx = ctx.tx().await;
    sqlx::query("insert into role (tenant_id, code, name) values ($1, 'escalate_to', '升級對象')")
        .bind(ctx.tenant_id)
        .execute(tx.executor())
        .await
        .expect("建立角色失敗");

    publish_flow_referencing(&mut tx, ctx.tenant_id, "escalate_to", "timeout").await;
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;

    let issues = issues_of(&body, "ROLE_HAS_NO_MEMBERS");
    assert_eq!(issues.len(), 1, "{body}");
    assert_eq!(issues[0]["subject_name"], "升級對象");
}

/// 有人的角色不報
#[tokio::test]
async fn role_with_members_is_not_reported() {
    let ctx = Ctx::new().await;

    let mut tx = ctx.tx().await;
    let role_id: Uuid = sqlx::query_scalar(
        "insert into role (tenant_id, code, name) values ($1, 'staffed', '有人的角色')
         returning id",
    )
    .bind(ctx.tenant_id)
    .fetch_one(tx.executor())
    .await
    .expect("建立角色失敗");

    sqlx::query("insert into user_role (user_id, role_id, tenant_id) values ($1, $2, $3)")
        .bind(ctx.staff)
        .bind(role_id)
        .bind(ctx.tenant_id)
        .execute(tx.executor())
        .await
        .expect("指派角色失敗");

    publish_flow_referencing(&mut tx, ctx.tenant_id, "staffed", "resolver").await;
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;
    assert_eq!(issues_of(&body, "ROLE_HAS_NO_MEMBERS").len(), 0, "{body}");
}

/// 草稿不檢查
///
/// 還在改的流程引用一個還沒建人的角色是正常的設計過程
#[tokio::test]
async fn draft_flows_are_not_checked() {
    let ctx = Ctx::new().await;

    let mut tx = ctx.tx().await;
    sqlx::query("insert into role (tenant_id, code, name) values ($1, 'draft_role', '草稿角色')")
        .bind(ctx.tenant_id)
        .execute(tx.executor())
        .await
        .expect("建立角色失敗");

    let flow_id: Uuid = sqlx::query_scalar(
        "insert into workflow_definition (tenant_id, workflow_key, business_object, name)
         values ($1, 'draft_flow', 'quotation', '草稿流程') returning id",
    )
    .bind(ctx.tenant_id)
    .fetch_one(tx.executor())
    .await
    .expect("建立流程失敗");

    sqlx::query(
        "insert into workflow_definition_version (tenant_id, workflow_id, content, status)
         values ($1, $2, $3, 'DRAFT')",
    )
    .bind(ctx.tenant_id)
    .bind(flow_id)
    .bind(flow_content("draft_role", "resolver"))
    .execute(tx.executor())
    .await
    .expect("建立版本失敗");
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;
    assert_eq!(issues_of(&body, "ROLE_HAS_NO_MEMBERS").len(), 0, "{body}");
}

/// 流程引用了不存在的角色
///
/// 比「角色沒有人」更嚴重——跳過不報等於讓它完全看不見
#[tokio::test]
async fn detects_reference_to_missing_role() {
    let ctx = Ctx::new().await;

    let mut tx = ctx.tx().await;
    publish_flow_referencing(&mut tx, ctx.tenant_id, "no_such_role", "resolver").await;
    tx.commit().await.unwrap();

    let (_, body) = ctx.get_health().await;

    let issues = issues_of(&body, "ROLE_HAS_NO_MEMBERS");
    assert_eq!(issues.len(), 1, "{body}");
    assert!(
        issues[0]["detail"].as_str().unwrap().contains("不存在"),
        "{body}"
    );
}

/// 最小的流程 DSL，把角色放在指定位置
fn flow_content(role_code: &str, position: &str) -> serde_json::Value {
    let node = if position == "timeout" {
        json!({
            "id": "approve",
            "type": "human_approval",
            "label": "簽核",
            "participant": "internal",
            "resolver": { "type": "manager_of", "of": "initiator" },
            "timeout": {
                "after": "P2D",
                "policy": "ESCALATE",
                "to": { "type": "role", "value": role_code }
            }
        })
    } else {
        json!({
            "id": "approve",
            "type": "human_approval",
            "label": "簽核",
            "participant": "internal",
            "resolver": { "type": "role", "value": role_code }
        })
    };

    json!({
        "workflow_key": "test_flow",
        "version": 1,
        "business_object": "quotation",
        "name": "測試流程",
        "nodes": [
            { "id": "start", "type": "trigger", "label": "送出" },
            node,
            { "id": "end", "type": "end", "label": "結束", "result": "completed" }
        ],
        "edges": [["start", "approve"], ["approve", "end"]]
    })
}

/// 建立並發布一個引用指定角色的流程
async fn publish_flow_referencing(
    tx: &mut persistence::TenantTx<'_>,
    tenant_id: Uuid,
    role_code: &str,
    position: &str,
) {
    let flow_id: Uuid = sqlx::query_scalar(
        "insert into workflow_definition (tenant_id, workflow_key, business_object, name)
         values ($1, $2, 'quotation', '測試流程') returning id",
    )
    .bind(tenant_id)
    .bind(format!("flow_{role_code}"))
    .fetch_one(tx.executor())
    .await
    .expect("建立流程失敗");

    sqlx::query(
        "insert into workflow_definition_version
            (tenant_id, workflow_id, version, content, status, published_at)
         values ($1, $2, 1, $3, 'PUBLISHED', now())",
    )
    .bind(tenant_id)
    .bind(flow_id)
    .bind(flow_content(role_code, position))
    .execute(tx.executor())
    .await
    .expect("發布版本失敗");
}

/// 未登入不能看
#[tokio::test]
async fn requires_authentication() {
    let ctx = Ctx::new().await;

    let req = Request::builder()
        .method("GET")
        .uri("/org/health")
        .body(Body::empty())
        .unwrap();

    let resp = http_public::router(ctx.state.clone())
        .oneshot(req)
        .await
        .expect("請求失敗");

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// 只看得到自己租戶的組織
///
/// 健康檢查的查詢全部走 tenant_tx，理論上受 RLS 保護。
/// 但「理論上」不夠——這裡實際建第二個租戶驗證
#[tokio::test]
async fn does_not_leak_across_tenants() {
    let ctx_a = Ctx::new().await;
    let ctx_b = Ctx::new().await;

    let (_, body_a) = ctx_a.get_health().await;
    let (_, body_b) = ctx_b.get_health().await;

    // 兩邊各自只有自己的 3 人 2 部門
    assert_eq!(body_a["employee_count"], 3, "{body_a}");
    assert_eq!(body_b["employee_count"], 3, "{body_b}");

    let a_ids: Vec<&Value> = body_a["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| &i["subject_id"])
        .collect();
    for issue in body_b["issues"].as_array().unwrap() {
        assert!(
            !a_ids.contains(&&issue["subject_id"]),
            "租戶 B 的問題不該出現在租戶 A：{body_a}"
        );
    }
}
