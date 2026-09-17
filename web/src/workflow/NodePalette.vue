<script setup lang="ts">
/**
 * 節點庫
 *
 * 依 D-03 不支援拖曳。使用者先在畫布選一個插入點（連接線上的加號），
 * 再從這裡點選節點型別。沒選插入點時整個面板停用，
 * 因為「加到哪」是必要資訊，不該由系統猜。
 */
import { NODE_PALETTE, NODE_PALETTE_COUNT, type NodePaletteItem } from './palette'

defineProps<{
  /** 目前選定的插入位置說明，null 代表尚未選擇 */
  insertHint: string | null
}>()

defineEmits<{ pick: [NodePaletteItem] }>()
</script>

<template>
  <aside
    class="w-[280px] shrink-0 bg-surface-container-lowest border-r border-outline-variant flex flex-col"
    data-testid="node-palette"
  >
    <div class="px-3 py-2.5 border-b border-outline-variant">
      <div class="flex items-center justify-between">
        <span class="flex items-center gap-1.5 font-title-md text-title-md text-on-surface">
          <span class="material-symbols-outlined text-[18px] text-primary">account_tree</span>
          節點庫
        </span>
        <span class="font-label-caption text-label-caption text-secondary">
          共 {{ NODE_PALETTE_COUNT }} 種
        </span>
      </div>
    </div>

    <div
      class="px-3 py-2 border-b border-outline-variant font-label-caption text-label-caption"
      :class="insertHint ? 'bg-[#DBEAFE] text-primary' : 'bg-surface-container-low text-secondary'"
      data-testid="insert-hint"
    >
      <span v-if="insertHint">將插入於：{{ insertHint }}</span>
      <span v-else>先點畫布連接線上的「+」，再選擇要加入的節點</span>
    </div>

    <div class="flex-1 overflow-y-auto p-3 space-y-3" :class="{ 'opacity-40': !insertHint }">
      <div v-for="g in NODE_PALETTE" :key="g.key">
        <span class="block font-label-header text-label-header text-secondary mb-1.5">
          {{ g.title }}
        </span>

        <div class="space-y-1.5">
          <button
            v-for="item in g.items"
            :key="item.type"
            type="button"
            :disabled="!insertHint"
            :data-testid="`node-palette-${item.type}`"
            class="w-full flex items-start gap-2 px-2 py-2 rounded-lg border border-outline-variant bg-surface-container-lowest text-left transition-colors enabled:hover:border-primary-container enabled:hover:bg-surface-container-low disabled:cursor-not-allowed"
            @click="$emit('pick', item)"
          >
            <span class="material-symbols-outlined text-[16px] text-primary shrink-0 mt-0.5">
              {{ item.icon }}
            </span>
            <span class="min-w-0">
              <span class="block font-body-dense text-[12px] text-on-surface">
                {{ item.label }}
              </span>
              <span class="block font-label-caption text-label-caption text-outline truncate">
                {{ item.description }}
              </span>
            </span>
          </button>
        </div>
      </div>
    </div>
  </aside>
</template>
