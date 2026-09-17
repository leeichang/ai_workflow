/**
 * 角色清單
 *
 * 與 server/seed-demo.sh 建立的角色一致。
 * 權限矩陣與表單設計器共用同一份，避免兩處各自維護而漂移。
 *
 * 目前是靜態清單。租戶自訂角色需要一支 /roles 端點，
 * 待權限管理畫面實作時再改為動態載入。
 */

export interface RoleOption {
  code: string
  label: string
  /** admin 不受欄位層級限制，UI 需標示以免設計者誤以為設定沒生效 */
  alwaysEditable?: boolean
}

export const ROLES: RoleOption[] = [
  { code: 'admin', label: '系統管理員', alwaysEditable: true },
  { code: 'designer', label: '流程設計者' },
  { code: 'approver', label: '簽核人員' },
  { code: 'requester', label: '申請人' },
  { code: 'viewer', label: '檢視者' },
]

export function roleLabel(code: string): string {
  return ROLES.find((r) => r.code === code)?.label ?? code
}
