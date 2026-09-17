/**
 * 角色清單的三態
 *
 * `editable_roles` 與 `readable_roles` 有三種語意不同的狀態，
 * 後端 domain/src/permission.rs 的 role_allowed() 據此判定：
 *
 *   鍵不存在 → 不限制，任何角色皆可
 *   []       → 明確拒絕所有角色
 *   ["a"]    → 僅角色 a
 *
 * 用逗號字串編輯會讓 `[]` 與「鍵不存在」都變成空字串，
 * 兩種相反的語意被壓成同一個輸入，使用者無從表達「拒絕所有人」。
 * 因此改為明確的三態選擇。
 */

export type RoleListMode = 'unrestricted' | 'deny_all' | 'allow_list'

export interface RoleListState {
  mode: RoleListMode
  /** 僅 mode = allow_list 時有意義 */
  roles: string[]
}

/** 從 DSL 的值判讀目前狀態 */
export function readRoleList(value: string[] | undefined): RoleListState {
  if (value === undefined) return { mode: 'unrestricted', roles: [] }
  if (value.length === 0) return { mode: 'deny_all', roles: [] }
  return { mode: 'allow_list', roles: [...value] }
}

/**
 * 轉回 DSL 的值
 *
 * 回傳 undefined 代表要移除這個鍵。呼叫端必須真的刪除，
 * 不能寫入 undefined，否則 JSON 序列化後的行為依實作而異。
 */
export function writeRoleList(state: RoleListState): string[] | undefined {
  switch (state.mode) {
    case 'unrestricted':
      return undefined
    case 'deny_all':
      return []
    case 'allow_list':
      return [...state.roles]
  }
}

/**
 * 切換清單中的單一角色
 *
 * 從 unrestricted 勾選第一個角色時自動轉為 allow_list；
 * 取消最後一個角色時轉為 deny_all 而非 unrestricted，
 * 因為使用者剛剛明確地把所有角色都取消了，意圖是拒絕而非放行。
 */
export function toggleRole(state: RoleListState, role: string): RoleListState {
  const set = new Set(state.mode === 'allow_list' ? state.roles : [])
  set.has(role) ? set.delete(role) : set.add(role)

  const roles = [...set]
  if (roles.length === 0) return { mode: 'deny_all', roles: [] }
  return { mode: 'allow_list', roles }
}

export const ROLE_LIST_MODES: { value: RoleListMode; label: string; hint: string }[] = [
  { value: 'unrestricted', label: '不限制', hint: '任何有此表單權限的角色皆可' },
  { value: 'allow_list', label: '指定角色', hint: '只有勾選的角色可以' },
  { value: 'deny_all', label: '全部拒絕', hint: '所有角色皆不可，通常搭配條件式規則' },
]
