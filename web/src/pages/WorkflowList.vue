<script setup lang="ts">
/**
 * 流程清單與新增
 *
 * 與 FormList 同一個缺口：後端有 POST /workflows、前端的
 * createWorkflow() 也已定義，但先前沒有任何畫面呼叫它——
 * /designer/workflows/:workflowKey 只能開既有流程，
 * 選單寫死指向 quotation_approval。
 *
 * 流程比表單多一層限制：初始內容要通過 WF-E001
 * （恰一個 trigger、至少一個 end），空殼會被擋下。
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import * as workflowsApi from '@/api/workflows'
import type { WorkflowSummary } from '@/api/workflows'
import { ApiError } from '@/api/types'

const router = useRouter()

const workflows = ref<WorkflowSummary[]>([])
const loading = ref(true)
const errorMessage = ref('')

// ── 新增對話框 ────────────────────────────────────
const dialogOpen = ref(false)
const newKey = ref('')
const newName = ref('')
const newObject = ref('')
const createError = ref('')
const creating = ref(false)

/**
 * workflow_key 的格式限制
 *
 * 與 form_key 相同的理由：會進 API 路徑與 Temporal 的
 * workflow_id（`{tenant}:{business_object}:{key}`），建立後不能改。
 */
const KEY_PATTERN = /^[a-z][a-z0-9_]*$/

function openDialog(): void {
  newKey.value = ''
  newName.value = ''
  newObject.value = ''
  createError.value = ''
  dialogOpen.value = true
}

async function load(): Promise<void> {
  loading.value = true
  errorMessage.value = ''
  try {
    workflows.value = await workflowsApi.listWorkflows()
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法載入流程，請稍後再試'
  } finally {
    loading.value = false
  }
}

/**
 * 新流程的初始內容
 *
 * 不能送空殼：WF-E001 要求恰一個 trigger 節點與至少一個 end 節點，
 * WF-E002 要求邊的兩端都存在。不符合的話使用者會看到 422
 * 而不是一個可以開始編輯的流程。
 *
 * 給的是最小可用骨架——起點直接連到終點。使用者從中間插入節點即可，
 * 設計器的「插入」功能（insert-{from}-{to}）正是為此設計。
 *
 * trigger 用 form_submit：這套系統的流程幾乎都由表單送出觸發，
 * schedule 與 webhook 是少數情況。
 */
function initialContent(workflowKey: string, name: string, object: string) {
  return {
    workflow_key: workflowKey,
    version: 1,
    business_object: object,
    name,
    trigger: { type: 'form_submit' },
    nodes: [
      { id: 'start', type: 'trigger', label: '送出申請' },
      { id: 'end', type: 'end', label: '結束', result: 'completed' },
    ],
    // edge 是 [from, to] 的陣列，不是 { from, to } 物件
    edges: [['start', 'end']],
  }
}

async function confirmCreate(): Promise<void> {
  const key = newKey.value.trim()
  const name = newName.value.trim()

  if (key === '') {
    createError.value = '請輸入流程代碼'
    return
  }
  if (!KEY_PATTERN.test(key)) {
    createError.value =
      '流程代碼只能用小寫英文、數字與底線，且需以英文開頭（例如 leave_approval）'
    return
  }
  if (name === '') {
    createError.value = '請輸入流程名稱'
    return
  }

  // 業務物件留空時沿用 workflow_key，與表單的處理一致
  const object = newObject.value.trim() === '' ? key : newObject.value.trim()

  creating.value = true
  createError.value = ''
  try {
    await workflowsApi.createWorkflow({
      workflow_key: key,
      business_object: object,
      name,
      content: initialContent(key, name, object),
    })
    router.push(`/designer/workflows/${key}`)
  } catch (error) {
    createError.value =
      error instanceof ApiError ? error.message : '建立失敗，請稍後再試'
  } finally {
    creating.value = false
  }
}

/** 發布狀態。未發布與有草稿是兩件事，要分開顯示。 */
function statusOf(w: WorkflowSummary): { text: string; class: string } {
  if (w.published_version === null) {
    return {
      text: '尚未發布',
      class: 'bg-surface-container-low text-secondary',
    }
  }
  if (w.has_draft) {
    return {
      text: `v${w.published_version} · 有草稿`,
      class: 'bg-[#FEF3C7] text-[#B45309]',
    }
  }
  return {
    text: `v${w.published_version}`,
    class: 'bg-[#DCFCE7] text-[#15803D]',
  }
}

