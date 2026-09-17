<script setup lang="ts">
/**
 * 流程設計器
 *
 * 三欄：左節點庫、中畫布、右屬性面板，與表單設計器一致。
 *
 * 畫布採 SVG 畫線 + 絕對定位的 HTML 卡片：
 *   線需要貝茲曲線與箭頭，SVG 最直接；
 *   卡片需要文字截斷、hover 按鈕、圖示字型，HTML 較好處理。
 *   兩者共用同一組座標，由 computeLayout 算出。
 */
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useRoute } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import NodeCard from '@/workflow/NodeCard.vue'
import NodePalette from '@/workflow/NodePalette.vue'
import NodePropertyPanel from '@/workflow/NodePropertyPanel.vue'
import { edgeLabelStyle } from '@/workflow/edgeLabel'
import { insertionPoints } from '@/workflow/layout'
import type { NodePaletteItem } from '@/workflow/palette'
import { rejectTargetsFor } from '@/workflow/upstream'
import { useWorkflowDesigner } from '@/workflow/useWorkflowDesigner'
import type { WorkflowNode } from '@/api/workflows'

const route = useRoute()
const workflowKey = (route.params.workflowKey as string) ?? 'quotation_approval'

const d = useWorkflowDesigner(workflowKey)

/** 使用者點選的插入點。選定後節點庫才可用。 */
const insertAt = ref<{ from: string; to: string } | null>(null)
const publishing = ref(false)
const toast = ref<string | null>(null)

const points = computed(() => (d.layout.value ? insertionPoints(d.layout.value) : []))

const nodeLabel = (id: string) => {
  const n = d.content.value?.nodes.find((x) => x.id === id)
  return n?.label || id
}

const insertHint = computed(() =>
  insertAt.value
    ? `${nodeLabel(insertAt.value.from)} → ${nodeLabel(insertAt.value.to)}`
    : null,
)

const rejectTargets = computed(() =>
  rejectTargetsFor(d.content.value, d.selectedId.value),
)

const savedLabel = computed(() => {
  if (d.saving.value) return '儲存中'
  if (!d.lastSavedAt.value) return d.dirty.value ? '尚未儲存' : ''
  const t = d.lastSavedAt.value
  const hh = String(t.getHours()).padStart(2, '0')
  const mm = String(t.getMinutes()).padStart(2, '0')
  return d.dirty.value ? '有未儲存的變更' : `已儲存 ${hh}:${mm}`
})

/** 驗證通過後顯示綠色提示，讓使用者知道可以安心發布 */
const validationPassed = computed(
  () => d.validatedAt.value !== null && d.graphErrors.value.length === 0,
)

function pickPoint(p: { from: string; to: string }) {
  insertAt.value =
    insertAt.value?.from === p.from && insertAt.value?.to === p.to ? null : p
}

function addNode(item: NodePaletteItem) {
  if (!insertAt.value) return

  // 並行一次建立整組（parallel + 分支 + join）。
  // 單獨插一個 parallel 會立刻違反 WF-E009 與 WF-E010，
  // 使用者得連做好幾步才能回到合法狀態。
  if (item.type === 'parallel') {
    d.insertParallel(insertAt.value)
  } else if (item.type === 'join') {
    // join 不能單獨存在，導引使用者用「並行」
    d.error.value = '請使用「並行」，它會一併建立匯合節點'
  } else {
    d.insertNode(item.defaults() as WorkflowNode, insertAt.value)
  }

  insertAt.value = null
}

async function handleSave() {
  try {
    await d.save()
    showToast('草稿已儲存')
  } catch {
    /* 錯誤已記於 d.error */
  }
}

