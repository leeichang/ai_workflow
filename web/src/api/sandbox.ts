/**
 * 開發模式（沙箱）與模擬簽核
 *
 * 使用者要的三件事：
 *   1. 真的解析簽核人——用當前的角色與組織設定跑 resolver
 *   2. 一人扮演所有角色——不換身分登入，點選就切換扮演對象
 *   3. 看到那個人會看到的畫面——欄位可見性依被扮演者的角色重算
 *
 * 沙箱不是另一個環境，是一個附身模式。
 */

import { request } from './client'

export interface SandboxSession {
  id: string
  tenant_id: string
  sandbox_tenant_id: string | null
  created_by: string
  /** 建立者在沙箱租戶內的 id。授權比對的是這個 */
  created_by_in_sandbox: string | null
  status: string
  created_at: string
  expires_at: string
}

/**
 * 會簽進度
 *
 * 執行期不存這個（Temporal 自己持久化分支進度），
 * 後端從 DSL 推出結構、再用同實例的待辦狀態算出進度。
 */
export interface BranchProgress {
  parallel_id: string
  parallel_label: string | null
  join_id: string
  /** 這組會簽共有幾條分支 */
  branch_total: number
  /** 本節點所在分支的起點 */
  branch_id: string
  completion: 'ALL' | 'ANY' | 'N_OF_M'
  n: number | null
  /** 已完成幾條 */
  done: number
}

/** 沙箱裡待簽的項目與解析出的簽核人 */
export interface PendingTask {
  task_id: string
  instance_id: string
  business_key: string
  node_id: string
  node_label: string | null
  assignee_user_id: string | null
  assignee_name: string | null
  assignee_role: string | null
  /** 不在平行結構裡就是 null */
  branch: BranchProgress | null
  /** 節點設的逾時。沒設就是 null，也就沒有等待可以快轉 */
  timeout: NodeTimeout | null
}

/** 節點的逾時設定 */
export interface NodeTimeout {
  /** ISO 8601 duration，例如 P2D */
  after: string
  policy: 'WAIT' | 'AUTO_APPROVE' | 'AUTO_REJECT' | 'ESCALATE'
}

export interface SkipTimeResult {
  task_id: string
  node_id: string
  policy: string
  after: string
}

export interface Participant {
  user_id: string
  name: string
  /** 由哪個角色解析出來的。讓使用者看得出「為什麼是這個人」 */
  role: string | null
}

/** 某個欄位在被扮演者眼中的權限 */
export interface FieldPermission {
  key: string
  permission: 'EDITABLE' | 'READONLY' | 'HIDDEN'
  required: boolean
  /** 為何是這個結果。讓使用者看得出「為什麼這個欄位看不到」 */
  reason: string
}

export interface SimulatedForm {
  form_key: string
  /** 被扮演者的角色——不是測試者的 */
  acting_as_roles: string[]
  acting_as_name: string | null
  fields: FieldPermission[]
}

export interface SimulateResult {
  task_id: string
  acting_as: string
  decision: string
}

export function listSandboxes(): Promise<SandboxSession[]> {
  return request<SandboxSession[]>('/sandboxes')
}

export function createSandbox(): Promise<SandboxSession> {
  return request<SandboxSession>('/sandboxes', { method: 'POST' })
}

export function retireSandbox(id: string): Promise<{ retired: boolean }> {
  return request<{ retired: boolean }>(
    `/sandboxes/${encodeURIComponent(id)}/retire`,
    { method: 'POST' },
  )
}

/**
 * 沙箱裡所有待簽的項目
 *
 * 不用 `/tasks`——那是收件匣，只列出指派給自己的。
 * 模擬的前提正是測試者不是 assignee，用收件匣會永遠是空的。
 */
export function listSandboxTasks(sandboxId: string): Promise<PendingTask[]> {
  return request<PendingTask[]>(
    `/sandboxes/${encodeURIComponent(sandboxId)}/tasks`,
  )
}

/** 以真實 resolver 解析某角色的簽核人 */
export function resolveParticipants(
  sandboxId: string,
  role: string,
): Promise<Participant[]> {
  return request<Participant[]>(
    `/sandboxes/${encodeURIComponent(sandboxId)}/participants/${encodeURIComponent(role)}`,
  )
}

/**
 * 以被扮演者的身分看這張單的欄位權限
 *
 * 用的是被扮演者的角色與實例的真實資料——
 * 傳測試者的角色或空資料都會讓畫面與真實簽核不同，
 * 而沙箱的整個價值就是兩者要一致。
 */
export function getSimulatedForm(
  sandboxId: string,
  taskId: string,
): Promise<SimulatedForm> {
  return request<SimulatedForm>(
    `/sandboxes/${encodeURIComponent(sandboxId)}/tasks/${encodeURIComponent(taskId)}/form`,
  )
}

/**
 * 時間快轉
 *
 * 含 P2D、P7D 的流程在模擬時原本要等兩天、七天才看得到逾時行為。
 *
 * **不假造決策**——後端讓等待立刻視為到期，跑的是真正的逾時處理。
 * 假造一個 APPROVE 會讓模擬顯示「通過了」，而正式環境設的
 * AUTO_REJECT 其實是退回。
 */
export function skipTime(
  sandboxId: string,
  taskId: string,
): Promise<SkipTimeResult> {
  return request<SkipTimeResult>(
    `/sandboxes/${encodeURIComponent(sandboxId)}/tasks/${encodeURIComponent(taskId)}/skip-time`,
    { method: 'POST' },
  )
}

/**
 * 以扮演的身分做決策
 *
 * 稽核會同時記真正按下按鈕的人與扮演對象。
 */
export function simulateDecision(
  sandboxId: string,
  taskId: string,
  actingAs: string,
  decision: 'APPROVE' | 'REJECT',
  comment = '',
): Promise<SimulateResult> {
  return request<SimulateResult>(
    `/sandboxes/${encodeURIComponent(sandboxId)}/tasks/${encodeURIComponent(taskId)}/simulate`,
    { method: 'POST', body: { acting_as: actingAs, decision, comment } },
  )
}
