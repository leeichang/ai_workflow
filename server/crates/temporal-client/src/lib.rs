//! Temporal gRPC Client
//!
//! Rust 端直接以 gRPC 控制 Temporal，不經 Python。原因是
//! Temporal 沒有官方 Rust SDK（見決議 D-02），而啟動流程、
//! 送 Signal 這些動作發生在 API 的請求處理中，多繞一層 Python
//! 會讓交易邊界變得難以推理。
//!
//! Workflow 的「執行」仍在 Python（`python-ai/`），這裡只做控制面。
//!
//! 最容易出錯的兩點各自獨立成模組並有測試守住：
//!   payload.rs      與 Python temporalio 的編碼相容性
//!   workflow_id.rs  租戶隔離前綴

pub mod payload;
pub mod workflow_id;

pub mod proto {
    //! tonic 由 .proto 產生的型別
    //!
    //! 巢狀 module 宣告由 build.rs 掃描產物自動生成。
    //! protoc 產生的 package 數量遠多於直覺（目前 29 個），
    //! 手寫清單漏掉任何一個都是編譯錯誤。
    include!(concat!(env!("OUT_DIR"), "/_modules.rs"));
}

use proto::temporal::api::common::v1::WorkflowType;
use proto::temporal::api::taskqueue::v1::TaskQueue;
use proto::temporal::api::workflowservice::v1::workflow_service_client::WorkflowServiceClient;
use proto::temporal::api::workflowservice::v1::{
    QueryWorkflowRequest, RequestCancelWorkflowExecutionRequest, SignalWorkflowExecutionRequest,
    StartWorkflowExecutionRequest,
};
use serde_json::Value;
use tonic::transport::Channel;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum TemporalError {
    #[error("無法連線至 Temporal：{0}")]
    Connect(#[from] tonic::transport::Error),

    #[error("Temporal 呼叫失敗：{0}")]
    Rpc(#[from] tonic::Status),

    #[error("workflow_id 組合失敗：{0}")]
    WorkflowId(#[from] workflow_id::WorkflowIdError),

    /// Query 在 Workflow 內拋出例外時回傳。與 RPC 失敗分開，
    /// 前者是流程邏輯問題，後者是基礎設施問題，處理方式不同。
    #[error("Query 被拒絕：{0}")]
    QueryRejected(String),
}

pub type Result<T> = std::result::Result<T, TemporalError>;

/// get_result 的預設等待上限
///
/// 比 Temporal 伺服器端的 long poll 逾時（約 20 秒）略長，
/// 讓伺服器先回應，避免每次都由這端切斷連線。
const DEFAULT_RESULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// 啟動流程的參數
#[derive(Debug, Clone)]
pub struct StartWorkflow {
    pub tenant_id: Uuid,
    pub business_object: String,
    pub instance_id: String,
    /// Python 端 @workflow.defn 的名稱
    pub workflow_type: String,
    pub task_queue: String,
    /// 位置引數。單一 dict 參數也要包成長度 1 的陣列。
    pub args: Vec<Value>,
}

#[derive(Debug, Clone)]
pub struct StartedWorkflow {
    pub workflow_id: String,
    pub run_id: String,
}

/// 流程的最終狀態
#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowResult {
    Running,
    Completed(Value),
    Failed(String),
    Cancelled,
    Terminated(String),
    TimedOut,
}

impl WorkflowResult {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }
}

#[derive(Clone)]
pub struct TemporalClient {
    inner: WorkflowServiceClient<Channel>,
    namespace: String,
}

impl TemporalClient {
    /// 連線至 Temporal
    ///
    /// target 形如 `http://localhost:7233`。
    pub async fn connect(target: &str, namespace: &str) -> Result<Self> {
        let channel = Channel::from_shared(target.to_string())
            .map_err(|e| {
                // InvalidUri 不是 transport::Error，轉成 RPC 錯誤以統一型別
                TemporalError::Rpc(tonic::Status::invalid_argument(format!(
                    "Temporal 位址格式錯誤：{e}"
                )))
            })?
            .connect()
            .await?;

        Ok(Self {
            inner: WorkflowServiceClient::new(channel),
            namespace: namespace.to_string(),
        })
    }

    /// 啟動流程
    ///
    /// `request_id` 用 workflow_id 推導而非隨機值：Temporal 以它
    /// 去重，重送同一個請求不會啟動第二個流程。API 逾時重試時
    /// 這一點很重要。
    pub async fn start_workflow(&self, req: StartWorkflow) -> Result<StartedWorkflow> {
        let wf_id = workflow_id::build(req.tenant_id, &req.business_object, &req.instance_id)?;

        let request = StartWorkflowExecutionRequest {
            namespace: self.namespace.clone(),
            workflow_id: wf_id.clone(),
            workflow_type: Some(WorkflowType {
                name: req.workflow_type,
            }),
            task_queue: Some(TaskQueue {
                name: req.task_queue,
                kind: 0,
                normal_name: String::new(),
            }),
            input: Some(payload::encode_all(&req.args)),
            request_id: Uuid::new_v5(&Uuid::NAMESPACE_OID, wf_id.as_bytes()).to_string(),
            ..Default::default()
        };

        let response = self
            .inner
            .clone()
            .start_workflow_execution(request)
            .await?
            .into_inner();

        Ok(StartedWorkflow {
            workflow_id: wf_id,
            run_id: response.run_id,
        })
    }

    /// 送 Signal
    ///
    /// 不指定 run_id，Temporal 會送給目前執行中的那一個。
    /// 指定舊的 run_id 會失敗，而流程可能因 continue-as-new
    /// 換過 run，因此一律留空。
    pub async fn signal(&self, workflow_id: &str, name: &str, arg: Value) -> Result<()> {
        let request = SignalWorkflowExecutionRequest {
            namespace: self.namespace.clone(),
            workflow_execution: Some(execution(workflow_id)),
            signal_name: name.to_string(),
            input: Some(payload::encode_all(&[arg])),
            request_id: Uuid::new_v4().to_string(),
            ..Default::default()
        };

        self.inner.clone().signal_workflow_execution(request).await?;
        Ok(())
    }

    /// 查詢流程狀態
    ///
    /// Query 不寫入事件歷史，可安全地頻繁呼叫。
    /// 但它會等 Worker 回應，Worker 掛掉時會逾時而非立即失敗。
    pub async fn query(&self, workflow_id: &str, name: &str, args: &[Value]) -> Result<Value> {
        let request = QueryWorkflowRequest {
            namespace: self.namespace.clone(),
            execution: Some(execution(workflow_id)),
            query: Some(proto::temporal::api::query::v1::WorkflowQuery {
                query_type: name.to_string(),
                query_args: Some(payload::encode_all(args)),
                header: None,
            }),
            ..Default::default()
        };

        let response = self.inner.clone().query_workflow(request).await?.into_inner();

        if let Some(rejected) = response.query_rejected {
            return Err(TemporalError::QueryRejected(format!(
                "流程狀態為 {:?}",
                rejected.status
            )));
        }

        Ok(payload::decode_first(response.query_result.as_ref()))
    }

    /// 等待流程結束並取得結果
    ///
    /// 用 long poll 而非輪詢：Temporal 會 hold 住連線直到流程結束，
    /// 不會產生大量無謂的請求。
    ///
    /// 注意「流程已結束」不能用 Query 失敗來判斷。Temporal 對已結束
    /// 的流程仍會回應 Query（重放最後狀態），這個誤判曾讓取消測試
    /// 假性失敗。
    pub async fn get_result(&self, workflow_id: &str) -> Result<WorkflowResult> {
        self.get_result_within(workflow_id, DEFAULT_RESULT_TIMEOUT)
            .await
    }

    /// 查詢目前狀態，不等待
    ///
    /// 給 HTTP 請求處理用。流程還在跑時 long poll 會一直 hold 住連線，
    /// 讓一個原本該立刻回應的 GET 卡住二十秒。
    ///
    /// 實作用 DescribeWorkflowExecution 而非 GetWorkflowExecutionHistory：
    /// 後者即使 wait_new_event = false，搭配 CloseEvent 過濾時仍會
    /// long poll 到伺服器端逾時（實測 20 秒）。Describe 是單純的
    /// 狀態查詢，一定立即回應。
    pub async fn peek_result(&self, workflow_id: &str) -> Result<WorkflowResult> {
        use proto::temporal::api::enums::v1::WorkflowExecutionStatus as S;
        use proto::temporal::api::workflowservice::v1::DescribeWorkflowExecutionRequest;

        let request = DescribeWorkflowExecutionRequest {
            namespace: self.namespace.clone(),
            execution: Some(execution(workflow_id)),
        };

        let response = self
            .inner
            .clone()
            .describe_workflow_execution(request)
            .await?
            .into_inner();

        let status = response
            .workflow_execution_info
            .map(|i| i.status)
            .unwrap_or(0);

        Ok(match S::try_from(status) {
            Ok(S::Running) | Ok(S::ContinuedAsNew) => WorkflowResult::Running,
            // Describe 不帶回傳值。需要結果時再呼叫 get_result，
            // 此時流程已結束，long poll 會立即返回。
            Ok(S::Completed) => WorkflowResult::Completed(Value::Null),
            Ok(S::Failed) => WorkflowResult::Failed(String::new()),
            Ok(S::Canceled) => WorkflowResult::Cancelled,
            Ok(S::Terminated) => WorkflowResult::Terminated(String::new()),
            Ok(S::TimedOut) => WorkflowResult::TimedOut,
            _ => WorkflowResult::Running,
        })
    }

    /// 取得結果，最多等待指定時間
    ///
    /// wait 為 0 時不等待，直接回報當下狀態（未結束則為 Running）。
    pub async fn get_result_within(
        &self,
        workflow_id: &str,
        wait: std::time::Duration,
    ) -> Result<WorkflowResult> {
        use proto::temporal::api::enums::v1::HistoryEventFilterType;
        use proto::temporal::api::history::v1::history_event::Attributes;
        use proto::temporal::api::workflowservice::v1::GetWorkflowExecutionHistoryRequest;

        let wait_new_event = !wait.is_zero();

        let request = GetWorkflowExecutionHistoryRequest {
            namespace: self.namespace.clone(),
            execution: Some(execution(workflow_id)),
            wait_new_event,
            history_event_filter_type: HistoryEventFilterType::CloseEvent as i32,
            ..Default::default()
        };

        let mut client = self.inner.clone();
        let call = client.get_workflow_execution_history(request);

        let response = if wait_new_event {
            // 加上上限。Temporal 的 long poll 本身有伺服器端逾時，
            // 但網路中斷時呼叫端可能永遠等下去。
            match tokio::time::timeout(wait, call).await {
                Ok(r) => r?.into_inner(),
                Err(_) => return Ok(WorkflowResult::Running),
            }
        } else {
            call.await?.into_inner()
        };

        let Some(event) = response.history.and_then(|h| h.events.into_iter().next_back()) else {
            return Ok(WorkflowResult::Running);
        };

        Ok(match event.attributes {
            Some(Attributes::WorkflowExecutionCompletedEventAttributes(a)) => {
                WorkflowResult::Completed(payload::decode_first(a.result.as_ref()))
            }
            Some(Attributes::WorkflowExecutionFailedEventAttributes(a)) => {
                WorkflowResult::Failed(a.failure.map(|f| f.message).unwrap_or_default())
            }
            Some(Attributes::WorkflowExecutionCanceledEventAttributes(_)) => {
                WorkflowResult::Cancelled
            }
            Some(Attributes::WorkflowExecutionTerminatedEventAttributes(a)) => {
                WorkflowResult::Terminated(a.reason)
            }
            Some(Attributes::WorkflowExecutionTimedOutEventAttributes(_)) => {
                WorkflowResult::TimedOut
            }
            _ => WorkflowResult::Running,
        })
    }

    /// 取消流程
    ///
    /// 這是「請求取消」而非強制終止。Workflow 收到取消後仍能
    /// 執行清理（取消待辦、寫入稽核），這正是我們要的行為。
    /// 強制終止用 TerminateWorkflowExecution，會跳過清理。
    ///
    /// 因此取消後流程的最終狀態可能是 COMPLETED 而非 CANCELLED：
    /// Workflow 攔下 CancelledError 並正常回傳時即是如此。
    /// 這是正確行為，不是漏接取消。
    pub async fn cancel(&self, workflow_id: &str, reason: &str) -> Result<()> {
        let request = RequestCancelWorkflowExecutionRequest {
            namespace: self.namespace.clone(),
            workflow_execution: Some(execution(workflow_id)),
            reason: reason.to_string(),
            request_id: Uuid::new_v4().to_string(),
            ..Default::default()
        };

        self.inner
            .clone()
            .request_cancel_workflow_execution(request)
            .await?;
        Ok(())
    }
}

fn execution(workflow_id: &str) -> proto::temporal::api::common::v1::WorkflowExecution {
    proto::temporal::api::common::v1::WorkflowExecution {
        workflow_id: workflow_id.to_string(),
        run_id: String::new(),
    }
}
