<script setup lang="ts">
/**
 * 流程節點卡片
 *
 * 位置由 computeLayout 決定，這裡只負責外觀。
 * 依決議 D-03 不支援拖曳，所以不綁任何拖曳事件。
 */
import { computed } from 'vue'
import type { WorkflowNode } from '@/api/workflows'
import { NODE_HEIGHT, NODE_WIDTH } from './layout'
import { NODE_STYLE, nodeTypeLabel, resolverLabel } from './palette'

const props = defineProps<{
  node: WorkflowNode
  x: number
  y: number
  selected: boolean
  hasError: boolean
}>()

defineEmits<{ select: []; remove: [] }>()

const style = computed(() => NODE_STYLE[props.node.type])

/** 卡片第二行的摘要，讓使用者不必點開屬性面板就看得懂 */
const summary = computed(() => {
  const n = props.node
  switch (n.type) {
    case 'trigger':
      return '表單送出後啟動'
    case 'condition':
      return n.expression || '尚未設定條件'
    case 'human_approval':
    case 'human_task':
      return n.participant === 'external'
        ? `外部：${resolverLabel(n.resolver ?? n.assignee)}`
        : resolverLabel(n.resolver ?? n.assignee)
    case 'action':
      return n.action || '尚未設定動作'
    case 'notification':
      return `${(n.channel ?? []).join('、') || '未設定管道'} → ${(n.to ?? []).join('、') || '未設定對象'}`
    case 'join':
      return joinSummary(n)
    case 'parallel':
      return '分支同時進行'
    case 'end':
      return endLabel(n.result)
    default:
      return ''
  }
})

function joinSummary(n: WorkflowNode): string {
  const j = n.join
  if (!j) return '尚未設定'
  if (j.completion === 'ALL') return '全部完成才繼續'
  if (j.completion === 'ANY') return '任一完成即繼續'
  return `${j.n ?? '?'} 人完成即繼續`
}

function endLabel(result?: string): string {
  if (result === 'rejected') return '以退回結束'
  if (result === 'cancelled') return '以取消結束'
  return '正常結束'
}

/** 有逾時或退回設定時在卡片右下角顯示小標記 */
const badges = computed(() => {
  const list: { icon: string; text: string }[] = []
  const n = props.node

  if (n.timeout) {
    list.push({ icon: 'schedule', text: `逾時 ${n.timeout.after}` })
  }
  if (n.on_reject?.action === 'goto') {
    list.push({ icon: 'undo', text: `退回至 ${n.on_reject.node}` })
  }
  if (n.retry?.max_attempts) {
    list.push({ icon: 'refresh', text: `重試 ${n.retry.max_attempts} 次` })
  }
  return list
})

const canRemove = computed(() => props.node.type !== 'trigger')
</script>

<template>
  <div
    class="absolute rounded-xl border bg-surface-container-lowest flex overflow-hidden transition-shadow cursor-pointer group"
    :class="[
      hasError
        ? 'border-error shadow-[0_0_0_2px_rgba(220,38,38,0.25)]'
        : selected
          ? 'border-primary-container shadow-[0_0_0_2px_rgba(37,99,235,0.2)]'
          : 'border-outline-variant hover:border-primary-container',
    ]"
    :style="{
      left: `${x}px`,
      top: `${y}px`,
      width: `${NODE_WIDTH}px`,
      minHeight: `${NODE_HEIGHT}px`,
    }"
    :data-testid="`node-${node.id}`"
    :data-node-type="node.type"
    :data-selected="selected ? 'true' : 'false'"
    :data-error="hasError ? 'true' : 'false'"
    @click.stop="$emit('select')"
  >
    <!-- 左側色條，用顏色快速區分節點類別 -->
    <div class="w-1 shrink-0" :class="style.accent" />

    <div class="flex-1 min-w-0 px-3 py-2.5 flex gap-2.5">
      <div
        class="w-7 h-7 rounded-lg flex items-center justify-center shrink-0"
        :class="[style.bg, style.text]"
      >
        <span class="material-symbols-outlined text-[16px]">{{ style.icon }}</span>
      </div>

      <div class="flex-1 min-w-0">
        <div class="flex items-center gap-1.5">
          <span class="font-title-md text-title-md text-on-surface truncate">
            {{ node.label || nodeTypeLabel(node.type) }}
          </span>
          <span
            class="px-1.5 py-0.5 rounded-lg bg-surface-container-high font-label-caption text-label-caption text-secondary shrink-0"
          >
            {{ nodeTypeLabel(node.type) }}
          </span>
        </div>

        <p
          class="font-body-dense text-body-dense text-secondary truncate mt-0.5"
          :data-testid="`node-summary-${node.id}`"
        >
          {{ summary }}
        </p>

        <div v-if="badges.length" class="flex flex-wrap items-center gap-x-2 gap-y-0.5 mt-1">
          <span
            v-for="b in badges"
            :key="b.text"
            class="inline-flex items-center gap-0.5 font-label-caption text-label-caption text-outline"
          >
            <span class="material-symbols-outlined text-[12px]">{{ b.icon }}</span>
            {{ b.text }}
          </span>
        </div>
      </div>

      <button
        v-if="canRemove"
        type="button"
        title="刪除節點"
        :data-testid="`node-remove-${node.id}`"
        class="self-start p-1 rounded-lg text-outline opacity-0 group-hover:opacity-100 hover:bg-error-container hover:text-error transition-opacity shrink-0"
        @click.stop="$emit('remove')"
      >
        <span class="material-symbols-outlined text-[16px]">delete</span>
      </button>
    </div>
  </div>
</template>
