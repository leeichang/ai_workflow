/**
 * 基本資料查詢
 *
 * 對應 server/crates/http-public/src/lookup.rs。
 *
 * 表單的下拉選單綁定組織資料時用這支——選部門、選人員、選角色。
 * 寫死的 options 維護不了組織異動（有人到職、部門改組），
 * 綁資料來源才能跟著變。
 */

import { request } from './client'

/** 可用的資料來源 */
export interface LookupSource {
  /** 表單定義裡填的值 */
  name: string
  /** 給設計者看的中文名 */
  label: string
  /** 這個來源提供哪些額外屬性，供 option_source.fill 使用 */
  extra_fields: string[]
}

/** 下拉選單的一個選項 */
export interface LookupItem {
  value: string
  label: string
  /** 額外屬性。選定後可自動填入其他欄位 */
  extra?: Record<string, unknown>
}

export interface LookupParams {
  /** 關鍵字。下拉選單輸入時用 */
  q?: string
  /** 筆數上限。後端預設 50、最大 200 */
  limit?: number
}

export function listSources(): Promise<LookupSource[]> {
  return request<LookupSource[]>('/lookup/sources')
}

/**
 * 查詢某個來源
 *
 * 空參數不送進查詢字串——送 `?q=` 會讓後端把空字串當成有效的
 * 過濾條件，結果是查不到任何資料。
 */
export function query(
  source: string,
  params: LookupParams = {},
): Promise<LookupItem[]> {
  const qs = new URLSearchParams()
  if (params.q !== undefined && params.q !== '') qs.set('q', params.q)
  if (params.limit !== undefined) qs.set('limit', String(params.limit))

  const suffix = qs.toString() === '' ? '' : `?${qs}`
  return request<LookupItem[]>(
    `/lookup/${encodeURIComponent(source)}${suffix}`,
  )
}
