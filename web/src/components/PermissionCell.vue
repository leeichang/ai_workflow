<script setup lang="ts">
/**
 * 權限矩陣的一格
 *
 * 三態循環：可編輯 → 唯讀 → 隱藏 → 可編輯。
 * 系統鎖定的格子（計算欄位）不可點擊，顯示鎖頭圖示。
 */
import { computed } from 'vue'
import type { Cell, Permission } from '@/api/permissions'
import { nextPermission } from '@/api/permissions'

const props = defineProps<{
  cell: Cell
  /** 是否已被使用者修改，未儲存時顯示標記 */
  modified?: boolean
}>()

const emit = defineEmits<{ change: [Permission] }>()

const style = computed(() => {
  const map: Record<Permission, { bg: string; text: string; icon: string; label: string }> = {
    EDITABLE: {
      bg: 'bg-[#DCFCE7]',
      text: 'text-[#15803D]',
      icon: 'edit',
      label: '可編輯',
    },
    READONLY: {
      bg: 'bg-[#DBEAFE]',
      text: 'text-primary',
      icon: 'visibility',
      label: '唯讀',
    },
    HIDDEN: {
      bg: 'bg-surface-container-high',
      text: 'text-secondary',
      icon: 'visibility_off',
      label: '隱藏',
    },
  }
  return map[props.cell.permission]
})

function handleClick() {
  if (props.cell.locked) return
  emit('change', nextPermission(props.cell.permission))
}
</script>

<template>
  <button
    type="button"
    :disabled="cell.locked"
    :title="cell.reason"
    :data-testid="'perm-cell'"
    :data-permission="cell.permission"
    :data-locked="cell.locked"
    class="relative inline-flex items-center gap-1 px-2 py-1 rounded-lg font-body-dense text-body-dense transition-colors"
    :class="[
      style.bg,
      style.text,
      cell.locked ? 'opacity-70 cursor-not-allowed' : 'hover:brightness-95 cursor-pointer',
    ]"
    @click="handleClick"
  >
    <span class="material-symbols-outlined text-[14px]">
      {{ cell.locked ? 'lock' : style.icon }}
    </span>
    <span>{{ style.label }}</span>

    <span
      v-if="cell.required"
      class="text-error font-bold leading-none"
      title="必填"
      data-testid="required-mark"
    >*</span>

    <span
      v-if="modified"
      class="absolute -top-1 -right-1 w-2 h-2 rounded-full bg-primary-container"
      title="尚未儲存"
      data-testid="modified-mark"
    />
  </button>
</template>
