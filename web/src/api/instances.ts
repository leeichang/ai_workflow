/**
 * 流程實例 API
 *
 * 對應 server/crates/http-public/src/instances.rs
 */

import { request } from './client'

export interface WorkflowInstance {
  id: string
  workflow_version_id: string
  business_object: string
  business_key: string
  temporal_workflow_id: string
  temporal_run_id: string | null
  status: string
  input: Record<string, unknown>
  output: Record<string, unknown> | null
  error: string | null
  started_by: string | null
  started_at: string
  ended_at: string | null
  /**
   * 需要人介入
   *
   * 與 `status` 正交：流程仍在跑（status 還是 RUNNING），
   * 只是有件事沒人處理會一直卡著。合併進 status 的話
   * 「異常但仍在跑」就沒辦法表達了。
   */
  needs_attention: boolean
  /** 機器可讀的原因。前端據此分支，不比對 detail 的中文 */
  attention_code: string | null
  attention_detail: string | null
  attention_at: string | null
}

export interface ListInstancesQuery {
  business_object?: string
  status?: string
  limit?: number
  /** 只看需要人介入的 */
  needs_attention?: boolean
}

export function list(
  query: ListInstancesQuery = {},
): Promise<WorkflowInstance[]> {
  const params = new URLSearchParams()
  if (query.business_object !== undefined) {
    params.set('business_object', query.business_object)
  }
  if (query.status !== undefined) {
    params.set('status', query.status)
  }
  if (query.limit !== undefined) {
    params.set('limit', String(query.limit))
  }
  // 只在 true 時帶上。後端是 `is not true` 判定，
  // 帶 false 沒有壞處但會讓網址多一段雜訊
  if (query.needs_attention) {
    params.set('needs_attention', 'true')
  }
  const qs = params.toString()
  return request<WorkflowInstance[]>(`/instances${qs ? `?${qs}` : ''}`)
}

/**
 * 單筆詳情
 *
 * `live_status` 是 Temporal 回報的即時狀態，與本地的 `status` 分開呈現：
 * 後者是投影，可能落後。**兩者不一致本身就是有用的訊號**，
 * 合併成一個欄位會把它藏起來。
 */
export interface InstanceDetail extends WorkflowInstance {
  live_status: string | null
}

export function getOne(id: string): Promise<InstanceDetail> {
  return request<InstanceDetail>(`/instances/${encodeURIComponent(id)}`)
}

export interface StartInstanceInput {
  workflow_key: string
  /** 業務單號。同一租戶內不可重複——Temporal 以此去重。 */
  business_key: string
  /**
   * 業務資料
   *
   * 結構必須與流程定義引用的路徑一致。例如條件式寫
   * `quotation.discount_rate > 0.15`，這裡就要有
   * `{ quotation: { discount_rate: 0.2 } }`——放在頂層的話
   * 取不到值會被當成 false，流程靜默少走一段。
   */
  input: Record<string, unknown>
}

export function start(input: StartInstanceInput): Promise<WorkflowInstance> {
  return request<WorkflowInstance>('/instances', {
    method: 'POST',
    body: input,
  })
}

// ── 流程監控的介入動作 ──────────────────────────────────
//
// 全部需要 admin 或 process_monitor。後端會擋，前端只是不顯示按鈕——
// 兩層都要有：只靠前端隱藏等於沒有權限控制。

export interface HumanTask {
  id: string
  instance_id: string
  node_id: string
  node_label: string | null
  assignee_user_id: string | null
  assignee_role: string | null
  status: string
  decision: string | null
  due_at: string | null
  created_at: string
}

/** 這筆流程卡在誰身上。流程狀態只說 RUNNING，卡在誰是待辦才知道 */
export function listTasks(instanceId: string): Promise<HumanTask[]> {
  return request<HumanTask[]>(
    `/instances/${encodeURIComponent(instanceId)}/tasks`,
  )
}

export interface RemindResult {
  notified: number
  /** 指派給角色的待辦定位不到個人信箱，催不到。要讓使用者知道 */
  skipped_role_tasks: number
  note?: string
}

/** 催辦：對還沒簽的人重寄提醒。卡住最常見的解法是催人，不是取消 */
export function remind(instanceId: string): Promise<RemindResult> {
  return request<RemindResult>(
    `/instances/${encodeURIComponent(instanceId)}/remind`,
    { method: 'POST' },
  )
}

/** 標記異常已處理。手動而非自動——修好組織資料不會有事件回來通知 */
export function resolveAttention(
  instanceId: string,
  note = '',
): Promise<{ resolved: boolean }> {
  return request<{ resolved: boolean }>(
    `/instances/${encodeURIComponent(instanceId)}/resolve-attention`,
    { method: 'POST', body: { note } },
  )
}

/** 改派待辦。會動到簽核責任歸屬，稽核事件型別與一般指派分開 */
export function reassignTask(
  taskId: string,
  toUserId: string,
  reason = '',
): Promise<{ reassigned: boolean }> {
  return request<{ reassigned: boolean }>(
    `/tasks/${encodeURIComponent(taskId)}/reassign`,
    { method: 'POST', body: { to_user_id: toUserId, reason } },
  )
}

export function cancel(
  instanceId: string,
  reason = '',
): Promise<{ requested: boolean }> {
  return request<{ requested: boolean }>(
    `/instances/${encodeURIComponent(instanceId)}/cancel`,
    { method: 'POST', body: { reason } },
  )
}
