/**
 * 組織資料同步
 *
 * 對應 server/crates/http-public/src/integration.rs。
 *
 * 本輪是 P1：CSV 匯入。流程是
 *   analyze（上傳看表頭）→ 確認欄位對應 → preview（試跑）→ sync（寫入）
 *
 * preview 不是加分項。沒有試跑，管理員第一次同步就是盲賭。
 */

import { request } from './client'

// ── 欄位對應建議 ────────────────────────────────────────

export interface MappingSuggestion {
  /** 來源的表頭，原樣保留 */
  source_field: string
  /** 建議對應到哪個 canonical 欄位。null 代表猜不出來 */
  canonical_field: string | null
  label: string | null
  /** exact 完全相符、synonym 同義詞、contains 包含、none 猜不出來 */
  confidence: string
}

export interface AnalyzeResult {
  headers: string[]
  row_count: number
  /** 前 5 列。回整份等於把全公司個資塞進一個 API 回應 */
  sample_rows: string[][]
  suggestions: MappingSuggestion[]
}

export function analyze(
  content: string,
  dataset: string,
): Promise<AnalyzeResult> {
  return request<AnalyzeResult>('/integration/analyze', {
    method: 'POST',
    body: { content, dataset },
  })
}

// ── 連線與來源 ──────────────────────────────────────────

export interface Connection {
  id: string
  name: string
  kind: string
  enabled: boolean
}

export function listConnections(): Promise<Connection[]> {
  return request<Connection[]>('/integration/connections')
}

export function createConnection(
  name: string,
  kind = 'FILE',
): Promise<{ id: string }> {
  return request<{ id: string }>('/integration/connections', {
    method: 'POST',
    body: { name, kind },
  })
}

export interface Source {
  id: string
  connection_id: string
  dataset: string
  name: string
  /** 來源欄位名 → canonical 欄位名 */
  mapping_definition: Record<string, string>
  /** 完整性閘的門檻。低於上次成功筆數 × 此值即整批中止 */
  completeness_threshold: number
  /** null 代表從未成功同步過，完整性閘不生效 */
  last_success_count: number | null
  last_synced_at: string | null
}

/**
 * 列出來源
 *
 * 匯入精靈靠它重用既有的來源。每次都建新的會撞唯一約束，
 * 而且新來源沒有 last_success_count，完整性閘會永遠不生效。
 */
export function listSources(): Promise<Source[]> {
  return request<Source[]>('/integration/sources')
}

export function getSource(id: string): Promise<Source> {
  return request<Source>(`/integration/sources/${id}`)
}

export interface NewSource {
  connection_id: string
  dataset: string
  name: string
  mapping: Record<string, string>
}

export function createSource(input: NewSource): Promise<{ id: string }> {
  return request<{ id: string }>('/integration/sources', {
    method: 'POST',
    body: input,
  })
}

/**
 * 刪除來源
 *
 * 換了人事系統、欄位對應完全不同時，重新設定比逐欄改容易。
 * 同步歷史會跟著刪掉——它的意義是「這個來源同步過什麼」，
 * 來源沒了之後那些計數對不上任何東西。稽核事件不受影響。
 */
export function deleteSource(id: string): Promise<void> {
  return request<void>(`/integration/sources/${id}`, { method: 'DELETE' })
}

export function updateSource(
  id: string,
  patch: { mapping?: Record<string, string>; completeness_threshold?: number },
): Promise<void> {
  return request<void>(`/integration/sources/${id}`, {
    method: 'PATCH',
    body: patch,
  })
}

// ── 同步 ────────────────────────────────────────────────

/**
 * §10.3 的計數契約
 *
 * 每一個都要有——同步不能只回「success」，管理員要看得出
 * 兩邊差異有多大。
 */
export interface SyncCounts {
  received_count: number
  parsed_count: number
  mapped_count: number
  validated_count: number
  inserted_count: number
  updated_count: number
  unchanged_count: number
  /** 因平台維護而跳過的**欄位**數，不是筆數 */
  skipped_by_ownership_count: number
  rejected_count: number
  missing_in_source_count: number
}

export type SyncStatus =
  | 'SUCCESS'
  | 'PARTIAL_SUCCESS'
  /** 完整性閘擋下。整批中止，未寫入任何變更 */
  | 'ABORTED_INCOMPLETE'
  | 'FAILED'

export interface SyncIssue {
  /** 來源的第幾列。null 代表這個問題不對應到特定的列 */
  row_number: number | null
  kind:
    | 'AMBIGUOUS'
    | 'VALIDATION_FAILED'
    | 'SKIPPED_BY_OWNERSHIP'
    | 'MISSING_IN_SOURCE'
  message: string
  field: string | null
}

export interface SyncOutcome {
  status: SyncStatus
  counts: SyncCounts
  issues: SyncIssue[]
  /** 中止或失敗的原因 */
  message: string | null
}

/**
 * 試跑
 *
 * 完全不寫入，但計數照算。管理員要能先看到
 * 「會新增 12 筆、更新 340 筆、跳過 8 個欄位」。
 */
export function preview(
  sourceId: string,
  content: string,
): Promise<SyncOutcome> {
  return request<SyncOutcome>(`/integration/sources/${sourceId}/preview`, {
    method: 'POST',
    body: { content },
  })
}

export function sync(
  sourceId: string,
  content: string,
): Promise<SyncOutcome> {
  return request<SyncOutcome>(`/integration/sources/${sourceId}/sync`, {
    method: 'POST',
    body: { content },
  })
}

// ── 歷史 ────────────────────────────────────────────────

export interface SyncRun {
  id: string
  source_id: string
  status: SyncStatus
  dry_run: boolean
  received_count: number
  validated_count: number
  inserted_count: number
  updated_count: number
  unchanged_count: number
  skipped_by_ownership_count: number
  rejected_count: number
  missing_in_source_count: number
  message: string | null
  started_at: string
  finished_at: string | null
}

export function listRuns(): Promise<SyncRun[]> {
  return request<SyncRun[]>('/integration/sync-runs')
}

export interface SyncIssueRow extends SyncIssue {
  id: string
}

export function listIssues(runId: string): Promise<SyncIssueRow[]> {
  return request<SyncIssueRow[]>(`/integration/sync-runs/${runId}/issues`)
}
