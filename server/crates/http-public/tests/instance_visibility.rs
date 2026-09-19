//! 流程監控的可見範圍（S0）
//!
//! 交辦（§5 S0）：「權限正確（一般使用者看不到別人的單）」。
//!
//! RLS 只隔離到**租戶**——同租戶的人預設看得見彼此的單。
//! 流程監控要的範圍更嚴，因此在查詢再收一層。
//!
//! 這是安全邊界，本檔是永久回歸測試：
//! `visible_to` 若被改成預設 None（不設限），這裡會紅。

use persistence::instance::{CreateInstance, ListFilter};
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

/// 一個租戶、兩個互不相干的人
async fn two_people() -> (Db, Uuid, Uuid, Uuid) {
    let db = Db::connect(&database_url()).await.expect("連線失敗");
    let admin = sqlx::postgres::PgPool::connect(&admin_url())
        .await
        .expect("管理連線失敗");

    let tenant_id = Uuid::new_v4();
    sqlx::query("insert into tenant (id, code, name) values ($1, $2, $3)")
        .bind(tenant_id)
        .bind(format!("vis-{}", &tenant_id.to_string()[..8]))
        .bind("可見範圍測試租戶")
        .execute(&admin)
        .await
        .expect("建租戶失敗");

    let mut ids = Vec::new();
    for who in ["alice", "bob"] {
        let id = Uuid::new_v4();
        sqlx::query(
            "insert into app_user (id, tenant_id, email, name, password_hash, status)
             values ($1, $2, $3, $4, 'x', 'ACTIVE')",
        )
        .bind(id)
        .bind(tenant_id)
        .bind(format!("{who}@vis.local"))
        .bind(who)
        .execute(&admin)
        .await
        .expect("建使用者失敗");
        ids.push(id);
    }

    (db, tenant_id, ids[0], ids[1])
}

/// 造一筆由 `owner` 發起的流程
async fn start_one(db: &Db, tenant: Uuid, owner: Uuid, key: &str) -> Uuid {
    let mut tx = db.tenant_tx(tenant).await.unwrap();

    // 流程定義：instance 需要一個存在的版本
    let wf_id = Uuid::new_v4();
    sqlx::query(
        "insert into workflow_definition (id, tenant_id, workflow_key, name, business_object)
         values ($1, $2, $3, '可見範圍測試', 'quotation')",
    )
    .bind(wf_id)
    .bind(tenant)
    .bind(format!("vis_{}", &Uuid::new_v4().to_string()[..8]))
    .execute(tx.executor())
    .await
    .unwrap();

    let ver_id = Uuid::new_v4();
    sqlx::query(
        "insert into workflow_definition_version
           (id, tenant_id, workflow_id, version, status, content, published_at)
         values ($1, $2, $3, 1, 'PUBLISHED', '{\"nodes\":[],\"edges\":[]}'::jsonb, now())",
    )
    .bind(ver_id)
    .bind(tenant)
    .bind(wf_id)
    .execute(tx.executor())
    .await
    .unwrap();

    let inst = persistence::instance::create(
        &mut tx,
        CreateInstance {
            workflow_version_id: ver_id,
            form_version_id: None,
            business_object: "quotation".into(),
            business_key: key.to_string(),
            temporal_workflow_id: format!("{tenant}:quotation:{key}"),
            temporal_run_id: String::new(),
            input: serde_json::json!({}),
        },
        owner,
    )
    .await
    .expect("建立實例失敗");

    tx.commit().await.unwrap();
    inst.id
}

async fn list_for(db: &Db, tenant: Uuid, visible_to: Option<Uuid>) -> Vec<String> {
    list_filtered(db, tenant, visible_to, false).await
}

async fn list_filtered(
    db: &Db,
    tenant: Uuid,
    visible_to: Option<Uuid>,
    needs_attention: bool,
) -> Vec<String> {
    let mut tx = db.tenant_tx(tenant).await.unwrap();
    let rows = persistence::instance::list(
        &mut tx,
        ListFilter {
            business_object: None,
            status: None,
            limit: 100,
            visible_to,
            needs_attention,
        },
    )
    .await
    .unwrap();
    tx.commit().await.ok();
    rows.into_iter().map(|r| r.business_key).collect()
}

#[tokio::test]
async fn a_user_does_not_see_another_persons_instance() {
    // **最重要的一條。** RLS 是租戶級的，同租戶的人預設看得見彼此。
    // 少了 visible_to，流程監控就變成全租戶的資料出口。
    let (db, tenant, alice, bob) = two_people().await;
    start_one(&db, tenant, alice, "ALICE-1").await;
    start_one(&db, tenant, bob, "BOB-1").await;

    let seen = list_for(&db, tenant, Some(alice)).await;

    assert!(seen.contains(&"ALICE-1".to_string()), "應看得到自己的單");
    assert!(
        !seen.contains(&"BOB-1".to_string()),
        "不可看到別人的單，實際看到：{seen:?}"
    );
}

