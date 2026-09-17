/**
 * 待辦 API
 *
 * 對應 server/crates/http-public/src/tasks.rs
 */

import { request } from './client'

/** 待辦。欄位對齊 persistence::human_task::HumanTask 加上單號 */
export interface HumanTask {
  id: string
  instance_id: string
  node_id: string
  node_label: string | null
  assignee_user_id: string | null
  assignee_role: string | null
  participant_kind: string
  form_key: string | null
  status: string
  decision: string | null
  comment: string | null
  due_at: string | null
  created_at: string
  decided_at: string | null
  /** 所屬流程的業務單號 */
  business_key: string | null
  business_object: string | null
}

export interface ListTasksQuery {
  status?: string
  limit?: number
}

/**
 * 收件匣
 *
 * 後端會依 user_id 與角色過濾，前端不需要也不應該自己篩——
 * 「我能看到哪些待辦」是授權問題。
 */
export function list(query: ListTasksQuery = {}): Promise<HumanTask[]> {
  const params = new URLSearchParams({
    status: query.status ?? 'PENDING',
  })
  if (query.limit !== undefined) {
    params.set('limit', String(query.limit))
  }
  return request<HumanTask[]>(`/tasks?${params.toString()}`)
}

export interface DecisionInput {
  decision: 'APPROVE' | 'REJECT'
  comment?: string
}

export interface DecisionResult {
  task_id: string
  decision: string
  /** 流程是否已收到 Signal */
  signalled: boolean
}

/**
 * 送出決策
 *
 * comment 預設空字串而非省略：後端以 #[serde(default)] 接收，
 * 省略雖然可行，但顯式送出讓「沒填意見」與「欄位漏掉」分得開。
 */
export function decide(
  taskId: string,
  input: DecisionInput,
): Promise<DecisionResult> {
  return request<DecisionResult>(`/tasks/${taskId}/decision`, {
    method: 'POST',
    body: { decision: input.decision, comment: input.comment ?? '' },
  })
}
