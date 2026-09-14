<script setup lang="ts">
/**
 * 畫布上的欄位卡片
 *
 * 顯示欄位的預覽外觀，選取時外框變藍並顯示浮動工具列。
 * 依設計稿 _5，工具列有複製、上下移動、刪除三個動作。
 */
import { computed } from 'vue'
import type { FormField } from '@/api/types'

const props = defineProps<{
  field: FormField
  selected: boolean
}>()

const emit = defineEmits<{
  select: []
  duplicate: []
  move: [number]
  remove: []
}>()

/** 版面元素用 help 欄位標記，不是真的欄位 */
const layoutKind = computed(() => {
  const h = props.field.ui.help ?? ''
  if (h === '__layout_heading__') return 'heading'
  if (h === '__layout_divider__') return 'divider'
  if (h === '__layout_note__') return 'note'
  return null
})

const isComputed = computed(() => Boolean(props.field.data.computed))
const isRequired = computed(() => props.field.workflow?.required_when === 'true')
const externalHidden = computed(() => props.field.ui.external_visible === false)

/** 12 欄網格的 span class。Tailwind 需要完整類名才能被掃描到。 */
const spanClass = computed(() => {
  const map: Record<number, string> = {
    1: 'col-span-1', 2: 'col-span-2', 3: 'col-span-3', 4: 'col-span-4',
    5: 'col-span-5', 6: 'col-span-6', 7: 'col-span-7', 8: 'col-span-8',
    9: 'col-span-9', 10: 'col-span-10', 11: 'col-span-11', 12: 'col-span-12',
  }
  return map[props.field.ui.width ?? 12] ?? 'col-span-12'
})
</script>

<template>
  <div
    :class="[spanClass, 'relative group']"
    :data-testid="'field-card'"
    :data-field-key="field.key"
    :data-selected="selected"
    @click.stop="emit('select')"
  >
    <!-- 分隔線 -->
    <hr v-if="layoutKind === 'divider'" class="my-3 border-outline-variant" />

    <!-- 區塊標題 -->
    <h3
      v-else-if="layoutKind === 'heading'"
      class="font-title-md text-title-md text-on-surface py-1"
    >
      {{ field.ui.label }}
    </h3>

    <!-- 說明文字 -->
    <p
      v-else-if="layoutKind === 'note'"
      class="font-body-dense text-body-dense text-secondary py-1"
    >
      {{ field.ui.label }}
    </p>

    <!-- 一般欄位 -->
    <div v-else class="py-1">
      <div class="flex items-center gap-1 mb-1">
        <span class="font-label-header text-label-header text-on-surface">
          {{ field.ui.label }}
        </span>
        <span v-if="isRequired" class="text-error">*</span>
        <span
          v-if="externalHidden"
          class="px-1 rounded bg-[#FED7AA] text-[#9A3412] text-[10px] font-semibold"
          data-testid="external-hidden"
        >客戶看不到</span>
        <span
          v-if="isComputed"
          class="material-symbols-outlined text-[13px] text-primary"
          title="系統計算欄位"
          data-testid="computed-icon"
        >calculate</span>
        <span class="ml-auto font-data-mono text-[10px] text-outline">
          {{ field.data.path || '未綁定' }}
        </span>
      </div>

      <!-- 明細表格預覽 -->
      <div
        v-if="field.ui.component === 'table'"
        class="border border-outline-variant rounded-lg overflow-hidden"
      >
        <table class="w-full">
          <thead class="bg-surface-container-low">
            <tr>
              <th
                v-for="col in field.ui.columns ?? []"
                :key="col.key"
                class="px-2 py-1 font-label-caption text-label-caption text-secondary"
                :class="col.align === 'right' ? 'text-right' : 'text-left'"
              >
                {{ col.label }}
              </th>
            </tr>
          </thead>
          <tbody>
            <tr class="border-t border-outline-variant">
              <td
                v-for="col in field.ui.columns ?? []"
                :key="col.key"
                class="px-2 py-1.5 font-body-dense text-body-dense text-outline"
                :class="col.align === 'right' ? 'text-right' : 'text-left'"
              >
                {{ col.component === 'number' ? '0.00' : '—' }}
              </td>
            </tr>
          </tbody>
        </table>
      </div>

      <!-- 一般輸入預覽 -->
      <div
        v-else
        class="h-9 px-2 flex items-center bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
        :class="isComputed ? 'text-outline' : 'text-secondary'"
      >
        <span v-if="field.ui.prefix" class="mr-1 text-outline">{{ field.ui.prefix }}</span>
        <span class="flex-1 truncate">
          {{ field.ui.placeholder || (isComputed ? '系統計算' : '　') }}
        </span>
        <span v-if="field.ui.suffix" class="ml-1 text-outline">{{ field.ui.suffix }}</span>
        <span v-if="isComputed" class="material-symbols-outlined text-[14px] text-outline">lock</span>
      </div>
    </div>

    <!-- 選取外框 -->
    <div
      class="absolute inset-0 rounded-lg pointer-events-none transition-colors"
      :class="selected
        ? 'ring-2 ring-primary-container'
        : 'ring-1 ring-transparent group-hover:ring-outline-variant'"
    />

    <!-- 浮動工具列
         放右下角而非右上角：右上角會蓋住欄位標籤與資料路徑，
         那兩者是設計者辨識欄位的主要依據。 -->
    <div
      v-if="selected"
      class="absolute -bottom-3 right-2 flex items-center gap-0.5 bg-primary-container rounded-lg shadow-sm px-1 py-0.5 z-20"
      data-testid="field-toolbar"
      @click.stop
    >
      <button
        type="button"
        title="複製"
        data-testid="field-duplicate"
        class="p-1 text-on-primary hover:bg-black/10 rounded"
        @click="emit('duplicate')"
      >
        <span class="material-symbols-outlined text-[14px]">content_copy</span>
      </button>
      <button
        type="button"
        title="上移"
        data-testid="field-move-up"
        class="p-1 text-on-primary hover:bg-black/10 rounded"
        @click="emit('move', -1)"
      >
        <span class="material-symbols-outlined text-[14px]">arrow_upward</span>
      </button>
      <button
        type="button"
        title="下移"
        data-testid="field-move-down"
        class="p-1 text-on-primary hover:bg-black/10 rounded"
        @click="emit('move', 1)"
      >
        <span class="material-symbols-outlined text-[14px]">arrow_downward</span>
      </button>
      <button
        type="button"
        title="刪除"
        data-testid="field-remove"
        class="p-1 text-on-primary hover:bg-black/10 rounded"
        @click="emit('remove')"
      >
        <span class="material-symbols-outlined text-[14px]">delete</span>
      </button>
    </div>
  </div>
</template>