const isEmpty = computed(() => !loading.value && workflows.value.length === 0)

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-secondary">設計器</span>
      <span class="text-outline mx-1">/</span>
      <span class="text-on-surface font-semibold">流程</span>
    </template>

    <div class="max-w-[1000px]">
      <div class="flex items-center justify-between mb-space-lg">
        <div>
          <h1 class="font-title-md text-title-md text-on-surface">流程</h1>
          <p class="font-body-dense text-body-dense text-secondary mt-1">
            建立與維護簽核流程定義
          </p>
        </div>
        <div class="flex items-center gap-space-sm">
          <button
            type="button"
            class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
            data-testid="workflow-refresh"
            @click="load"
          >
            <span class="material-symbols-outlined text-[16px]">refresh</span>
            重新整理
          </button>
          <button
            type="button"
            class="flex items-center gap-1 h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 transition-opacity"
            data-testid="workflow-create"
            @click="openDialog"
          >
            <span class="material-symbols-outlined text-[16px]">add</span>
            建立流程
          </button>
        </div>
      </div>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="workflow-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <div
        v-if="loading"
        data-testid="workflow-loading"
        class="py-space-xl text-center font-body-dense text-body-dense text-secondary"
      >
        載入中…
      </div>

      <div
        v-else-if="isEmpty"
        data-testid="workflow-empty"
        class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
      >
        <span class="material-symbols-outlined text-[40px] text-outline">account_tree</span>
        <p class="font-body-dense text-body-dense">尚未建立任何流程</p>
        <p class="font-body-dense text-body-dense text-outline">
          按右上角的「建立流程」開始。新流程會有起點與終點，從中間插入簽核節點即可
        </p>
      </div>

      <table
        v-else
        class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
        data-testid="workflow-table"
      >
        <thead>
          <tr class="bg-surface-container-low border-b border-outline-variant">
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              名稱
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              代碼
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              業務物件
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              版本
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="w in workflows"
            :key="w.id"
            :data-testid="`workflow-row-${w.workflow_key}`"
            class="border-b border-outline-variant last:border-b-0 hover:bg-surface-container-low transition-colors cursor-pointer"
            @click="router.push(`/designer/workflows/${w.workflow_key}`)"
          >
            <td class="px-space-md py-space-sm font-body-dense text-body-dense text-on-surface">
              {{ w.name }}
            </td>
            <td class="px-space-md py-space-sm font-data-mono text-[12px] text-secondary">
              {{ w.workflow_key }}
            </td>
            <td class="px-space-md py-space-sm font-data-mono text-[12px] text-secondary">
              {{ w.business_object }}
            </td>
            <td class="px-space-md py-space-sm">
              <span
                class="inline-block px-2 py-0.5 rounded font-label-caption text-label-caption"
                :class="statusOf(w).class"
              >
                {{ statusOf(w).text }}
              </span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- ── 新增對話框 ──────────────────────────────── -->
    <div
      v-if="dialogOpen"
      class="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-8"
      data-testid="create-dialog"
      @click.self="dialogOpen = false"
    >
      <div class="bg-surface-container-lowest rounded-xl shadow-2xl w-[520px] max-w-full">
        <div class="px-space-lg py-space-md border-b border-outline-variant">
          <h2 class="font-title-sm text-title-sm text-on-surface">建立流程</h2>
        </div>

        <div class="px-space-lg py-space-md flex flex-col gap-space-md">
          <label class="flex flex-col gap-1">
            <span class="font-label-header text-label-header text-secondary">
              流程名稱 <span class="text-error">*</span>
            </span>
            <input
              v-model="newName"
              type="text"
              placeholder="例如：請假簽核"
              data-testid="new-workflow-name"
              class="h-9 px-3 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
            />
          </label>

          <label class="flex flex-col gap-1">
            <span class="font-label-header text-label-header text-secondary">
              流程代碼 <span class="text-error">*</span>
            </span>
            <input
              v-model="newKey"
              type="text"
              placeholder="例如：leave_approval"
              data-testid="new-workflow-key"
              class="h-9 px-3 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-[12px]"
            />
            <span class="font-label-caption text-label-caption text-outline">
              小寫英文、數字與底線。建立後不能修改。
            </span>
          </label>

          <label class="flex flex-col gap-1">
            <span class="font-label-header text-label-header text-secondary">
              業務物件
            </span>
            <input
              v-model="newObject"
              type="text"
              placeholder="留空則與流程代碼相同"
              data-testid="new-business-object"
              class="h-9 px-3 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-[12px]"
            />
            <!--
              業務物件決定條件式可以引用哪些路徑（WF-E012）。
              沒有定義的業務物件不會被檢查，等於少一層保護。
            -->
            <span class="font-label-caption text-label-caption text-outline">
              決定流程的條件式可以引用哪些資料路徑。應與表單的業務物件一致。
            </span>
          </label>

          <p
            v-if="createError"
            role="alert"
            data-testid="create-error"
            class="px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
          >
            {{ createError }}
          </p>
        </div>

        <div class="px-space-lg py-space-md border-t border-outline-variant flex justify-end gap-space-sm">
          <button
            type="button"
            data-testid="create-cancel"
            class="h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
            @click="dialogOpen = false"
          >
            取消
          </button>
          <button
            type="button"
            data-testid="create-confirm"
            :disabled="creating"
            class="h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 transition-opacity disabled:opacity-50"
            @click="confirmCreate"
          >
            {{ creating ? '建立中…' : '建立並開始設計' }}
          </button>
        </div>
      </div>
    </div>
  </AppShell>
</template>
