<script setup lang="ts">
/**
 * 可用欄位 / 元素元件
 *
 * 依設計稿 _9 左側面板。兩個頁籤：
 *   可用欄位 綁資料的欄位，拖進版面後印出實際值
 *   元素元件 純版面圖形，與資料無關
 *
 * 「可用欄位」是這個設計器與一般繪圖工具的差別所在，因此設為預設頁籤。
 */
import { computed, ref } from 'vue'
import {
  ELEMENTS,
  ITEMS_FIELD,
  KIND_LABEL,
  filterGroups,
  type DocField,
  type ElementSpec,
} from './fields'

defineEmits<{
  addField: [DocField]
  addElement: [ElementSpec]
}>()

type Tab = 'fields' | 'elements'
const tab = ref<Tab>('fields')
const search = ref('')

const groups = computed(() => filterGroups(search.value))

const matchesItems = computed(() => {
  const q = search.value.trim().toLowerCase()
  return !q || ITEMS_FIELD.label.toLowerCase().includes(q)
})
</script>

<template>
  <aside
    class="w-[280px] shrink-0 bg-surface-container-lowest border-r border-outline-variant flex flex-col"
    data-testid="field-palette"
  >
    <div class="flex border-b border-outline-variant" role="tablist">
      <button
        v-for="t in [
          { key: 'fields' as Tab, label: '可用欄位', icon: 'data_object' },
          { key: 'elements' as Tab, label: '元素元件', icon: 'widgets' },
        ]"
        :key="t.key"
        type="button"
        role="tab"
        :aria-selected="tab === t.key"
        :data-testid="`tab-${t.key}`"
        class="flex-1 h-10 flex items-center justify-center gap-1.5 font-body-dense text-body-dense border-b-2 transition-colors"
        :class="
          tab === t.key
            ? 'border-primary-container text-primary font-semibold'
            : 'border-transparent text-secondary hover:text-on-surface'
        "
        @click="tab = t.key"
      >
        <span class="material-symbols-outlined text-[16px]">{{ t.icon }}</span>
        {{ t.label }}
      </button>
    </div>

    <template v-if="tab === 'fields'">
      <div class="px-3 py-2.5 border-b border-outline-variant">
        <div class="relative flex items-center">
          <span class="material-symbols-outlined absolute left-2 text-[16px] text-outline">
            search
          </span>
          <input
            v-model="search"
            type="text"
            placeholder="搜尋欄位 (如：幣別、統一編號)"
            data-testid="field-search"
            class="w-full h-8 pl-7 pr-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
          />
        </div>
      </div>

      <div class="flex-1 overflow-y-auto p-3 space-y-4">
        <!--
          明細表獨立一區。它拖進去會展開成多欄動態表格，
          與其他「單一值」欄位行為不同，混在一起容易誤解。
        -->
        <div v-if="matchesItems">
          <div class="flex items-center justify-between mb-1.5">
            <span class="font-label-header text-label-header text-secondary">核心表格元件</span>
            <span
              class="px-1.5 py-0.5 rounded bg-primary-container text-on-primary font-label-caption text-[10px] font-bold"
            >
              DYNAMIC
            </span>
          </div>

          <button
            type="button"
            data-testid="field-items"
            class="w-full flex items-start gap-2 px-2.5 py-2.5 rounded-lg border border-primary-container bg-surface-container-low text-left hover:brightness-95 transition"
            @click="$emit('addField', ITEMS_FIELD)"
          >
            <span class="material-symbols-outlined text-[18px] text-primary shrink-0 mt-0.5">
              table_chart
            </span>
            <span class="min-w-0">
              <span class="block font-title-md text-title-md text-on-surface">
                {{ ITEMS_FIELD.label }}
              </span>
              <span class="block font-label-caption text-label-caption text-secondary mt-0.5">
                拖入後可自訂多欄位排版
              </span>
              <span class="block font-data-mono text-[10px] text-outline mt-1">
                {{ ITEMS_FIELD.source }}
              </span>
            </span>
          </button>
        </div>

        <div v-for="g in groups" :key="g.key">
          <span class="block font-label-header text-label-header text-secondary mb-1.5">
            {{ g.title }}
          </span>

          <div class="space-y-1">
            <button
              v-for="f in g.fields"
              :key="f.path"
              type="button"
              :data-testid="`field-${f.path}`"
              :title="`點擊加入${f.label}`"
              class="w-full flex items-center gap-2 px-2 py-1.5 rounded-lg border border-outline-variant bg-surface-container-lowest text-left hover:border-primary-container hover:bg-surface-container-low transition-colors"
              @click="$emit('addField', f)"
            >
              <span class="material-symbols-outlined text-[14px] text-outline shrink-0">
                drag_indicator
              </span>
              <span class="min-w-0 flex-1">
                <span class="block font-body-dense text-[12px] text-on-surface truncate">
                  {{ f.label }}
                </span>
                <span class="block font-data-mono text-[10px] text-outline truncate">
                  {{ f.source }}
                </span>
              </span>
              <span
                class="px-1 py-0.5 rounded bg-surface-container-high font-label-caption text-[10px] text-secondary shrink-0"
              >
                {{ KIND_LABEL[f.kind] }}
              </span>
            </button>
          </div>
        </div>

        <p
          v-if="groups.length === 0 && !matchesItems"
          class="text-center font-body-dense text-body-dense text-secondary py-6"
          data-testid="field-empty"
        >
          找不到符合的欄位
        </p>
      </div>
    </template>

    <div v-else class="flex-1 overflow-y-auto p-3">
      <span class="block font-label-header text-label-header text-secondary mb-1.5">
        版面元素
      </span>
      <div class="grid grid-cols-2 gap-1.5">
        <button
          v-for="el in ELEMENTS"
          :key="el.type"
          type="button"
          :data-testid="`element-${el.type}`"
          class="flex items-center gap-1.5 px-2 py-2 rounded-lg border border-outline-variant bg-surface-container-lowest hover:border-primary-container hover:bg-surface-container-low transition-colors text-left"
          @click="$emit('addElement', el)"
        >
          <span class="material-symbols-outlined text-[16px] text-primary shrink-0">
            {{ el.icon }}
          </span>
          <span class="font-body-dense text-[12px] text-on-surface truncate">
            {{ el.label }}
          </span>
        </button>
      </div>
    </div>
  </aside>
</template>
