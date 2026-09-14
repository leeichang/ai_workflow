<script setup lang="ts">
/**
 * 表單設計器
 *
 * 依設計稿 docs/UI 設計/.../_5 實作。三欄：
 *   左 元件庫（17 種，三個分組）
 *   中 畫布（12 欄網格預覽）
 *   右 屬性面板（基本／資料／規則三頁籤）
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import FieldCard from '@/designer/FieldCard.vue'
import PropertyPanel from '@/designer/PropertyPanel.vue'
import { filterPalette, PALETTE_COUNT, type PaletteItem } from '@/designer/palette'
import { useDesigner } from '@/designer/useDesigner'

const route = useRoute()
const formKey = (route.params.formKey as string) ?? 'quotation_form'

const d = useDesigner(formKey)
const search = ref('')
const collapsed = ref<Set<string>>(new Set())
const publishing = ref(false)
const toast = ref<string | null>(null)

const groups = computed(() => filterPalette(search.value))

const savedLabel = computed(() => {
  if (d.saving.value) return '儲存中'
  if (!d.lastSavedAt.value) return d.dirty.value ? '尚未儲存' : ''
  const t = d.lastSavedAt.value
  const hh = String(t.getHours()).padStart(2, '0')
  const mm = String(t.getMinutes()).padStart(2, '0')
  return d.dirty.value ? '有未儲存的變更' : `已同步自動存檔 ${hh}:${mm}`
})

function toggleGroup(key: string) {
  const next = new Set(collapsed.value)
  next.has(key) ? next.delete(key) : next.add(key)
  collapsed.value = next
}

function addFromPalette(item: PaletteItem) {
  d.addField(item.defaults(), d.selectedKey.value ?? undefined)
}

async function handleSave() {
  try {
    await d.save()
    showToast('草稿已儲存')
  } catch {
    /* 錯誤已記於 d.error */
  }
}

async function handlePublish() {
  publishing.value = true
  const ok = await d.publish()
  publishing.value = false
  if (ok) showToast(`已發布為 v${d.publishedVersion.value}`)
}

function showToast(msg: string) {
  toast.value = msg
  setTimeout(() => (toast.value = null), 2500)
}

/** Cmd/Ctrl + Z 與 Shift 組合，以及 Cmd/Ctrl + S */
function onKeydown(e: KeyboardEvent) {
  const meta = e.metaKey || e.ctrlKey
  if (!meta) return

  if (e.key === 'z') {
    e.preventDefault()
    e.shiftKey ? d.redo() : d.undo()
  } else if (e.key === 's') {
    e.preventDefault()
    void handleSave()
  }
}

onMounted(() => {
  void d.load()
  window.addEventListener('keydown', onKeydown)
})

