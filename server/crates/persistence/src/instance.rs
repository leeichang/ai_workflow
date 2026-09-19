//! 流程實例的持久化
//!
//! Temporal 是流程狀態的真相，這裡存的是業務視角的投影。
//! 兩者可能短暫不一致（Temporal 已結束、投影還沒更新），
//! 因此查詢流程「當下狀態」要問 Temporal，查歷史與清單走這裡。

use crate::{Result, TenantTx};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct WorkflowInstance {
    pub id: Uuid,
    pub workflow_version_id: Uuid,
    /// 建立當下的表單版本。既有實例與沒有已發布表單的業務物件為 None
    pub form_version_id: Option<Uuid>,
    pub business_object: String,
    pub business_key: String,
    pub temporal_workflow_id: String,
    pub temporal_run_id: Option<String>,
    pub status: String,
    pub input: Value,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub started_by: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// 需要人介入。流程仍在跑，不是失敗狀態
    ///
    /// 與 `status` 正交：status 是生命週期，這是「跑得下去但不對勁」。
    /// 塞進 status 的話「異常但仍在跑」就沒辦法表達了。
    pub needs_attention: bool,
    /// 機器可讀的原因代碼（ESCALATE_UNRESOLVED 等）
    pub attention_code: Option<String>,
    pub attention_detail: Option<String>,
    pub attention_at: Option<DateTime<Utc>>,
}

/// 查詢欄位清單
///
/// 做成常數：四個查詢要列同一組欄位，先前加 `form_version_id`
/// 時就漏過一處，直到 decode 失敗才發現。
const COLUMNS: &str = "id, workflow_version_id, form_version_id, \
     business_object, business_key, \
     temporal_workflow_id, temporal_run_id, status, input, output, \
     error, started_by, started_at, ended_at, \
     needs_attention, attention_code, attention_detail, attention_at";

#[derive(Debug, Deserialize)]
pub struct CreateInstance {
    pub workflow_version_id: Uuid,
    /// 建立當下的表單版本。業務物件沒有已發布的表單時為 None
    pub form_version_id: Option<Uuid>,
    pub business_object: String,
    pub business_key: String,
    pub temporal_workflow_id: String,
    pub temporal_run_id: String,
    pub input: Value,
}

/// 建立實例
///
/// 刻意在啟動 Temporal 之前寫入，而非之後：
/// 先寫 DB 再啟動，失敗時留下一筆 RUNNING 但實際沒跑的孤兒紀錄，
/// 可由對帳程序清掉；反過來先啟動再寫 DB，失敗時流程已在跑
/// 但系統完全不知道它存在，沒有任何線索可追。
/// 寧可留孤兒，不可留幽靈。
pub async fn create(
    tx: &mut TenantTx<'_>,
    input: CreateInstance,
    started_by: Uuid,
) -> Result<WorkflowInstance> {
    let row = sqlx::query_as::<_, WorkflowInstance>(&format!(
        r#"
        insert into workflow_instance (
            tenant_id, workflow_version_id, form_version_id,
            business_object, business_key,
            temporal_workflow_id, temporal_run_id, input, started_by
        )
        values (current_setting('app.tenant_id')::uuid, $1, $2, $3, $4, $5, $6, $7, $8)
        returning {COLUMNS}
        "#
    ))
    .bind(input.workflow_version_id)
    .bind(input.form_version_id)
    .bind(&input.business_object)
    .bind(&input.business_key)
    .bind(&input.temporal_workflow_id)
    .bind(&input.temporal_run_id)
    .bind(&input.input)
    .bind(started_by)
    .fetch_one(tx.executor())
    .await?;

    Ok(row)
}

pub async fn find_by_id(tx: &mut TenantTx<'_>, id: Uuid) -> Result<WorkflowInstance> {
    let row = sqlx::query_as::<_, WorkflowInstance>(&format!(
        "select {COLUMNS} from workflow_instance where id = $1"
    ))
    .bind(id)
    .fetch_optional(tx.executor())
    .await?
    .ok_or_else(|| crate::Error::not_found("workflow_instance", id))?;

    Ok(row)
}

#[derive(Debug, Default)]
pub struct ListFilter {
    pub business_object: Option<String>,
    pub status: Option<String>,
    pub limit: i64,
    /// 只列出這個人看得到的單。`None` = 不設限（監控角色）。
    ///
    /// 刻意做成 Option 而非 bool + user_id：呼叫端必須明確表態
    /// 「不設限」，漏傳會變成看得到全部，那正是要避免的方向。
    pub visible_to: Option<Uuid>,
    /// 只列需要人介入的。監控頁的「只看異常」用它
    pub needs_attention: bool,
}

