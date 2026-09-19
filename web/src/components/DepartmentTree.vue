<script setup lang="ts">
/**
 * 部門樹
 *
 * 遞迴元件——每一層都是同一個元件。Vue 的自我參照需要元件有名字，
 * `<script setup>` 用檔名推導，所以檔名就是元件名。
 *
 * 樹在應用層組而非 SQL 遞迴 CTE：部門數是百位數等級，
 * 而且前端本來就要拿到扁平清單做「上層部門」下拉選單。
 */
import type { Department } from '@/api/org'

/** 扁平清單組成的樹節點 */
export interface DeptNode extends Department {
  children: DeptNode[]
}

defineProps<{
  nodes: DeptNode[]
  /** 目前選取的部門。null 代表「全部」 */
  selectedId: string | null
  depth?: number
}>()

defineEmits<{ select: [id: string] }>()
</script>

<template>
  <ul class="list-none">
    <li v-for="node in nodes" :key="node.id">
      <button
        type="button"
        class="w-full flex items-center justify-between gap-2 px-space-sm py-1.5 rounded-lg text-left transition-colors"
        :class="
          selectedId === node.id
            ? 'bg-primary-container text-on-primary font-semibold'
            : 'text-on-surface-variant hover:bg-surface-container-low'
        "
        :style="{ paddingLeft: `${(depth ?? 0) * 16 + 8}px` }"
        :data-testid="`dept-node-${node.code}`"
        @click="$emit('select', node.id)"
      >
        <span class="flex items-center gap-1.5 min-w-0">
          <span class="material-symbols-outlined text-[16px] shrink-0">
            {{ node.children.length > 0 ? 'folder' : 'folder_open' }}
          </span>
          <span class="font-body-dense text-body-dense truncate">
            {{ node.name }}
          </span>
          <!-- 停用的部門仍要看得到：進行中的流程可能還指向它 -->
          <span
            v-if="node.status !== 'ACTIVE'"
            class="px-1 rounded bg-surface-container-low text-secondary font-label-caption text-label-caption shrink-0"
          >
            停用
          </span>
        </span>
        <!-- 空部門要看得出來——沒有成員的部門通常是建錯或已裁撤 -->
        <span
          class="font-label-caption text-label-caption shrink-0"
          :class="node.member_count === 0 ? 'text-error' : 'text-secondary'"
        >
          {{ node.member_count }}
        </span>
      </button>

      <DepartmentTree
        v-if="node.children.length > 0"
        :nodes="node.children"
        :selected-id="selectedId"
        :depth="(depth ?? 0) + 1"
        @select="$emit('select', $event)"
      />
    </li>
  </ul>
</template>
