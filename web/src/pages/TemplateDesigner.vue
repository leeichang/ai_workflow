<script setup lang="ts">
/**
 * 單據套版設計器
 *
 * 依設計稿 _9 實作。三欄：
 *   左 可用欄位／元素元件
 *   中 A4 版面畫布（含標尺、安全邊距、縮放）
 *   右 元素屬性
 *
 * 編輯結果是 pdfme 模板，交給渲染服務產生 PDF。
 * 預覽走真實渲染而非前端模擬——設計時看到的就是印出來的，
 * 不會有「畫面對了但 PDF 不對」的落差。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import AppShell from '@/components/AppShell.vue'
import FieldPalette from '@/template/FieldPalette.vue'
import TemplateCanvas from '@/template/TemplateCanvas.vue'
import ElementPropertyPanel from '@/template/ElementPropertyPanel.vue'
import { useTemplateDesigner } from '@/template/useTemplateDesigner'
import { defaultQuotationLayout } from '@/template/defaultLayout'
import { renderPreview } from '@/api/templates'
import type { DocField, ElementSpec } from '@/template/fields'

const d = useTemplateDesigner()

const zoom = ref(100)
const toast = ref<string | null>(null)
const previewUrl = ref<string | null>(null)
const previewing = ref(false)
const previewError = ref<string | null>(null)

/** 底稿模式。固定底稿不支援動態分頁，見 06_單據套版設計器選型.md 4.3。 */
const baseMode = ref<'standard' | 'fixed'>('standard')

const savedLabel = computed(() => (d.dirty.value ? '有未儲存的變更' : '已儲存'))

function addField(field: DocField) {
  // 依序往下堆疊，避免全部疊在同一點
  const y = 20 + d.elements.value.length * 10
  d.addField(field, { x: 15, y })
}

function addElement(el: ElementSpec) {
  const y = 20 + d.elements.value.length * 10
  d.addElement(el.type, { x: 15, y })
}

function showToast(message: string) {
  toast.value = message
  setTimeout(() => (toast.value = null), 2500)
}

/**
 * 預覽 PDF
 *
 * 送到渲染服務產生真實的 PDF。以範例資料渲染，
 * 實際內容依單據資料而定——設計稿的提示文字也是這樣寫的。
 */
async function preview() {
  previewing.value = true
  previewError.value = null

  try {
    const blob = await renderPreview(d.elements.value)
    if (previewUrl.value) URL.revokeObjectURL(previewUrl.value)
    previewUrl.value = URL.createObjectURL(blob)
  } catch (e) {
    previewError.value = e instanceof Error ? e.message : '預覽失敗'
  } finally {
    previewing.value = false
  }
}

function closePreview() {
  if (previewUrl.value) URL.revokeObjectURL(previewUrl.value)
  previewUrl.value = null
  previewError.value = null
}

function onKeydown(e: KeyboardEvent) {
  const meta = e.metaKey || e.ctrlKey

  if (meta && e.key === 'z') {
    e.preventDefault()
    e.shiftKey ? d.redo() : d.undo()
    return
  }

  // 選取元素後用方向鍵微調位置。滑鼠拖曳難以精準對齊。
  if (!d.selectedName.value) return
  const step = e.shiftKey ? 5 : 1
  const moves: Record<string, [number, number]> = {
    ArrowLeft: [-step, 0],
    ArrowRight: [step, 0],
    ArrowUp: [0, -step],
    ArrowDown: [0, step],
  }
  const delta = moves[e.key]
  if (delta) {
    e.preventDefault()
    d.move(d.selectedName.value, delta[0], delta[1])
  }
}

onMounted(() => {
  d.load(defaultQuotationLayout())
  window.addEventListener('keydown', onKeydown)
})

onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  if (previewUrl.value) URL.revokeObjectURL(previewUrl.value)
})
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="hover:text-on-surface cursor-pointer">設計器</span>
      <span class="material-symbols-outlined text-[14px]">chevron_right</span>
      <span class="hover:text-on-surface cursor-pointer">單據</span>
      <span class="material-symbols-outlined text-[14px]">chevron_right</span>
      <span class="text-on-surface font-semibold">報價單</span>
      <span
        class="ml-1 px-1.5 py-0.5 rounded-lg bg-secondary-container text-on-secondary-fixed font-label-caption text-label-caption"
        data-testid="version-chip"
      >
        v2 草稿
      </span>
      <span
        class="ml-2 flex items-center gap-1 font-label-caption text-label-caption"
        :class="d.dirty.value ? 'text-[#B45309]' : 'text-[#15803D]'"
        data-testid="save-status"
      >
        <span
          class="w-1.5 h-1.5 rounded-full"
          :class="d.dirty.value ? 'bg-[#B45309]' : 'bg-[#15803D]'"
        />
        {{ savedLabel }}
      </span>
    </template>

    <!--
      抵銷 AppShell 的 padding 讓設計器佔滿視窗。
      上方不抵銷：main 的 padding-top 正好避開固定的 56px 頂部列。
    -->
    <div class="flex flex-col -mx-space-xl -mb-space-xl h-[calc(100vh-56px)]">
      <!-- 工具列 -->
      <div
        class="sticky top-0 z-30 flex items-center justify-between gap-2 px-space-xl py-2 bg-surface-container-lowest border-b border-outline-variant shrink-0"
      >
        <label class="flex items-center gap-1.5">
          <span class="font-label-caption text-label-caption text-secondary">底稿模式</span>
          <select
            v-model="baseMode"
            data-testid="base-mode"
            class="h-8 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
          >
            <option value="standard">標準（可自動分頁）</option>
            <option value="fixed">固定底稿（上傳 PDF）</option>
          </select>
        </label>

        <div class="flex items-center gap-2">
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
            data-testid="preview"
            :disabled="previewing"
            class="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg text-secondary hover:bg-surface-container-low font-body-dense text-body-dense disabled:opacity-50"
            @click="preview"
          >
            <span class="material-symbols-outlined text-[16px]">visibility</span>
            {{ previewing ? '產生中' : '預覽 PDF' }}
          </button>

          <button
            type="button"
            data-testid="save-draft"
            class="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg bg-surface-container-low text-on-surface hover:bg-surface-container font-body-dense text-body-dense"
            @click="showToast('草稿已儲存')"
          >
            <span class="material-symbols-outlined text-[16px]">save</span>
            儲存草稿
          </button>

          <button
            type="button"
            data-testid="publish"
            class="inline-flex items-center gap-1.5 h-8 px-4 rounded-lg bg-primary-container text-on-primary font-body-dense text-body-dense hover:brightness-110"
            @click="showToast('已發布')"
          >
            <span class="material-symbols-outlined text-[16px]">rocket_launch</span>
            發布
          </button>
        </div>
      </div>

      <!-- 固定底稿不支援自動分頁，選了就要講清楚 -->
      <div
        v-if="baseMode === 'fixed'"
        class="px-space-xl py-2 bg-[#FEF3C7] font-label-caption text-label-caption text-[#B45309]"
        data-testid="fixed-mode-warning"
      >
        固定底稿模式不支援明細自動分頁。明細列數超過一頁時會被截斷，
        僅適用於已有印刷格式且行數固定的單據。
      </div>

      <!-- 三欄主體 -->
      <div class="flex-1 flex min-h-0">
        <FieldPalette @add-field="addField" @add-element="addElement" />

        <div class="flex-1 flex flex-col min-w-0">
          <TemplateCanvas
            :elements="d.elements.value"
            :selected-name="d.selectedName.value"
            :zoom="zoom"
            @select="d.selectedName.value = $event"
            @move="(name, dx, dy) => d.move(name, dx, dy)"
          />

          <!-- 縮放列 -->
          <div
            class="h-10 shrink-0 flex items-center justify-center gap-3 bg-surface-container-lowest border-t border-outline-variant"
          >
            <button
              type="button"
              title="縮小"
              data-testid="zoom-out"
              class="w-7 h-7 rounded-lg text-secondary hover:bg-surface-container-low"
              @click="zoom = Math.max(50, zoom - 10)"
            >
              <span class="material-symbols-outlined text-[16px]">remove</span>
            </button>
            <span
              class="font-data-mono text-[12px] text-on-surface w-12 text-center"
              data-testid="zoom-value"
            >
              {{ zoom }}%
            </span>
            <button
              type="button"
              title="放大"
              data-testid="zoom-in"
              class="w-7 h-7 rounded-lg text-secondary hover:bg-surface-container-low"
              @click="zoom = Math.min(200, zoom + 10)"
            >
              <span class="material-symbols-outlined text-[16px]">add</span>
            </button>
            <button
              type="button"
              data-testid="zoom-reset"
              class="h-7 px-2 rounded-lg text-secondary hover:bg-surface-container-low font-body-dense text-[12px]"
              @click="zoom = 100"
            >
              實際大小
            </button>
          </div>
        </div>

        <ElementPropertyPanel
          :element="d.selected.value"
          @update="(patch) => d.selectedName.value && d.update(d.selectedName.value, patch)"
          @remove="d.selectedName.value && d.remove(d.selectedName.value)"
        />
      </div>
    </div>

    <!-- 預覽視窗 -->
    <div
      v-if="previewUrl || previewError"
      class="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-8"
      data-testid="preview-modal"
      @click.self="closePreview"
    >
      <div class="bg-surface-container-lowest rounded-xl shadow-2xl w-[900px] max-w-full h-full flex flex-col overflow-hidden">
        <div class="h-12 px-4 flex items-center justify-between border-b border-outline-variant shrink-0">
          <span class="font-title-md text-title-md text-on-surface">報價單 PDF 預覽</span>
          <div class="flex items-center gap-2">
            <a
              v-if="previewUrl"
              :href="previewUrl"
              download="quotation-preview.pdf"
              data-testid="preview-download"
              class="inline-flex items-center gap-1 h-8 px-3 rounded-lg bg-primary-container text-on-primary font-body-dense text-body-dense"
            >
              <span class="material-symbols-outlined text-[16px]">download</span>
              下載 PDF
            </a>
            <button
              type="button"
              data-testid="preview-close"
              class="w-8 h-8 rounded-lg text-secondary hover:bg-surface-container-low"
              @click="closePreview"
            >
              <span class="material-symbols-outlined text-[18px]">close</span>
            </button>
          </div>
        </div>

        <div
          v-if="previewError"
          class="flex-1 flex items-center justify-center p-6 text-center"
          data-testid="preview-error"
        >
          <p class="font-body-dense text-body-dense text-error">{{ previewError }}</p>
        </div>
        <iframe
          v-else-if="previewUrl"
          :src="previewUrl"
          class="flex-1 w-full"
          title="PDF 預覽"
          data-testid="preview-frame"
        />

        <p class="px-4 py-2 border-t border-outline-variant font-label-caption text-label-caption text-secondary shrink-0">
          以範例資料渲染，實際內容依報價單資料而定
        </p>
      </div>
    </div>

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