async function handleValidate() {
  const ok = await d.validate()
  showToast(ok ? '驗證通過' : `發現 ${d.graphErrors.value.length} 個問題`)
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
      <span class="hover:text-on-surface cursor-pointer">流程</span>
      <span class="material-symbols-outlined text-[14px]">chevron_right</span>
      <span class="text-on-surface font-semibold">
        {{ d.content.value?.name ?? workflowKey }}
      </span>
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
        <span
          class="w-1.5 h-1.5 rounded-full"
          :class="d.dirty.value ? 'bg-[#B45309]' : 'bg-[#15803D]'"
        />
        {{ savedLabel }}
      </span>
    </template>

    <!--
      抵銷 AppShell 主內容區的 padding，讓設計器佔滿整個視窗。
      上方不抵銷：main 的 padding-top 正好避開固定的 56px 頂部列，
      拉掉會讓工具列被頂部列蓋住。
    -->
    <div class="flex flex-col -mx-space-xl -mb-space-xl h-[calc(100vh-56px)]">
      <!-- 工具列 -->
      <div
        class="sticky top-0 z-30 flex items-center justify-between gap-2 px-space-xl py-2 bg-surface-container-lowest border-b border-outline-variant shrink-0"
      >
        <span class="font-label-caption text-label-caption text-secondary">
          由上往下自動排版，插入節點即可調整流程
        </span>

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
            data-testid="validate"
            class="inline-flex items-center gap-1.5 h-8 px-3 rounded-lg text-secondary hover:bg-surface-container-low font-body-dense text-body-dense"
            @click="handleValidate"
          >
            <span class="material-symbols-outlined text-[16px]">rule</span>
            驗證流程
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
            發布流程
          </button>
        </div>
      </div>

      <!-- 驗證結果 -->
      <div
        v-if="d.graphErrors.value.length > 0"
        class="px-space-xl py-2 bg-error-container border-b border-outline-variant"
        data-testid="validation-banner"
      >
        <div class="flex items-start gap-2">
          <span class="material-symbols-outlined text-[18px] text-error">error</span>
          <div class="flex-1">
            <span class="font-label-header text-label-header text-on-error-container">
              發現 {{ d.graphErrors.value.length }} 個問題
            </span>
            <ul class="mt-1 space-y-0.5">
              <li
                v-for="(err, i) in d.graphErrors.value"
                :key="i"
                class="font-body-dense text-body-dense text-on-error-container"
              >
                <button
                  v-if="err.node_id"
                  type="button"
                  class="underline underline-offset-2"
                  :data-testid="`error-goto-${err.node_id}`"
                  @click="d.selectedId.value = err.node_id"
                >
                  {{ err.code }} · {{ err.message }}
                </button>
                <span v-else>{{ err.code }} · {{ err.message }}</span>
              </li>
            </ul>
          </div>
        </div>
      </div>

      <div
        v-else-if="validationPassed"
        class="px-space-xl py-2 bg-[#DCFCE7] border-b border-outline-variant flex items-center gap-2"
        data-testid="validation-ok"
      >
        <span class="material-symbols-outlined text-[18px] text-[#15803D]">check_circle</span>
        <span class="font-body-dense text-body-dense text-[#15803D]">
          流程結構驗證通過，可以發布
        </span>
      </div>

      <div v-if="d.error.value" class="px-space-xl py-2 bg-error-container" data-testid="error-banner">
        <span class="font-body-dense text-body-dense text-on-error-container">
          {{ d.error.value }}
        </span>
      </div>

      <!-- 三欄主體 -->
      <div class="flex-1 flex min-h-0">
        <NodePalette :insert-hint="insertHint" @pick="addNode" />

        <div
          class="flex-1 overflow-auto bg-background p-8"
          data-testid="workflow-canvas"
          @click="d.selectedId.value = null"
        >
          <div
            v-if="d.loading.value"
            class="text-center text-secondary font-body-dense py-12"
            data-testid="canvas-loading"
          >
            載入中
          </div>

          <div
            v-else-if="!d.layout.value"
            class="text-center text-secondary font-body-dense py-12"
            data-testid="canvas-empty"
          >
            無法載入流程
          </div>

          <div
            v-else
            class="relative mx-auto"
            :style="{
              width: `${d.layout.value.width}px`,
              height: `${d.layout.value.height}px`,
            }"
          >
            <svg
              class="absolute inset-0 pointer-events-none"
              :width="d.layout.value.width"
              :height="d.layout.value.height"
              data-testid="workflow-edges"
            >
              <defs>
                <marker
                  id="arrow"
                  viewBox="0 0 10 10"
                  refX="9"
                  refY="5"
                  markerWidth="6"
                  markerHeight="6"
                  orient="auto-start-reverse"
                >
                  <path d="M 0 0 L 10 5 L 0 10 z" fill="#94A3B8" />
                </marker>
                <marker
                  id="arrow-return"
                  viewBox="0 0 10 10"
                  refX="9"
                  refY="5"
                  markerWidth="6"
                  markerHeight="6"
                  orient="auto-start-reverse"
                >
                  <path d="M 0 0 L 10 5 L 0 10 z" fill="#DC2626" />
                </marker>
              </defs>

              <path
                v-for="(e, i) in d.layout.value.edges"
                :key="`${e.from}-${e.to}-${i}`"
                :d="e.path"
                fill="none"
                :stroke="e.isReturn ? '#DC2626' : '#94A3B8'"
                stroke-width="1.5"
                :stroke-dasharray="e.isReturn ? '4 3' : undefined"
                :marker-end="e.isReturn ? 'url(#arrow-return)' : 'url(#arrow)'"
                :data-testid="`edge-${e.from}-${e.to}`"
                :data-return="e.isReturn ? 'true' : 'false'"
              />
            </svg>

            <!-- 分支標記。畫在線旁，說明這條路徑的條件。 -->
            <span
              v-for="(e, i) in d.layout.value.edges.filter((x) => x.when !== undefined || x.label)"
              :key="`label-${e.from}-${e.to}-${i}`"
              class="absolute px-1.5 py-0.5 rounded-lg font-label-caption text-label-caption pointer-events-none"
              :class="
                e.isReturn
                  ? 'bg-error-container text-error'
                  : e.when === false
                    ? 'bg-surface-container-high text-secondary'
                    : 'bg-[#DCFCE7] text-[#15803D]'
              "
              :style="edgeLabelStyle(e.path, e.isReturn)"
              :data-testid="`edge-label-${e.from}-${e.to}`"
            >
              {{ e.label ?? (e.when ? '是' : '否') }}
            </span>

            <!-- 插入點 -->
            <button
              v-for="p in points"
              :key="`insert-${p.from}-${p.to}`"
              type="button"
              :title="`在 ${nodeLabel(p.from)} 與 ${nodeLabel(p.to)} 之間插入節點`"
              :data-testid="`insert-${p.from}-${p.to}`"
              class="absolute w-6 h-6 -ml-3 -mt-3 rounded-full border flex items-center justify-center transition-colors z-10"
              :class="
                insertAt?.from === p.from && insertAt?.to === p.to
                  ? 'bg-primary-container border-primary-container text-on-primary'
                  : 'bg-surface-container-lowest border-outline-variant text-secondary hover:border-primary-container hover:text-primary'
              "
              :style="{ left: `${p.x}px`, top: `${p.y}px` }"
              @click.stop="pickPoint(p)"
            >
              <span class="material-symbols-outlined text-[14px]">add</span>
            </button>

            <NodeCard
              v-for="p in d.layout.value.nodes"
              :key="p.node.id"
              :node="p.node"
              :x="p.x"
              :y="p.y"
              :selected="d.selectedId.value === p.node.id"
              :has-error="d.errorNodeIds.value.has(p.node.id)"
              @select="d.selectedId.value = p.node.id"
              @remove="d.removeNode(p.node.id)"
            />
          </div>
        </div>

        <NodePropertyPanel
          :node="d.selectedNode.value"
          :reject-targets="rejectTargets"
          @update="(patch) => d.selectedId.value && d.updateNode(d.selectedId.value, patch)"
          @remove="d.selectedId.value && d.removeNode(d.selectedId.value)"
        />
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
