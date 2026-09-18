/**
 * API 型別
 *
 * 對應 server/crates/http-public 的回應結構。
 * 後端改欄位時這裡必須同步，否則 MSW 測試會因契約不符而失敗。
 */

// ── 錯誤 ────────────────────────────────────────────────

/** 後端錯誤代碼。前端依此決定行為，不依賴 message 內容。 */
export type ErrorCode =
  | 'UNAUTHORIZED'
  | 'FORBIDDEN'
  | 'NOT_FOUND'
  | 'CONFLICT'
  | 'BAD_REQUEST'
  | 'VALIDATION_FAILED'
  | 'IMMUTABLE_PUBLISHED'
  | 'INTERNAL_ERROR'

export interface ApiErrorBody {
  code: ErrorCode
  message: string
  details?: { errors?: string[] }
}

/** API 錯誤。所有非 2xx 回應都轉成此型別拋出。 */
export class ApiError extends Error {
  readonly code: ErrorCode
  readonly status: number
  readonly details?: { errors?: string[] }

  constructor(status: number, body: ApiErrorBody) {
    super(body.message)
    this.name = 'ApiError'
    this.status = status
    this.code = body.code
    this.details = body.details
  }

  /** 驗證失敗時的逐條錯誤，供 UI 列出 */
  get validationErrors(): string[] {
    return this.details?.errors ?? []
  }

  /** 是否可重試。網路或伺服器暫時性問題才重試。 */
  get retryable(): boolean {
    return this.status >= 500 || this.status === 503
  }
}

// ── 認證 ────────────────────────────────────────────────

export interface LoginRequest {
  tenant_code: string
  email: string
  password: string
}

export interface UserInfo {
  id: string
  name: string
  email: string
  tenant_id: string
  roles: string[]
}

export interface LoginResponse {
  access_token: string
  user: UserInfo
}

// ── 表單定義 ────────────────────────────────────────────

export interface FormDefinition {
  id: string
  form_key: string
  business_object: string
  name: string
  created_at: string
  updated_at: string
}

export interface FormVersion {
  id: string
  form_id: string
  /** null 代表草稿 */
  version: number | null
  content: FormContent
  status: 'DRAFT' | 'PUBLISHED' | 'ARCHIVED'
  created_by: string | null
  created_at: string
  updated_at: string
  published_by: string | null
  published_at: string | null
}

export interface FormSummary extends FormDefinition {
  published_version: number | null
  has_draft: boolean
}

export interface FormDetail extends FormDefinition {
  draft: FormVersion | null
  published: FormVersion | null
}

export interface ValidationResult {
  valid: boolean
  errors?: string[]
  /**
   * 不影響 valid 的提醒
   *
   * 兩種：業務物件未定義、欄位路徑不在正式定義中。
   * 與 errors 分開——警告不擋下發布，但使用者要看得到。
   */
  warnings?: string[]
}

/** 發布的回應。可能帶著警告成功 */
export interface PublishResult extends FormVersion {
  warnings?: string[]
}

export interface CreateFormRequest {
  form_key: string
  business_object: string
  name: string
  content: FormContent
}

// ── 表單內容（對應 schemas/form.schema.json）─────────────

export type FieldComponent =
  | 'input' | 'textarea' | 'number' | 'select' | 'multi_select'
  | 'date' | 'datetime' | 'checkbox' | 'radio' | 'switch'
  | 'upload' | 'table' | 'reference' | 'user_picker' | 'department_picker'
  | 'display'

export type DataType =
  | 'string' | 'text' | 'integer' | 'decimal' | 'boolean'
  | 'date' | 'datetime' | 'uuid' | 'array' | 'file'

export interface SelectOption {
  value: string | number | boolean
  label: string
}

export interface TableColumn {
  key: string
  label: string
  component: 'input' | 'number' | 'select' | 'reference' | 'display' | 'date'
  width?: string
  align?: 'left' | 'center' | 'right'
  precision?: number
  readonly?: boolean
  /** 計算欄位，例如 row.qty * row.unit_price */
  formula?: string
  options?: SelectOption[]
  reference?: ReferenceSpec
}

export interface ReferenceSpec {
  source: string
  value_field?: string
  label_field?: string
  /** 選定後自動填入其他欄位 */
  fill?: Record<string, string>
}

export interface FieldUi {
  component: FieldComponent
  label: string
  placeholder?: string
  help?: string
  width?: number
  precision?: number
  prefix?: string
  suffix?: string
  options?: SelectOption[]
  /**
   * 選項綁定基本資料。與 options 二擇一
   *
   * 寫死的 options 維護不了組織異動——有人到職、部門改組時
   * 要回頭改每一張表單。
   */
  option_source?: OptionSourceSpec
  columns?: TableColumn[]
  reference?: ReferenceSpec
  /** false 時客戶 Portal 不顯示此欄位，例如成本與毛利 */
  external_visible?: boolean
}

/** 選項的資料來源。對應 GET /lookup/{source} */
export interface OptionSourceSpec {
  source: 'employee' | 'department' | 'role'
  /** 額外過濾條件，例如只列某部門的人 */
  filter?: Record<string, string>
  /** 選定後自動填入其他欄位。key 為目標欄位 key，value 為 extra 的屬性名 */
  fill?: Record<string, string>
}

export interface FieldData {
  path: string
  type: DataType
  item?: string
  default?: unknown
  min?: number
  max?: number
  max_length?: number
  pattern?: string
  /** 後端計算欄位，前端唯讀 */
  computed?: string
}

export interface FieldWorkflow {
  visible_when?: string
  readonly_when?: string
  required_when?: string
  editable_roles?: string[]
  readable_roles?: string[]
}

export interface FormField {
  key: string
  section?: string
  ui: FieldUi
  data: FieldData
  workflow?: FieldWorkflow
}

export interface FormSection {
  key: string
  title: string
  collapsible?: boolean
  visible_when?: string
}

export interface FormLayout {
  columns?: number
  label_width?: string
  label_position?: 'left' | 'top'
}

export interface FormContent {
  form_key: string
  version: number
  business_object: string
  name?: string
  layout?: FormLayout
  sections?: FormSection[]
  fields: FormField[]
}
