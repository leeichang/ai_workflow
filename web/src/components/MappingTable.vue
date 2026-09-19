<script setup lang="ts">
/**
 * 欄位對應確認表
 *
 * 需求 07 §8 的「UI 顯示『建議，請確認』，逐欄可改」。
 *
 * 建議只是省下打字的工夫，**人工確認才是關鍵**——對應一旦錯了，
 * 整批資料會寫進錯的欄位，而且同步成功、沒有任何錯誤訊息。
 * 所以每一欄都要看得到樣本值，管理員才判斷得出對應對不對。
 */
import { computed } from 'vue'
import type { MappingSuggestion } from '@/api/integration'

const props = defineProps<{
  headers: string[]
  suggestions: MappingSuggestion[]
  /** 前幾列的原始值，供管理員判斷 */
  sampleRows: string[][]
  /** 目前的對應：來源欄位 → canonical 欄位。'' 代表不匯入 */
  modelValue: Record<string, string>
  /** 可選的 canonical 欄位 */
  options: { value: string; label: string }[]
}>()

const emit = defineEmits<{ 'update:modelValue': [Record<string, string>] }>()

/** 每個來源欄位的前幾個樣本值 */
const samplesOf = computed(() => {
  const result: Record<string, string[]> = {}
  props.headers.forEach((header, index) => {
    result[header] = props.sampleRows
      .map((row) => row[index] ?? '')
      .filter((v) => v !== '')
      .slice(0, 3)
  })
  return result
})

/**
 * 被指派兩次的 canonical 欄位
 *
 * 兩欄對到同一個目標時，後面的會覆蓋前面的，而畫面上看不出來。
 * 建議階段已經避免了，但管理員手動改就可能撞上
 */
const duplicated = computed(() => {
  const seen = new Map<string, number>()
  for (const target of Object.values(props.modelValue)) {
    if (target === '') continue
    seen.set(target, (seen.get(target) ?? 0) + 1)
  }
  return new Set(
    [...seen.entries()].filter(([, count]) => count > 1).map(([key]) => key),
  )
})

const confidenceLabel: Record<string, string> = {
  exact: '完全相符',
  contains: '推測',
  none: '未對應',
}

function update(header: string, value: string): void {
  emit('update:modelValue', { ...props.modelValue, [header]: value })
}
</script>

<template>
  <table
    class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
    data-testid="mapping-table"
  >
    <thead>
      <tr class="bg-surface-container-low border-b border-outline-variant">
        <th
          class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary w-[30%]"
        >
          檔案的欄位
        </th>
        <th
          class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
        >
          資料範例
        </th>
        <th
          class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary w-[35%]"
        >
          對應到
        </th>
      </tr>
    </thead>
    <tbody>
      <tr
        v-for="(header, index) in headers"
        :key="header"
        :data-testid="`mapping-row-${index}`"
        class="border-b border-outline-variant last:border-b-0"
      >
        <td class="px-space-md py-space-sm align-top">
          <p class="font-body-dense text-body-dense text-on-surface">
            {{ header }}
          </p>
          <p
            class="font-label-caption text-label-caption mt-0.5"
            :class="
              suggestions[index]?.confidence === 'none'
                ? 'text-secondary'
                : 'text-[#15803D]'
            "
          >
            {{ confidenceLabel[suggestions[index]?.confidence ?? 'none'] }}
          </p>
        </td>
        <td class="px-space-md py-space-sm align-top">
          <p
            class="font-data-mono text-[11px] text-secondary truncate"
            :title="samplesOf[header]?.join('、')"
          >
            {{ samplesOf[header]?.join('、') || '（空白）' }}
          </p>
        </td>
        <td class="px-space-md py-space-sm align-top">
          <select
            :value="modelValue[header] ?? ''"
            :data-testid="`mapping-select-${index}`"
            class="w-full h-8 px-2 rounded-lg border bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
            :class="
              duplicated.has(modelValue[header] ?? '')
                ? 'border-error'
                : 'border-outline-variant'
            "
            @change="update(header, ($event.target as HTMLSelectElement).value)"
          >
            <option value="">不匯入這一欄</option>
            <option v-for="opt in options" :key="opt.value" :value="opt.value">
              {{ opt.label }}
            </option>
          </select>
          <!--
            兩欄對到同一個目標時，後面的會覆蓋前面的，
            而同步照樣成功、沒有錯誤訊息
          -->
          <p
            v-if="duplicated.has(modelValue[header] ?? '')"
            class="font-label-caption text-label-caption text-error mt-0.5"
            :data-testid="`mapping-duplicate-${index}`"
          >
            這個欄位被對應了兩次，後面的會覆蓋前面的
          </p>
        </td>
      </tr>
    </tbody>
  </table>
</template>
