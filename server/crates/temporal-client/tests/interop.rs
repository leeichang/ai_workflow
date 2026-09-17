//! Rust ↔ Python 互通整合測試
//!
//! 驗證決議 D-02 的風險點：Temporal 沒有官方 Rust SDK，
//! Rust 端自行實作 Payload 編碼，必須與 Python `temporalio`
//! 的預設 DataConverter 相容。
//!
//! 不相容時的症狀是 Workflow 收到 None 或型別錯誤，
//! 而不是明確的解碼失敗——單元測試測不出來，只能實際跑一次。
//!
//! 需要：
//!   1. Temporal Server（poc/temporal/docker-compose.yml）
//!   2. Python Worker（python-ai/worker.py）
//!
//! 兩者任一未啟動時測試會 skip 而非失敗。CI 未備妥環境時
//! 不該讓整個測試套件紅燈，但本機驗證時必須真的跑過。

use serde_json::{json, Value};
use temporal_client::{StartWorkflow, TemporalClient, WorkflowResult};
use uuid::Uuid;

const TARGET: &str = "http://localhost:7233";
const NAMESPACE: &str = "default";
const TASK_QUEUE: &str = "workflow-platform";

/// 取得 client，環境未備妥時回 None
async fn client() -> Option<TemporalClient> {
    match TemporalClient::connect(TARGET, NAMESPACE).await {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("略過互通測試：無法連線 Temporal（{e}）");
            eprintln!("  啟動方式：cd poc/temporal && docker compose up -d");
            None
        }
    }
}

fn tenant() -> Uuid {
    Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
}

/// 測試用的 payload，涵蓋各種型別
///
/// 只測物件會漏掉 null 的特例：Python 把 None 編成 binary/null
/// 而非 json/plain 的 "null"，兩端處理不一致時只有這個值會錯。
fn rich_payload() -> Value {
    json!({
        "tenant_id": "11111111-1111-1111-1111-111111111111",
        "amount": 1234567.89,
        "approved": true,
        "rejected": false,
        "note": null,
        "customer": "台灣精密工業股份有限公司",
        "items": [
            { "sku": "A-001", "qty": 3, "price": 100.5 },
            { "sku": "B-002", "qty": 1, "price": 99.99 }
        ],
        "nested": { "level2": { "level3": ["深層", 42, null, true] } }
    })
}

async fn start_probe(c: &TemporalClient, instance: &str, arg: Value) -> Option<String> {
    let started = c
        .start_workflow(StartWorkflow {
            tenant_id: tenant(),
            business_object: "interop".into(),
            instance_id: instance.into(),
            workflow_type: "InteropProbe".into(),
            task_queue: TASK_QUEUE.into(),
            args: vec![arg],
        })
        .await;

    match started {
        Ok(s) => {
            assert!(!s.run_id.is_empty(), "啟動後應取得 run_id");
            Some(s.workflow_id)
        }
        Err(e) => {
            eprintln!("略過：啟動 Workflow 失敗（{e}）");
            eprintln!("  Python Worker 啟動方式：python-ai/.venv/bin/python python-ai/worker.py");
            None
        }
    }
}

/// 等 Workflow 進到可查詢狀態
///
/// 啟動後 Worker 需要一點時間領取任務。直接 Query 會拿到
/// 「沒有 Worker 回應」的錯誤。
async fn wait_queryable(c: &TemporalClient, wf_id: &str) -> bool {
    for _ in 0..30 {
        if c.query(wf_id, "current_state", &[]).await.is_ok() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }
    false
}

#[tokio::test]
async fn rust_啟動的流程_python_收得到完整_payload() {
    let Some(c) = client().await else { return };
    let instance = format!("probe-{}", Uuid::new_v4());

    let Some(wf_id) = start_probe(&c, &instance, rich_payload()).await else {
        return;
    };

    assert!(wait_queryable(&c, &wf_id).await, "Workflow 應可查詢");

    let state = c.query(&wf_id, "current_state", &[]).await.unwrap();
    let received = &state["received"];

    // 逐型別比對。整份相等即可，但分開斷言能指出是哪一種型別壞掉。
    assert_eq!(received["amount"], json!(1234567.89), "浮點數");
    assert_eq!(received["approved"], json!(true), "布林 true");
    assert_eq!(received["rejected"], json!(false), "布林 false");
    assert_eq!(received["note"], Value::Null, "null 值");
    assert_eq!(
        received["customer"], json!("台灣精密工業股份有限公司"),
        "非 ASCII 字串"
    );
    assert_eq!(received["items"][0]["sku"], json!("A-001"), "陣列內物件");
    assert_eq!(
        received["nested"]["level2"]["level3"],
        json!(["深層", 42, null, true]),
        "巢狀混合型別陣列"
    );

    assert_eq!(received, &rich_payload(), "整份 payload 應完全一致");

    c.signal(&wf_id, "finish", Value::Null).await.unwrap();
}

