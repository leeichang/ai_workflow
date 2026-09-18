/**
 * 表單定義 API
 *
 * 對應 server/crates/http-public/src/forms.rs 的端點。
 */

import { request } from './client'
import type {
  CreateFormRequest,
  FormContent,
  FormDetail,
  FormSummary,
  FormVersion,
  PublishResult,
  ValidationResult,
} from './types'

export function listForms(): Promise<FormSummary[]> {
  return request<FormSummary[]>('/forms')
}

export function getForm(formKey: string): Promise<FormDetail> {
  return request<FormDetail>(`/forms/${encodeURIComponent(formKey)}`)
}

export function createForm(input: CreateFormRequest): Promise<FormDetail> {
  return request<FormDetail>('/forms', { method: 'POST', body: input })
}

export function saveDraft(formKey: string, content: FormContent): Promise<FormVersion> {
  return request<FormVersion>(`/forms/${encodeURIComponent(formKey)}/draft`, {
    method: 'PUT',
    body: { content },
  })
}

export function discardDraft(formKey: string): Promise<void> {
  return request<void>(`/forms/${encodeURIComponent(formKey)}/draft`, { method: 'DELETE' })
}

/**
 * 驗證草稿但不發布
 *
 * 讓使用者在按下發布前先看到問題，而不是按了才失敗。
 * 注意：驗證失敗時仍回 200，結果在 body 的 valid 欄位。
 */
export function validateDraft(formKey: string): Promise<ValidationResult> {
  return request<ValidationResult>(`/forms/${encodeURIComponent(formKey)}/draft/validate`, {
    method: 'POST',
  })
}

export function publishDraft(formKey: string): Promise<PublishResult> {
  return request<PublishResult>(`/forms/${encodeURIComponent(formKey)}/draft/publish`, {
    method: 'POST',
  })
}

export function listVersions(formKey: string): Promise<FormVersion[]> {
  return request<FormVersion[]>(`/forms/${encodeURIComponent(formKey)}/versions`)
}

export function getVersion(formKey: string, version: number): Promise<FormVersion> {
  return request<FormVersion>(`/forms/${encodeURIComponent(formKey)}/versions/${version}`)
}