pub async fn list(tx: &mut TenantTx<'_>, filter: ListFilter) -> Result<Vec<WorkflowInstance>> {
    let limit = filter.limit.clamp(1, 200);

    // 可見範圍：RLS 只隔離租戶，同租戶的人預設看得到彼此的單。
    // 流程監控要「一般使用者看不到別人的單」，因此這裡再收一層。
    //
    // 看得到的條件（`visible_to` 為 None 時不設限，給監控角色用）：
    //   1. 自己發起的
    //   2. 自己有（或曾有）待辦的——那張單本來就在你的收件匣裡，
    //      看得到它的存在不算新增洩漏
    //
    // 排序把異常排最前面：監控頁的用途是找出要處理的單，
    // 讓它們沉在第三頁等於這個功能沒做。
    let rows = sqlx::query_as::<_, WorkflowInstance>(&format!(
        r#"
        select {COLUMNS}
        from workflow_instance i
        where ($1::text is null or business_object = $1)
          and ($2::text is null or status = $2)
          and ($4::uuid is null
               or started_by = $4
               or exists (select 1 from human_task t
                          where t.instance_id = i.id
                            and t.assignee_user_id = $4))
          and ($5::boolean is not true or needs_attention)
        order by needs_attention desc, started_at desc
        limit $3
        "#
    ))
    .bind(filter.business_object.as_deref())
    .bind(filter.status.as_deref())
    .bind(limit)
    .bind(filter.visible_to)
    .bind(filter.needs_attention)
    .fetch_all(tx.executor())
    .await?;

    Ok(rows)
}

/// 補上 Temporal 的 run_id
///
/// 建立時還不知道 run_id（要啟動後才有），啟動成功再回填。
pub async fn set_run_id(tx: &mut TenantTx<'_>, id: Uuid, run_id: &str) -> Result<()> {
    sqlx::query("update workflow_instance set temporal_run_id = $2 where id = $1")
        .bind(id)
        .bind(run_id)
        .execute(tx.executor())
        .await?;
    Ok(())
}

/// 標記需要人介入
///
/// 流程**不會**因此停下。原簽核人照樣能簽，只是監控頁會紅字標出。
/// 這是 N3 的定案：改成失敗會讓已經跑到一半的流程死掉，
/// 而原簽核人本來還簽得動。
///
/// 已經是異常的再標一次會覆寫原因。後發生的異常比較接近當下，
/// 而且流程只有一個「現在卡在哪」——保留最舊的沒有意義。
pub async fn mark_attention(
    tx: &mut TenantTx<'_>,
    id: Uuid,
    code: &str,
    detail: &str,
) -> Result<bool> {
    let affected = sqlx::query(
        r#"
        update workflow_instance
        set needs_attention = true,
            attention_code = $2,
            attention_detail = $3,
            attention_at = now()
        where id = $1 and status = 'RUNNING'
        "#,
    )
    .bind(id)
    .bind(code)
    .bind(detail)
    .execute(tx.executor())
    .await?
    .rows_affected();

    Ok(affected > 0)
}

/// 解除異常標記
///
/// 人處理完之後由監控頁呼叫。刻意不自動解除：
/// 「解析不到加簽對象」修好組織資料後不會有任何事件回來通知，
/// 自動解除只能靠輪詢重試，而重試的時機無從判定。
/// 由處理的人按下「已處理」，順帶留下誰處理的稽核。
pub async fn clear_attention(tx: &mut TenantTx<'_>, id: Uuid) -> Result<bool> {
    let affected = sqlx::query(
        r#"
        update workflow_instance
        set needs_attention = false,
            attention_code = null,
            attention_detail = null,
            attention_at = null
        where id = $1 and needs_attention
        "#,
    )
    .bind(id)
    .execute(tx.executor())
    .await?
    .rows_affected();

    Ok(affected > 0)
}

/// 更新最終狀態
///
/// 只允許從 RUNNING 轉出。流程結束後再收到結束事件是可能的
/// （Temporal 重送、對帳程序重跑），不該覆蓋已記錄的結果。
pub async fn finish(
    tx: &mut TenantTx<'_>,
    id: Uuid,
    status: &str,
    output: Option<Value>,
    error: Option<String>,
) -> Result<bool> {
    let affected = sqlx::query(
        r#"
        update workflow_instance
        set status = $2, output = $3, error = $4, ended_at = now(),
            -- 結束的流程不需要人介入了。不清掉的話監控頁會一直
            -- 把已完成的單排在最前面，真正該處理的反而被擠下去
            needs_attention = false,
            attention_code = null,
            attention_detail = null,
            attention_at = null
        where id = $1 and status = 'RUNNING'
        "#,
    )
    .bind(id)
    .bind(status)
    .bind(output)
    .bind(error)
    .execute(tx.executor())
    .await?
    .rows_affected();

    Ok(affected > 0)
}
