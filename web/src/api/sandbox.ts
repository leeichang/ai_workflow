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
}

export interface Participant {
  user_id: string
  name: string
  /** 由哪個角色解析出來的。讓使用者看得出「為什麼是這個人」 */
  role: string | null
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
