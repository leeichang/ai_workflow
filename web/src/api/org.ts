/**
 * 組織
 *
 * 對應 server/crates/http-public/src/org.rs。
 *
 * 讀取只要登入，寫入要 admin——改主管等於改簽核路徑，
 * designer 管的是表單與流程定義，不是人事。
 */

import { request } from './client'

// ── 健康檢查 ────────────────────────────────────────────

/**
 * HIGH 代表流程會卡住，MEDIUM 代表功能受損但流程走得完。
 *
 * 只有兩級——分不出輕重的等級會讓人忽略整張報表。
 */
export type Severity = 'HIGH' | 'MEDIUM'

/**
 * 問題代碼
 *
 * 與後端 persistence::org_health::IssueCode 一對一。
 * 前端依代碼決定「前往修正」跳到哪一頁，所以要是列舉而非自由字串。
 */
export type IssueCode =
  | 'EMPLOYEE_NO_MANAGER'
  | 'MANAGER_INACTIVE'
  | 'MANAGER_CYCLE'
  | 'DEPARTMENT_NO_MANAGER'
  | 'DEPARTMENT_MANAGER_IS_SELF'
  | 'EMPLOYEE_NO_DEPARTMENT'
  | 'EMPLOYEE_NO_EMAIL'
  | 'DEPARTMENT_BROKEN_PARENT'

export interface OrgIssue {
  code: IssueCode
  severity: Severity
  /** employee 或 department */
  subject_type: string
  subject_id: string
  subject_name: string
  /** 後端產生的說明。點出後果而非現象 */
  message: string
  /** 補充資訊。例如離職主管的名字、循環長度 */
  detail?: string
}

export interface HealthReport {
  high_count: number
  medium_count: number
  /** 在職員工數。分母，讓「3 個問題」有比例可言 */
  employee_count: number
  department_count: number
  issues: OrgIssue[]
}

export function health(): Promise<HealthReport> {
  return request<HealthReport>('/org/health')
}

// ── 部門 ────────────────────────────────────────────────

export interface Department {
  id: string
  code: string
  name: string
  parent_id: string | null
  manager_user_id: string | null
  manager_name: string | null
  status: string
  sort_order: number
  source_system: string
  /** 平台維護的欄位。同步時一律跳過 */
  platform_managed_fields: string[]
  /** 直屬成員數（不含子部門） */
  member_count: number
}

export function listDepartments(): Promise<Department[]> {
  return request<Department[]>('/org/departments')
}

export interface NewDepartment {
  code: string
  name: string
  parent_id?: string | null
  manager_user_id?: string | null
  sort_order?: number
}

export function createDepartment(input: NewDepartment): Promise<{ id: string }> {
  return request<{ id: string }>('/org/departments', {
    method: 'POST',
    body: input,
  })
}

/**
 * 部門的編輯
 *
 * `fields` 列出這次要改哪些欄位，值從同名屬性取。
 * 不在 `fields` 裡的欄位一律不動，即使物件裡帶了值。
 *
 * 這個形狀是刻意的：單純用「有值就改」無法表達「把主管清成空」，
 * 而 `fields` 同時也是後端寫入 platform_managed_fields 的依據。
 */
export interface DepartmentPatch {
  fields: string[]
  name?: string
  parent_id?: string | null
  manager_user_id?: string | null
  status?: string
  sort_order?: number
}

export function updateDepartment(
  id: string,
  patch: DepartmentPatch,
): Promise<void> {
  return request<void>(`/org/departments/${id}`, {
    method: 'PATCH',
    body: patch,
  })
}

/**
 * 刪除部門
 *
 * 只刪得掉沒有成員也沒有子部門的部門——有成員的部門刪掉會讓那些人的
 * department_id 被靜默清空，依部門解析的簽核人全部失效。
 * 那種情況應該改成停用（`status: 'INACTIVE'`）。
 */
export function deleteDepartment(id: string): Promise<void> {
  return request<void>(`/org/departments/${id}`, { method: 'DELETE' })
}

// ── 員工 ────────────────────────────────────────────────

export interface Employee {
  id: string
  employee_no: string | null
  name: string
  email: string
  department_id: string | null
  department_name: string | null
  manager_id: string | null
  manager_name: string | null
  job_title: string | null
  phone: string | null
  extension: string | null
  hired_at: string | null
  left_at: string | null
  status: string
  can_login: boolean
  sync_status: string
  source_system: string
  platform_managed_fields: string[]
  roles: string[]
}

export interface EmployeeFilter {
  q?: string
  department_id?: string
  /** 預設只列在職者。要看離職的人必須明講 */
  include_inactive?: boolean
}

export function listEmployees(filter: EmployeeFilter = {}): Promise<Employee[]> {
  const qs = new URLSearchParams()
  // 空字串會讓後端把它當成有效的過濾條件，結果是查不到資料
  if (filter.q) qs.set('q', filter.q)
  if (filter.department_id) qs.set('department_id', filter.department_id)
  if (filter.include_inactive) qs.set('include_inactive', 'true')

  const suffix = qs.toString() === '' ? '' : `?${qs}`
  return request<Employee[]>(`/org/employees${suffix}`)
}

/** 員工的編輯。語意同 DepartmentPatch */
export interface EmployeePatch {
  fields: string[]
  name?: string
  employee_no?: string | null
  department_id?: string | null
  manager_id?: string | null
  job_title?: string | null
  phone?: string | null
  extension?: string | null
  hired_at?: string | null
  left_at?: string | null
  status?: string
}

export function updateEmployee(id: string, patch: EmployeePatch): Promise<void> {
  return request<void>(`/org/employees/${id}`, {
    method: 'PATCH',
    body: patch,
  })
}

// ── 欄位鎖定 ────────────────────────────────────────────

/**
 * 改回跟隨來源系統
 *
 * 解除後下一次同步就會覆蓋這些欄位——等同放棄平台上的人工修正，
 * 所以後端會留稽核。
 */
export function unlockFields(
  subject: 'employees' | 'departments',
  id: string,
  fields: string[],
): Promise<void> {
  return request<void>(`/org/${subject}/${id}/unlock-fields`, {
    method: 'POST',
    body: { fields },
  })
}
