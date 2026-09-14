/**
 * 權限矩陣 API
 *
 * 對應 server/crates/http-public/src/permissions.rs。
 */

import { request } from './client'

export type Permission = 'EDITABLE' | 'READONLY' | 'HIDDEN'

/** 三態循環順序。UI 點擊儲存格時依此輪替。 */
export const PERMISSION_CYCLE: Permission[] = ['EDITABLE', 'READONLY', 'HIDDEN']

export function nextPermission(current: Permission): Permission {
  const i = PERMISSION_CYCLE.indexOf(current)
  return PERMISSION_CYCLE[(i + 1) % PERMISSION_CYCLE.length]
}

export interface Cell {
  permission: Permission
  required: boolean
  /** 判定依據，供 tooltip 顯示 */
  reason: string
  /** 系統鎖定（計算欄位），UI 不可點擊 */
  locked: boolean
}

export interface MatrixRow {
  key: string
  label: string
  data_path?: string
  section?: string
  /** false 時顯示「客戶看不到」標記 */
  external_visible?: boolean
  cells: Record<string, Cell>
}

export interface MatrixColumn {
  key: string
  label: string
  participant_kind: string
}

export interface SectionInfo {
  key: string
  title: string
  field_count: number
}

export interface MatrixResponse {
  form_key: string
  workflow_key?: string
  /** null 代表草稿 */
  version: number | null
  columns: MatrixColumn[]
  rows: MatrixRow[]
  sections: SectionInfo[]
}

export interface FieldPermission {
  key: string
  permission: Permission
  required: boolean
  reason: string
}

export type ViewMode = 'by_role' | 'by_node'

export interface MatrixParams {
  workflowKey?: string
  mode?: ViewMode
  role?: string
  nodeId?: string
  version?: number
}

export function getMatrix(formKey: string, params: MatrixParams = {}): Promise<MatrixResponse> {
  const q = new URLSearchParams()
  if (params.workflowKey) q.set('workflow_key', params.workflowKey)
  if (params.mode) q.set('mode', params.mode)
  if (params.role) q.set('role', params.role)
  if (params.nodeId) q.set('node_id', params.nodeId)
  if (params.version !== undefined) q.set('version', String(params.version))

  const qs = q.toString()
  return request<MatrixResponse>(
    `/forms/${encodeURIComponent(formKey)}/permissions${qs ? `?${qs}` : ''}`,
  )
}

export interface PreviewParams {
  nodeId?: string
  roles?: string[]
  participantKind?: 'internal' | 'external'
}

/**
 * 單一情境預覽
 *
 * 設計者調完規則後，確認「某角色在某節點看到什麼」。
 */
export function previewPermissions(
  formKey: string,
  params: PreviewParams = {},
): Promise<FieldPermission[]> {
  const q = new URLSearchParams()
  if (params.nodeId) q.set('node_id', params.nodeId)
  if (params.roles?.length) q.set('roles', params.roles.join(','))
  if (params.participantKind) q.set('participant_kind', params.participantKind)

  const qs = q.toString()
  return request<FieldPermission[]>(
    `/forms/${encodeURIComponent(formKey)}/permissions/preview${qs ? `?${qs}` : ''}`,
  )
}
