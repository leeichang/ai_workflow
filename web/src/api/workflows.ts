/**
 * 流程定義 API
 *
 * 端點與表單 API 對稱。差別在驗證回傳結構化的 GraphError，
 * 前端需要 node_id 來在畫布上標記出錯的節點。
 */

import { request } from './client'

// ── DSL 型別（對應 schemas/workflow-dsl.schema.json）─────

export type NodeType =
  | 'trigger'
  | 'condition'
  | 'human_approval'
  | 'human_task'
  | 'parallel'
  | 'join'
  | 'action'
  | 'notification'
  | 'end'

export type ResolverType =
  | 'user'
  | 'role'
  | 'department_manager'
  | 'manager_of'
  | 'initiator'
  | 'external_contacts'
  | 'expression'
  | 'composite'

export interface Resolver {
  type: ResolverType
  user_id?: string
  value?: string
  of?: string | ConditionalResolver[]
  path?: string
  cel?: string
}

export interface ConditionalResolver {
  spec: Resolver
  /** 條件為 false 時此子項不產生參與者 */
  when?: string
}

export interface JoinPolicy {
  completion: 'ALL' | 'ANY' | 'N_OF_M'
  n?: number
  result?: 'ALL_SUCCESS' | 'ANY_REJECT' | 'MAJORITY'
}

export interface TimeoutPolicy {
  /** ISO 8601 duration，例如 P3D、PT8H */
  after: string
  policy: 'WAIT' | 'ESCALATE' | 'AUTO_APPROVE' | 'AUTO_REJECT'
  to?: Resolver
  /**
   * 到期前多久發提醒，例如 ["P2D", "P1D", "PT6H"]
   *
   * 每個值是「距離到期還有多久」而非「從現在起算」。
   * 比 after 還長的值會被忽略——那個時間點在流程開始之前。
   */
  remind_at?: string[]
}

export interface RejectPolicy {
  action: 'goto' | 'end'
  /** action=goto 時必填，目標須在上游 */
  node?: string
  result?: 'rejected' | 'cancelled'
}

export interface RetryPolicy {
  max_attempts?: number
  backoff?: 'fixed' | 'exponential'
  initial_interval?: string
}

export interface WorkflowNode {
  id: string
  type: NodeType
  label?: string
  /** condition */
  expression?: string
  /** human_approval / human_task */
  participant?: 'internal' | 'external'
  resolver?: Resolver
  assignee?: Resolver
  form_key?: string
  join?: JoinPolicy
  timeout?: TimeoutPolicy
  on_reject?: RejectPolicy
  /** action */
  action?: string
  input_mapping?: Record<string, string>
  idempotency_key?: string
  retry?: RetryPolicy
  on_failure?: RejectPolicy
  /** notification */
  channel?: ('email' | 'line')[]
  to?: string[]
  template_key?: string
  /** end */
  result?: 'completed' | 'rejected' | 'cancelled'
}

export interface EdgeMeta {
  when?: boolean
  label?: string
}

/** [from, to] 或 [from, to, meta] */
export type WorkflowEdge = [string, string] | [string, string, EdgeMeta]

export interface Trigger {
  type: 'form_submit' | 'schedule' | 'webhook' | 'manual'
  cron?: string
  webhook_key?: string
}

export interface WorkflowContent {
  workflow_key: string
  version: number
  business_object: string
  name?: string
  description?: string
  trigger: Trigger
  nodes: WorkflowNode[]
  edges: WorkflowEdge[]
}

// ── API 型別 ────────────────────────────────────────────

export interface WorkflowDefinition {
  id: string
  workflow_key: string
  business_object: string
  name: string
  description: string | null
  created_at: string
  updated_at: string
}

export interface WorkflowVersion {
  id: string
  workflow_id: string
  version: number | null
  content: WorkflowContent
  status: 'DRAFT' | 'PUBLISHED' | 'ARCHIVED'
  created_by: string | null
  created_at: string
  updated_at: string
  published_by: string | null
  published_at: string | null
}

export interface WorkflowSummary extends WorkflowDefinition {
  published_version: number | null
  has_draft: boolean
}

export interface WorkflowDetail extends WorkflowDefinition {
  draft: WorkflowVersion | null
  published: WorkflowVersion | null
}

/** 圖結構驗證錯誤。node_id 供畫布標記出錯節點。 */
export interface GraphError {
  code: string
  node_id: string | null
  message: string
}

export interface ValidationResult {
  valid: boolean
  errors?: GraphError[]
}

export interface CreateWorkflowRequest {
  workflow_key: string
  business_object: string
  name: string
  description?: string
  content: WorkflowContent
}

// ── 端點 ────────────────────────────────────────────────

export function listWorkflows(): Promise<WorkflowSummary[]> {
  return request<WorkflowSummary[]>('/workflows')
}

export function getWorkflow(key: string): Promise<WorkflowDetail> {
  return request<WorkflowDetail>(`/workflows/${encodeURIComponent(key)}`)
}

export function createWorkflow(input: CreateWorkflowRequest): Promise<WorkflowDetail> {
  return request<WorkflowDetail>('/workflows', { method: 'POST', body: input })
}

export function saveWorkflowDraft(
  key: string,
  content: WorkflowContent,
): Promise<WorkflowVersion> {
  return request<WorkflowVersion>(`/workflows/${encodeURIComponent(key)}/draft`, {
    method: 'PUT',
    body: { content },
  })
}

export function discardWorkflowDraft(key: string): Promise<void> {
  return request<void>(`/workflows/${encodeURIComponent(key)}/draft`, { method: 'DELETE' })
}

/**
 * 驗證草稿
 *
 * 驗證失敗仍回 200，結果在 valid 欄位。前端需要逐條錯誤
 * 來標記節點，用 4xx 表達會讓處理變複雜。
 */
export function validateWorkflowDraft(key: string): Promise<ValidationResult> {
  return request<ValidationResult>(`/workflows/${encodeURIComponent(key)}/draft/validate`, {
    method: 'POST',
  })
}

export function publishWorkflowDraft(key: string): Promise<WorkflowVersion> {
  return request<WorkflowVersion>(`/workflows/${encodeURIComponent(key)}/draft/publish`, {
    method: 'POST',
  })
}

export function listWorkflowVersions(key: string): Promise<WorkflowVersion[]> {
  return request<WorkflowVersion[]>(`/workflows/${encodeURIComponent(key)}/versions`)
}