#[tokio::test]
async fn rust_送出的_signal_python_解得開() {
    let Some(c) = client().await else { return };
    let instance = format!("probe-signal-{}", Uuid::new_v4());

    let Some(wf_id) = start_probe(&c, &instance, json!({ "kind": "signal" })).await else {
        return;
    };
    assert!(wait_queryable(&c, &wf_id).await);

    // 各送一種型別，確認都不會在傳輸中變形
    let values = vec![
        json!({ "decision": "APPROVE", "comment": "同意" }),
        json!("純字串"),
        json!(42),
        json!(null),
        json!([1, 2, 3]),
    ];

    for v in &values {
        c.signal(&wf_id, "add_value", v.clone()).await.unwrap();
    }

    // Signal 是非同步的，等 Worker 處理完
    let mut state = json!({});
    for _ in 0..30 {
        state = c.query(&wf_id, "current_state", &[]).await.unwrap();
        if state["signal_count"] == json!(values.len()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    assert_eq!(
        state["signal_count"],
        json!(values.len()),
        "所有 Signal 都應送達"
    );
    assert_eq!(state["signals"], json!(values), "Signal 內容應原樣保留");

    c.signal(&wf_id, "finish", Value::Null).await.unwrap();
}

#[tokio::test]
async fn query_的參數與結果雙向相容() {
    let Some(c) = client().await else { return };
    let instance = format!("probe-query-{}", Uuid::new_v4());

    let Some(wf_id) = start_probe(&c, &instance, json!({ "kind": "query" })).await else {
        return;
    };
    assert!(wait_queryable(&c, &wf_id).await);

    // echo_arg 原樣回傳，可同時驗證送出與接收兩個方向
    let arg = rich_payload();
    let echoed = c.query(&wf_id, "echo_arg", &[arg.clone()]).await.unwrap();

    assert_eq!(echoed, arg, "Query 參數往返後應一致");

    c.signal(&wf_id, "finish", Value::Null).await.unwrap();
}

#[tokio::test]
async fn 相同_instance_重複啟動不會產生第二個流程() {
    // request_id 由 workflow_id 推導，Temporal 據此去重。
    // API 逾時重試時若每次都啟動新流程，會產生重複的簽核單。
    let Some(c) = client().await else { return };
    let instance = format!("probe-dedup-{}", Uuid::new_v4());

    let Some(first) = start_probe(&c, &instance, json!({ "n": 1 })).await else {
        return;
    };

    let second = c
        .start_workflow(StartWorkflow {
            tenant_id: tenant(),
            business_object: "interop".into(),
            instance_id: instance.clone(),
            workflow_type: "InteropProbe".into(),
            task_queue: TASK_QUEUE.into(),
            args: vec![json!({ "n": 2 })],
        })
        .await;

    match second {
        Ok(s) => assert_eq!(s.workflow_id, first, "重複啟動應回同一個 workflow_id"),
        Err(e) => {
            // Temporal 也可能直接回 AlreadyExists，同樣是正確行為
            let msg = e.to_string();
            assert!(
                msg.contains("already") || msg.contains("Already"),
                "重複啟動應被去重或明確拒絕，實得：{msg}"
            );
        }
    }

    // 確認第一次的參數沒被覆蓋
    assert!(wait_queryable(&c, &first).await);
    let state = c.query(&first, "current_state", &[]).await.unwrap();
    assert_eq!(state["received"]["n"], json!(1), "應保留第一次的參數");

    c.signal(&first, "finish", Value::Null).await.unwrap();
}

#[tokio::test]
async fn 取消流程讓_workflow_收到取消() {
    let Some(c) = client().await else { return };
    let instance = format!("probe-cancel-{}", Uuid::new_v4());

    let Some(wf_id) = start_probe(&c, &instance, json!({ "kind": "cancel" })).await else {
        return;
    };
    assert!(wait_queryable(&c, &wf_id).await);

    c.cancel(&wf_id, "測試取消").await.unwrap();

    // 用結束事件判斷，不能用「Query 失敗」。Temporal 對已結束的流程
    // 仍會回應 Query（重放最後狀態），拿它當結束訊號會永遠等不到。
    let result = c.get_result(&wf_id).await.unwrap();

    // 取消是「請求」而非強制終止。Workflow 攔下 CancelledError 做完
    // 清理再正常回傳時，最終狀態是 COMPLETED 而非 CANCELLED。
    // 正式的 Interpreter 也會這樣做（取消待辦、寫稽核），
    // 因此這裡接受兩種狀態，但必須確認取消真的被 Workflow 看見。
    match result {
        WorkflowResult::Completed(v) => {
            assert_eq!(
                v["cancelled"],
                json!(true),
                "流程正常結束時，必須是走取消分支結束的"
            );
        }
        WorkflowResult::Cancelled => {}
        other => panic!("取消後應結束，實得：{other:?}"),
    }
}

#[tokio::test]
async fn 未取消的流程正常結束時不帶取消標記() {
    // 對照組。少了它，上一個測試即使在「從未收到取消」的情況下
    // 也可能因為預設值而通過。
    let Some(c) = client().await else { return };
    let instance = format!("probe-normal-{}", Uuid::new_v4());

    let Some(wf_id) = start_probe(&c, &instance, json!({ "kind": "normal" })).await else {
        return;
    };
    assert!(wait_queryable(&c, &wf_id).await);

    c.signal(&wf_id, "finish", Value::Null).await.unwrap();

    let result = c.get_result(&wf_id).await.unwrap();
    match result {
        WorkflowResult::Completed(v) => {
            assert_eq!(v["cancelled"], json!(false), "正常結束不該帶取消標記");
            assert_eq!(v["echoed"]["kind"], json!("normal"));
        }
        other => panic!("應正常結束，實得：{other:?}"),
    }
}

#[tokio::test]
async fn peek_對執行中的流程立即回應() {
    // get_result 會 long poll 等到流程結束。HTTP 的 GET 處理若用它，
    // 一個該立刻回應的請求會卡住數十秒——實測時就是這樣卡住的。
    let Some(c) = client().await else { return };
    let instance = format!("probe-peek-{}", Uuid::new_v4());

    let Some(wf_id) = start_probe(&c, &instance, json!({ "kind": "peek" })).await else {
        return;
    };
    assert!(wait_queryable(&c, &wf_id).await);

    let started = std::time::Instant::now();
    let result = c.peek_result(&wf_id).await.unwrap();
    let elapsed = started.elapsed();

    assert_eq!(result, WorkflowResult::Running, "執行中應回報 Running");
    assert!(
        elapsed < std::time::Duration::from_secs(3),
        "peek 不該等待，實際耗時 {elapsed:?}"
    );

    c.signal(&wf_id, "finish", Value::Null).await.unwrap();
}

#[tokio::test]
async fn peek_對已結束的流程回報已完成() {
    let Some(c) = client().await else { return };
    let instance = format!("probe-peek-done-{}", Uuid::new_v4());

    let Some(wf_id) = start_probe(&c, &instance, json!({ "kind": "peek_done" })).await else {
        return;
    };
    assert!(wait_queryable(&c, &wf_id).await);

    c.signal(&wf_id, "finish", Value::Null).await.unwrap();

    // 等它真的結束，順便驗證 get_result 帶得回實際輸出
    match c.get_result(&wf_id).await.unwrap() {
        WorkflowResult::Completed(v) => {
            assert_eq!(v["echoed"]["kind"], json!("peek_done"));
        }
        other => panic!("應回報已完成，實得：{other:?}"),
    }

    // peek 走 Describe，只帶狀態不帶輸出。需要輸出時用 get_result。
    let started = std::time::Instant::now();
    let peeked = c.peek_result(&wf_id).await.unwrap();
    assert!(
        matches!(peeked, WorkflowResult::Completed(_)),
        "應回報已完成，實得：{peeked:?}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "peek 不該等待"
    );
}

#[tokio::test]
async fn 對不存在的流程送_signal_會失敗而非靜默成功() {
    // 靜默成功最危險：使用者按了核准、API 回 200，但流程從未收到。
    let Some(c) = client().await else { return };

    let missing = format!(
        "{}:interop:{}",
        tenant(),
        Uuid::new_v4()
    );
    let result = c.signal(&missing, "add_value", json!({ "x": 1 })).await;

    assert!(result.is_err(), "對不存在的流程送 Signal 應回錯誤");
}
