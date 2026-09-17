//! 收件匣與決策 API 整合測試
//!
//! 重點在授權：RLS 只保證同租戶，不保證「這張待辦是你的」。
//! 少了那道檢查，同租戶的任何人都能核准別人的單。

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_public::{AppState, JwtKeys};
use persistence::Db;
use serde_json::{json, Value};
use tower::ServiceExt;
use uuid::Uuid;

const JWT_SECRET: &[u8] = b"integration-test-secret-32-bytes!!!!";
const INTERNAL_TOKEN: &str = "test-internal-token";

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
    /// 被指派待辦的人
    assignee_token: String,
    assignee_id: Uuid,
    /// 同租戶但沒被指派的人
    stranger_token: String,
    instance_id: Uuid,
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
            .bind(format!("task-{}", &tenant_id.to_string()[..8]))
            .bind("待辦測試租戶")
            .execute(&admin)
            .await
            .expect("建立租戶失敗");

        let jwt = JwtKeys::new(JWT_SECRET, 1);
        let mut tx = db.tenant_tx(tenant_id).await.unwrap();

        sqlx::query("insert into role (tenant_id, code, name) values ($1, 'approver', '簽核人')")
            .bind(tenant_id)
            .execute(tx.executor())
            .await
            .unwrap();

        let mut tokens = Vec::new();
        let mut ids = Vec::new();
        for (email, name) in [("a@test.local", "簽核人"), ("s@test.local", "路人")] {
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

            ids.push(uid);
            tokens.push(
                jwt.issue(uid, tenant_id, name.into(), vec!["approver".into()])
                    .unwrap(),
            );
        }

        // 建立流程定義與實例，待辦必須掛在實例底下
        let wf_id: Uuid = sqlx::query_scalar(
            "insert into workflow_definition (tenant_id, workflow_key, business_object, name)
             values ($1, 'test_flow', 'quotation', '測試流程') returning id",
        )
        .bind(tenant_id)
        .fetch_one(tx.executor())
        .await
        .unwrap();

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

        let instance_id: Uuid = sqlx::query_scalar(
            "insert into workflow_instance
                (tenant_id, workflow_version_id, business_object, business_key,
                 temporal_workflow_id, started_by)
             values ($1, $2, 'quotation', 'QT-TEST-1', $3, $4) returning id",
        )
        .bind(tenant_id)
        .bind(version_id)
        .bind(format!("{tenant_id}:quotation:QT-TEST-1"))
        .bind(ids[0])
        .fetch_one(tx.executor())
        .await
        .unwrap();

        tx.commit().await.unwrap();

        Self {
            state: AppState {
                db,
                jwt,
                // 決策需要 Temporal 送 Signal。未連線時應回 503，
                // 這本身就是要驗證的行為之一。
                temporal: None,
                internal_token: Some(INTERNAL_TOKEN.into()),
                // 測試不寄信
                mailer: http_public::Mailer::disabled(),
            },
            tenant_id,
            assignee_token: tokens[0].clone(),
            assignee_id: ids[0],
            stranger_token: tokens[1].clone(),
            instance_id,
        }
    }

    async fn create_task(&self, assignee: Option<Uuid>, role: Option<&str>) -> Uuid {
        let mut tx = self.state.db.tenant_tx(self.tenant_id).await.unwrap();
        let task = persistence::human_task::create(
            &mut tx,
            persistence::human_task::CreateTask {
                instance_id: self.instance_id,
                node_id: "approval".into(),
                node_label: Some("測試簽核".into()),
                assignee_user_id: assignee,
                assignee_role: role.map(str::to_string),
                participant_kind: "internal".into(),
                form_key: None,
                due_at: None,
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        task.id
    }

    async fn send(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
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

        let response = http_public::router(self.state.clone())
            .oneshot(req)
            .await
            .unwrap();

        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, value)
    }
}

#[tokio::test]
async fn 收件匣只顯示自己的待辦() {
    let ctx = Ctx::new().await;
    ctx.create_task(Some(ctx.assignee_id), None).await;

    let (status, body) = ctx
        .send("GET", "/tasks", Some(&ctx.assignee_token), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);

    let (status, body) = ctx
        .send("GET", "/tasks", Some(&ctx.stranger_token), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.as_array().unwrap().is_empty(),
        "沒被指派的人不該看到待辦"
    );
}