#[tokio::test]
async fn unrestricted_sees_everything() {
    // 監控角色（admin）要看得到全部，否則監控本身沒有意義
    let (db, tenant, alice, bob) = two_people().await;
    start_one(&db, tenant, alice, "ALICE-2").await;
    start_one(&db, tenant, bob, "BOB-2").await;

    let seen = list_for(&db, tenant, None).await;

    assert!(seen.contains(&"ALICE-2".to_string()));
    assert!(seen.contains(&"BOB-2".to_string()));
}

#[tokio::test]
async fn a_participant_sees_the_instance_even_if_someone_else_started_it() {
    // 那張單本來就在你的收件匣裡，看得到它的存在不算新增洩漏。
    // 少了這條，簽核者在監控畫面看不到自己正在簽的單。
    let (db, tenant, alice, bob) = two_people().await;
    let inst = start_one(&db, tenant, bob, "BOB-3").await;

    let mut tx = db.tenant_tx(tenant).await.unwrap();
    persistence::human_task::create(
        &mut tx,
        persistence::human_task::CreateTask {
            instance_id: inst,
            node_id: "approve".into(),
            node_label: Some("簽核".into()),
            assignee_user_id: Some(alice),
            assignee_role: None,
            participant_kind: "internal".into(),
            form_key: None,
            due_at: None,
        },
    )
    .await
    .expect("建立待辦失敗");
    tx.commit().await.unwrap();

    let seen = list_for(&db, tenant, Some(alice)).await;
    assert!(
        seen.contains(&"BOB-3".to_string()),
        "有待辦的單要看得到，實際看到：{seen:?}"
    );
}

// ── 異常標記（N3）────────────────────────────────────────

#[tokio::test]
async fn attention_flag_does_not_widen_visibility() {
    // **安全回歸。** 異常標記是給監控角色看的，但它不能變成
    // 「被標記的單所有人都看得到」的後門。
    let (db, tenant, alice, bob) = two_people().await;
    let inst = start_one(&db, tenant, bob, "BOB-ATT").await;

    let mut tx = db.tenant_tx(tenant).await.unwrap();
    persistence::instance::mark_attention(&mut tx, inst, "ESCALATE_UNRESOLVED", "測試")
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let seen = list_for(&db, tenant, Some(alice)).await;
    assert!(
        !seen.contains(&"BOB-ATT".to_string()),
        "異常標記不該讓別人的單變得看得到，實際看到：{seen:?}"
    );
}