onUnmounted(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="hover:text-on-surface cursor-pointer">設計器</span>
      <span class="material-symbols-outlined text-[14px]">chevron_right</span>
      <span class="hover:text-on-surface cursor-pointer">表單</span>
      <span class="material-symbols-outlined text-[14px]">chevron_right</span>
      <span class="text-on-surface font-semibold">{{ d.content.value?.name ?? formKey }}</span>
      <span
        class="ml-1 px-1.5 py-0.5 rounded-lg bg-secondary-container text-on-secondary-fixed font-label-caption text-label-caption"
        data-testid="version-chip"
      >
        {{ d.publishedVersion.value ? `v${d.publishedVersion.value}` : 'v1' }} 草稿
      </span>
      <span
        v-if="savedLabel"
        class="ml-2 flex items-center gap-1 font-label-caption text-label-caption"
        :class="d.dirty.value ? 'text-[#B45309]' : 'text-[#15803D]'"
        data-testid="save-status"
      >
        <span class="w-1.5 h-1.5 rounded-full" :class="d.dirty.value ? 'bg-[#B45309]' : 'bg-[#15803D]'" />
        {{ savedLabel }}
      </span>
    </template>

    <div class="flex flex-col -m-space-xl h-[calc(100vh-56px)]">
      <!-- 工具列。sticky 避免捲動時被頂部列蓋住。 -->
      <div class="sticky top-0 z-30 flex items-center justify-end gap-2 px-space-xl py-2 bg-surface-container-lowest border-b border-outline-variant shrink-0">
        <div class="flex items-center gap-0.5 mr-2">
          <button
            type="button"
            title="復原 (Cmd+Z)"
            data-testid="undo"
            :disabled="!d.canUndo.value"
            class="p-1.5 rounded-lg text-secondary hover:bg-surface-container-low disabled:opacity-30 disabled:cursor-not-allowed"
            @click="d.undo()"
          >
            <span class="material-symbols-outlined text-[18px]">undo</span>
          </button>
          <button
            type="button"
            title="重做 (Cmd+Shift+Z)"
            data-testid="redo"
            :disabled="!d.canRedo.value"
            class="p-1.5 rounded-lg text-secondary hover:bg-surface-container-low disabled:opacity-30 disabled:cursor-not-allowed"
            @click="d.redo()"
          >
            <span class="material-symbols-outlined text-[18px]">redo</span>
          </button>
        </div>

        <button
          type="button"
          data-testid="validate"
          class="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg text-secondary hover:bg-surface-container-low font-body-dense text-body-dense"
          @click="d.validate()"
        >
          <span class="material-symbols-outlined text-[16px]">visibility</span>
          驗證
        </button>

        <button
          type="button"
          data-testid="save-draft"
          :disabled="d.saving.value"
          class="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-surface-container-low text-on-surface hover:bg-surface-container font-body-dense text-body-dense disabled:opacity-50"
          @click="handleSave"
        >
          <span class="material-symbols-outlined text-[16px]">save</span>
          儲存草稿
        </button>

        <button
          type="button"
          data-testid="publish"
          :disabled="publishing"
          class="inline-flex items-center gap-1.5 h-8 px-4 rounded-lg bg-primary-container text-on-primary font-body-dense text-body-dense hover:brightness-110 disabled:opacity-50"
          @click="handlePublish"
        >
          <span class="material-symbols-outlined text-[16px]">rocket_launch</span>
          發布表單
        </button>
      </div>

      <!-- 驗證錯誤 -->
      <div
        v-if="d.validationErrors.value.length > 0"
        class="px-space-xl py-2 bg-error-container border-b border-outline-variant"
        data-testid="validation-banner"
      >
        <div class="flex items-start gap-2">
          <span class="material-symbols-outlined text-[18px] text-error">error</span>
          <div class="flex-1">
            <span class="font-label-header text-label-header text-on-error-container">
              發現 {{ d.validationErrors.value.length }} 個問題
            </span>
            <ul class="mt-1 space-y-0.5">
              <li
                v-for="(err, i) in d.validationErrors.value"
                :key="i"
                class="font-body-dense text-body-dense text-on-error-container"
              >· {{ err }}</li>
            </ul>
          </div>
        </div>
      </div>

      <div v-if="d.error.value" class="px-space-xl py-2 bg-error-container" data-testid="error-banner">
        <span class="font-body-dense text-body-dense text-on-error-container">{{ d.error.value }}</span>
      </div>

      <!-- 三欄主體 -->
      <div class="flex-1 flex min-h-0">
        <!-- 元件庫 -->
        <aside
          class="w-[280px] shrink-0 bg-surface-container-lowest border-r border-outline-variant flex flex-col"
          data-testid="palette"
        >
          <div class="px-3 py-2.5 border-b border-outline-variant">
            <div class="flex items-center justify-between mb-2">
              <span class="flex items-center gap-1.5 font-title-md text-title-md text-on-surface">
                <span class="material-symbols-outlined text-[18px] text-primary">widgets</span>
                元件庫
              </span>
              <span class="font-label-caption text-label-caption text-secondary">
                共 {{ PALETTE_COUNT }} 種
              </span>
            </div>
            <div class="relative flex items-center">
              <span class="material-symbols-outlined absolute left-2 text-[16px] text-outline">search</span>
              <input
                v-model="search"
                type="text"
                placeholder="搜尋元件"
                data-testid="palette-search"
                class="w-full h-8 pl-7 pr-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
              />
            </div>
          </div>

          <div class="flex-1 overflow-y-auto p-3 space-y-3">
            <div v-for="g in groups" :key="g.key">
              <button
                type="button"
                class="w-full flex items-center justify-between mb-1.5"
                :data-testid="`palette-group-${g.key}`"
                @click="toggleGroup(g.key)"
              >
                <span class="font-label-header text-label-header text-secondary">{{ g.title }}</span>
                <span class="material-symbols-outlined text-[16px] text-outline">
                  {{ collapsed.has(g.key) ? 'expand_more' : 'expand_less' }}
                </span>
              </button>

              <div v-if="!collapsed.has(g.key)" class="grid grid-cols-2 gap-1.5">
                <button
                  v-for="item in g.items"
                  :key="`${g.key}-${item.label}`"
                  type="button"
                  :data-testid="`palette-item-${item.component}`"
                  :title="`點擊新增${item.label}`"
                  class="flex items-center gap-1.5 px-2 py-2 rounded-lg border border-outline-variant bg-surface-container-lowest hover:border-primary-container hover:bg-surface-container-low transition-colors text-left"
                  @click="addFromPalette(item)"
                >
                  <span class="material-symbols-outlined text-[16px] text-primary shrink-0">
                    {{ item.icon }}
                  </span>
                  <span class="font-body-dense text-[12px] text-on-surface truncate">
                    {{ item.label }}
                  </span>
                </button>
              </div>
            </div>
          </div>
        </aside>

        <!-- 畫布 -->
        <div class="flex-1 overflow-y-auto bg-background p-6" @click="d.selectedKey.value = null">
          <div
            v-if="d.loading.value"
            class="text-center text-secondary font-body-dense py-12"
            data-testid="canvas-loading"
          >載入中</div>

          <div
            v-else-if="!d.content.value"
            class="text-center text-secondary font-body-dense py-12"
            data-testid="canvas-empty"
          >無法載入表單</div>

          <div
            v-else
            class="max-w-[900px] mx-auto bg-surface-container-lowest rounded-xl shadow-sm p-6"
            data-testid="canvas"
          >
            <div class="mb-4 pb-4 border-b border-outline-variant">
              <h2 class="font-headline-lg text-headline-lg text-on-surface">
                {{ d.content.value.name ?? formKey }}
              </h2>
              <div class="flex items-center gap-3 mt-1 font-label-caption text-label-caption text-secondary">
                <span>表單代碼: {{ d.content.value.form_key }}</span>
                <span>業務物件: {{ d.content.value.business_object }}</span>
                <span data-testid="field-count">{{ d.content.value.fields.length }} 個欄位</span>
              </div>
            </div>

            <div class="grid grid-cols-12 gap-x-4 gap-y-1">
              <FieldCard
                v-for="f in d.content.value.fields"
                :key="f.key"
                :field="f"
                :selected="d.selectedKey.value === f.key"
                @select="d.selectedKey.value = f.key"
                @duplicate="d.duplicateField(f.key)"
                @move="(delta) => d.moveField(f.key, delta)"
                @remove="d.removeField(f.key)"
              />
            </div>

            <div
              v-if="d.content.value.fields.length === 0"
              class="text-center py-12 text-secondary font-body-dense border-2 border-dashed border-outline-variant rounded-xl"
            >
              從左側元件庫點選以新增欄位
            </div>
          </div>
        </div>

        <PropertyPanel
          :field="d.selectedField.value"
          @update="(patch) => d.selectedKey.value && d.updateField(d.selectedKey.value, patch)"
          @remove="d.selectedKey.value && d.removeField(d.selectedKey.value)"
        />
      </div>
    </div>

    <!-- 提示 -->
    <Transition name="fade">
      <div
        v-if="toast"
        class="fixed bottom-6 left-1/2 -translate-x-1/2 px-4 py-2 rounded-lg bg-inverse-surface text-inverse-on-surface font-body-dense text-body-dense shadow-lg z-50"
        data-testid="toast"
      >
        {{ toast }}
      </div>
    </Transition>
  </AppShell>
</template>

<style scoped>
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
