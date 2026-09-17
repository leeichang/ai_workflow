/**
 * 權限預覽
 *
 * 欄位的實際權限由六條規則依序判定（見後端 domain/src/permission.rs
 * 的 compute()），設計者很難從四個獨立的輸入框推出結果。
 * 因此直接問後端：某角色在某節點上，這張表單會長什麼樣。
 *
 * 用後端而非前端重算的理由：判定邏輯只該有一份。
 * 前端複製一份必然會與後端漂移，而漂移的症狀是「畫面說可編輯、
 * 實際送出被擋」，極難追查。
 */

import { ref, shallowRef } from 'vue'
import { previewPermissions, type FieldPermission } from '@/api/permissions'
import { ApiError } from '@/api/types'

export interface PreviewScenario {
  nodeId?: string
  roles: string[]
  participantKind: 'internal' | 'external'
}

export function usePermissionPreview(formKey: string) {
  const result = shallowRef<FieldPermission[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  /** 預覽是否反映最新的草稿。存檔前的修改後端看不到。 */
  const stale = ref(false)

  /** 序號，避免慢的請求覆蓋快的結果 */
  let seq = 0

  async function run(scenario: PreviewScenario) {
    const mine = ++seq
    loading.value = true
    error.value = null

    try {
      const data = await previewPermissions(formKey, {
        nodeId: scenario.nodeId,
        roles: scenario.roles,
        participantKind: scenario.participantKind,
      })

      // 期間已有新的請求發出，丟棄這次的結果
      if (mine !== seq) return

      result.value = data
      stale.value = false
    } catch (e) {
      if (mine !== seq) return
      error.value = e instanceof ApiError ? e.message : '預覽失敗'
      result.value = []
    } finally {
      if (mine === seq) loading.value = false
    }
  }

  /** 草稿有未存檔的修改時呼叫，提示預覽結果已過時 */
  function markStale() {
    if (result.value.length > 0) stale.value = true
  }

  function clear() {
    seq++
    result.value = []
    error.value = null
    stale.value = false
  }

  return { result, loading, error, stale, run, markStale, clear }
}

/** 權限的顯示樣式，與權限矩陣的色票一致 */
export const PERMISSION_STYLE: Record<
  string,
  { label: string; bg: string; text: string; icon: string }
> = {
  EDITABLE: {
    label: '可編輯',
    bg: 'bg-[#DCFCE7]',
    text: 'text-[#15803D]',
    icon: 'edit',
  },
  READONLY: {
    label: '唯讀',
    bg: 'bg-[#FEF3C7]',
    text: 'text-[#B45309]',
    icon: 'visibility',
  },
  HIDDEN: {
    label: '隱藏',
    bg: 'bg-surface-container-high',
    text: 'text-secondary',
    icon: 'visibility_off',
  },
}