#[tokio::test]
async fn attention_filter_returns_only_flagged() {
    let (db, tenant, alice, _bob) = two_people().await;
    let flagged = start_one(&db, tenant, alice, "ATT-YES").await;
    start_one(&db, tenant, alice, "ATT-NO").await;

    let mut tx = db.tenant_tx(tenant).await.unwrap();
    persistence::instance::mark_attention(
        &mut tx,
        flagged,
        "ESCALATE_UNRESOLVED",
        "解析不到簽核人",
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let seen = list_filtered(&db, tenant, None, true).await;
    assert!(seen.contains(&"ATT-YES".to_string()));
    assert!(
        !seen.contains(&"ATT-NO".to_string()),
        "只看異常時不該出現正常的單"
    );
}

#[tokio::test]
async fn flagged_instances_sort_first() {
    // 監控頁的用途是找出要處理的單。異常沉在第三頁等於這功能沒做。
    let (db, tenant, alice, _bob) = two_people().await;
    start_one(&db, tenant, alice, "SORT-NORMAL").await;
    let flagged = start_one(&db, tenant, alice, "SORT-FLAGGED").await;

    let mut tx = db.tenant_tx(tenant).await.unwrap();
    persistence::instance::mark_attention(&mut tx, flagged, "ESCALATE_UNRESOLVED", "x")
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let seen = list_filtered(&db, tenant, None, false).await;
    assert_eq!(
        seen.first().map(String::as_str),
        Some("SORT-FLAGGED"),
        "異常要排最前面，實際順序：{seen:?}"
    );
}

#[tokio::test]
async fn finishing_an_instance_clears_the_flag() {
    // 結束的流程不需要人介入。不清掉的話監控頁會一直被
    // 已完成的單佔在最前面，真正該處理的反而被擠下去。
    let (db, tenant, alice, _bob) = two_people().await;
    let inst = start_one(&db, tenant, alice, "ATT-FINISH").await;

    let mut tx = db.tenant_tx(tenant).await.unwrap();
    persistence::instance::mark_attention(&mut tx, inst, "ESCALATE_UNRESOLVED", "x")
        .await
        .unwrap();
    persistence::instance::finish(&mut tx, inst, "COMPLETED", None, None)
        .await
        .unwrap();
    let after = persistence::instance::find_by_id(&mut tx, inst).await.unwrap();
    tx.commit().await.unwrap();

    assert!(!after.needs_attention, "結束後旗標要清掉");
    assert!(after.attention_code.is_none());
}

#[tokio::test]
async fn attention_is_not_marked_on_finished_instances() {
    // 已結束的流程收到遲來的異常回報時不該重新亮起——
    // Temporal 重送是可能的，那會讓已完成的單永遠掛著紅字。
    let (db, tenant, alice, _bob) = two_people().await;
    let inst = start_one(&db, tenant, alice, "ATT-LATE").await;

    let mut tx = db.tenant_tx(tenant).await.unwrap();
    persistence::instance::finish(&mut tx, inst, "COMPLETED", None, None)
        .await
        .unwrap();
    let marked = persistence::instance::mark_attention(&mut tx, inst, "ESCALATE_UNRESOLVED", "x")
        .await
        .unwrap();
    let after = persistence::instance::find_by_id(&mut tx, inst).await.unwrap();
    tx.commit().await.unwrap();

    assert!(!marked, "已結束的流程不該標記成功");
    assert!(!after.needs_attention);
}

#[tokio::test]
async fn clearing_attention_is_idempotent() {
    // 兩個監控人員同時按「已處理」時，第二個要得到 false
    // 而不是報錯——那不是錯誤，只是慢了一步。
    let (db, tenant, alice, _bob) = two_people().await;
    let inst = start_one(&db, tenant, alice, "ATT-CLEAR").await;

    let mut tx = db.tenant_tx(tenant).await.unwrap();
    persistence::instance::mark_attention(&mut tx, inst, "ESCALATE_UNRESOLVED", "x")
        .await
        .unwrap();
    let first = persistence::instance::clear_attention(&mut tx, inst).await.unwrap();
    let second = persistence::instance::clear_attention(&mut tx, inst).await.unwrap();
    tx.commit().await.unwrap();

    assert!(first, "第一次要真的清掉");
    assert!(!second, "第二次應回 false，不是報錯");
}

// ── 改派（N1）────────────────────────────────────────────

#[tokio::test]
async fn reassign_moves_the_task_and_drops_the_role() {
    // 改派沒清 assignee_role 的話，原角色的其他人在收件匣
    // 仍看得到這張待辦（inbox 是 user_id or role 的 or 條件），
    // 那等於沒有改派。
    let (db, tenant, alice, bob) = two_people().await;
    let inst = start_one(&db, tenant, bob, "REASSIGN-1").await;

    let mut tx = db.tenant_tx(tenant).await.unwrap();
    let task = persistence::human_task::create(
        &mut tx,
        persistence::human_task::CreateTask {
            instance_id: inst,
            node_id: "approve".into(),
            node_label: None,
            assignee_user_id: None,
            assignee_role: Some("approver".into()),
            participant_kind: "internal".into(),
            form_key: None,
            due_at: None,
        },
    )
    .await
    .unwrap();

    let moved = persistence::human_task::reassign(&mut tx, task.id, alice)
        .await
        .unwrap();
    let after = persistence::human_task::find_by_id(&mut tx, task.id)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(moved);
    assert_eq!(after.assignee_user_id, Some(alice));
    assert!(
        after.assignee_role.is_none(),
        "改派後要清掉角色，否則原角色的人還看得到"
    );
}

#[tokio::test]
async fn reassign_refuses_decided_tasks() {
    // 已決策的待辦改派會讓稽核裡「誰簽的」與「指派給誰」對不起來
    let (db, tenant, alice, bob) = two_people().await;
    let inst = start_one(&db, tenant, bob, "REASSIGN-2").await;

    let mut tx = db.tenant_tx(tenant).await.unwrap();
    let task = persistence::human_task::create(
        &mut tx,
        persistence::human_task::CreateTask {
            instance_id: inst,
            node_id: "approve".into(),
            node_label: None,
            assignee_user_id: Some(bob),
            assignee_role: None,
            participant_kind: "internal".into(),
            form_key: None,
            due_at: None,
        },
    )
    .await
    .unwrap();

    persistence::human_task::decide(&mut tx, task.id, "APPROVE", "", bob)
        .await
        .unwrap();
    let moved = persistence::human_task::reassign(&mut tx, task.id, alice)
        .await
        .unwrap();
    let after = persistence::human_task::find_by_id(&mut tx, task.id)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert!(!moved, "已決策的待辦不該改派成功");
    assert_eq!(after.assignee_user_id, Some(bob), "指派對象不該被改動");
}

#[tokio::test]
async fn participation_does_not_leak_sibling_instances() {
    // 在一張單上有待辦，不代表看得到同一個人的其他單
    let (db, tenant, alice, bob) = two_people().await;
    let inst = start_one(&db, tenant, bob, "BOB-4").await;
    start_one(&db, tenant, bob, "BOB-5").await;

    let mut tx = db.tenant_tx(tenant).await.unwrap();
    persistence::human_task::create(
        &mut tx,
        persistence::human_task::CreateTask {
            instance_id: inst,
            node_id: "approve".into(),
            node_label: None,
            assignee_user_id: Some(alice),
            assignee_role: None,
            participant_kind: "internal".into(),
            form_key: None,
            due_at: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let seen = list_for(&db, tenant, Some(alice)).await;
    assert!(seen.contains(&"BOB-4".to_string()));
    assert!(
        !seen.contains(&"BOB-5".to_string()),
        "沒有待辦的單不該看得到"
    );
}
