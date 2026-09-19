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
}

export interface ListInstancesQuery {
  business_object?: string
  status?: string
  limit?: number
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