#[tokio::test]
async fn 收件匣帶出業務單號() {
    // 少了單號，使用者看到「測試簽核」卻不知道是哪一張單
    let ctx = Ctx::new().await;
    ctx.create_task(Some(ctx.assignee_id), None).await;

    let (_, body) = ctx
        .send("GET", "/tasks", Some(&ctx.assignee_token), None)
        .await;

    assert_eq!(body[0]["business_key"], json!("QT-TEST-1"));
    assert_eq!(body[0]["business_object"], json!("quotation"));
}

#[tokio::test]
async fn 指派給角色時該角色的人都看得到() {
    let ctx = Ctx::new().await;
    ctx.create_task(None, Some("approver")).await;

    // 兩人都有 approver 角色
    for token in [&ctx.assignee_token, &ctx.stranger_token] {
        let (_, body) = ctx.send("GET", "/tasks", Some(token), None).await;
        assert_eq!(body.as_array().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn 未認證時拒絕() {
    let ctx = Ctx::new().await;
    let (status, _) = ctx.send("GET", "/tasks", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn 不是自己的待辦不可決策() {
    // RLS 只保證同租戶，不保證是本人。少了這道檢查，
    // 同租戶的任何人都能核准別人的單。
    let ctx = Ctx::new().await;
    let task_id = ctx.create_task(Some(ctx.assignee_id), None).await;

    let (status, body) = ctx
        .send(
            "POST",
            &format!("/tasks/{task_id}/decision"),
            Some(&ctx.stranger_token),
            Some(json!({ "decision": "APPROVE" })),
        )
        .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(body["message"].as_str().unwrap().contains("不是指派給您"));
}

#[tokio::test]
async fn 決策值必須是_approve_或_reject() {
    let ctx = Ctx::new().await;
    let task_id = ctx.create_task(Some(ctx.assignee_id), None).await;

    let (status, _) = ctx
        .send(
            "POST",
            &format!("/tasks/{task_id}/decision"),
            Some(&ctx.assignee_token),
            Some(json!({ "decision": "MAYBE" })),
        )
        .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn temporal_未連線時決策回_503_且不改狀態() {
    // 流程引擎收不到 Signal 時，待辦必須維持 PENDING。
    // 若先提交再送 Signal，這裡會變成「待辦顯示已核准、
    // 流程卻停在原地等」——而且沒有人會再送一次。
    let ctx = Ctx::new().await;
    let task_id = ctx.create_task(Some(ctx.assignee_id), None).await;

    let (status, _) = ctx
        .send(
            "POST",
            &format!("/tasks/{task_id}/decision"),
            Some(&ctx.assignee_token),
            Some(json!({ "decision": "APPROVE" })),
        )
        .await;

    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let task = persistence::human_task::find_by_id(&mut tx, task_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(task.status, "PENDING", "Signal 失敗時待辦必須維持 PENDING");
    assert!(task.decision.is_none());
}

#[tokio::test]
async fn 已決策的待辦不可再次決策() {
    let ctx = Ctx::new().await;
    let task_id = ctx.create_task(Some(ctx.assignee_id), None).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    persistence::human_task::decide(&mut tx, task_id, "APPROVE", "", ctx.assignee_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (status, _) = ctx
        .send(
            "POST",
            &format!("/tasks/{task_id}/decision"),
            Some(&ctx.assignee_token),
            Some(json!({ "decision": "APPROVE" })),
        )
        .await;

    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn 同時決策只有一個人成功() {
    // 兩人同時點同一張角色待辦。decide() 只從 PENDING 轉出，
    // 第二個人得到 false，API 回 409 而非覆蓋第一個人的決定。
    let ctx = Ctx::new().await;
    let task_id = ctx.create_task(None, Some("approver")).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let first = persistence::human_task::decide(&mut tx, task_id, "APPROVE", "", ctx.assignee_id)
        .await
        .unwrap();
    let second = persistence::human_task::decide(&mut tx, task_id, "REJECT", "", ctx.assignee_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(first, "第一次決策應成功");
    assert!(!second, "第二次決策應失敗，不可覆蓋");
}

#[tokio::test]
async fn 取消待辦不影響已決策的() {
    // 已決策的是使用者真實做過的事，蓋掉會讓稽核紀錄失真
    let ctx = Ctx::new().await;
    let approved = ctx.create_task(Some(ctx.assignee_id), None).await;
    let pending = ctx.create_task(Some(ctx.assignee_id), None).await;

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    persistence::human_task::decide(&mut tx, approved, "APPROVE", "", ctx.assignee_id)
        .await
        .unwrap();

    let cancelled =
        persistence::human_task::cancel_by_instance(&mut tx, ctx.instance_id, "流程已結束")
            .await
            .unwrap();

    let a = persistence::human_task::find_by_id(&mut tx, approved)
        .await
        .unwrap();
    let p = persistence::human_task::find_by_id(&mut tx, pending)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(cancelled, 1, "只該取消 PENDING 的那一張");
    assert_eq!(a.status, "APPROVED", "已核准的不該被取消");
    assert_eq!(p.status, "CANCELLED");
}

#[tokio::test]
async fn internal_api_需要正確密鑰() {
    let ctx = Ctx::new().await;

    let request = Request::builder()
        .method("POST")
        .uri("/internal/human-tasks/cancel")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-internal-token", "wrong-token")
        .header("x-tenant-id", ctx.tenant_id.to_string())
        .body(Body::from(json!({ "task_ids": [] }).to_string()))
        .unwrap();

    let response = http_public::router(ctx.state.clone())
        .oneshot(request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn internal_api_缺少租戶標頭時拒絕() {
    let ctx = Ctx::new().await;

    let request = Request::builder()
        .method("POST")
        .uri("/internal/human-tasks/cancel")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-internal-token", INTERNAL_TOKEN)
        .body(Body::from(json!({ "task_ids": [] }).to_string()))
        .unwrap();

    let response = http_public::router(ctx.state.clone())
        .oneshot(request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn 回報流程結束會更新狀態並取消殘留待辦() {
    let ctx = Ctx::new().await;
    ctx.create_task(Some(ctx.assignee_id), None).await;

    let request = Request::builder()
        .method("POST")
        .uri(format!("/internal/instances/{}/finish", ctx.instance_id))
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-internal-token", INTERNAL_TOKEN)
        .header("x-tenant-id", ctx.tenant_id.to_string())
        .body(Body::from(
            json!({ "status": "COMPLETED", "output": {} }).to_string(),
        ))
        .unwrap();

    let response = http_public::router(ctx.state.clone())
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let instance = persistence::instance::find_by_id(&mut tx, ctx.instance_id)
        .await
        .unwrap();
    let tasks = persistence::human_task::inbox(
        &mut tx,
        ctx.assignee_id,
        &["approver".to_string()],
        "PENDING",
        10,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(instance.status, "COMPLETED");
    assert!(instance.ended_at.is_some());
    assert!(tasks.is_empty(), "流程結束後不該留下 PENDING 待辦");
}

#[tokio::test]
async fn 重複回報結束不會覆蓋結果() {
    // Temporal 的 Activity 重試本來就可能造成重複呼叫
    let ctx = Ctx::new().await;

    let finish = |status: &'static str| {
        let state = ctx.state.clone();
        let tenant = ctx.tenant_id;
        let instance = ctx.instance_id;
        async move {
            let request = Request::builder()
                .method("POST")
                .uri(format!("/internal/instances/{instance}/finish"))
                .header(header::CONTENT_TYPE, "application/json")
                .header("x-internal-token", INTERNAL_TOKEN)
                .header("x-tenant-id", tenant.to_string())
                .body(Body::from(json!({ "status": status }).to_string()))
                .unwrap();

            let response = http_public::router(state).oneshot(request).await.unwrap();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        }
    };

    assert_eq!(finish("COMPLETED").await["updated"], json!(true));
    assert_eq!(
        finish("FAILED").await["updated"],
        json!(false),
        "第二次回報不該更新"
    );

    let mut tx = ctx.state.db.tenant_tx(ctx.tenant_id).await.unwrap();
    let instance = persistence::instance::find_by_id(&mut tx, ctx.instance_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(instance.status, "COMPLETED", "首次的結果應保留");
}
