<script setup lang="ts">
/**
 * 流程清單與新增
 *
 * 與表單那半（FormList）補的是同一個缺口：後端有 POST /workflows、
 * 前端的 createWorkflow() 也寫好了，但先前**沒有任何畫面呼叫它**——
 * 設計器路由是 /designer/workflows/:workflowKey，只能開既有流程，
 * 選單還寫死指向 quotation_approval。
 *
 * 換句話說：使用者無法從零建立一個流程。
 * 表單那半補完之後補上這半，「從零建一張表單 + 一個流程」
 * 的操作手冊才寫得出來。
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
 * 與 form_key 同一套理由：這個值會進 API 路徑
 * （/workflows/{workflow_key}）與資料表關聯，建立之後不能改。
 *
 * 擋在前端是為了給出可讀的訊息——後端也會擋，
 * 但它的錯誤對使用者來說是天書。
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
 * 新流程的初始 DSL
 *
 * 流程與表單最大的差別：圖結構要通過 WF-E001～E012 才能發布。
 * 產生一個建了卻發布不了的流程比不給建還糟——使用者會以為
 * 是自己設計錯了，而錯誤訊息講的是他沒寫過的節點。
 *
 * 因此起始骨架刻意滿足這幾條：
 *   E001  有 trigger 節點與 end 節點
 *   E003  每個節點都被邊碰到，沒有孤兒
 *   E012  不引用任何資料路徑——新流程的業務物件多半還沒有
 *         定義檔（schemas/business-objects/*.json），
 *         引用任何路徑都會讓發布失敗
 *
 * 中間放一關「主管簽核」而非只有 start→end，理由與表單的起始欄位
 * 相同：使用者進設計器時看得到一個實際的節點，知道節點長什麼樣、
 * 屬性面板怎麼用。而且 manager_of 不引用角色也不引用路徑，
 * 是唯一不需要任何前置設定就能跑的 resolver。
 */
function initialContent(
  workflowKey: string,
  name: string,
  object: string,
): workflowsApi.WorkflowContent {
  return {
    workflow_key: workflowKey,
    version: 1,
    business_object: object,
    name,
    trigger: { type: 'form_submit' },
    nodes: [
      { id: 'start', type: 'trigger', label: '送出申請' },
      {
        id: 'approval',
        type: 'human_approval',
        label: '主管簽核',
        participant: 'internal',
        resolver: { type: 'manager_of', of: 'initiator' },
      },
      { id: 'end', type: 'end', label: '完成', result: 'completed' },
    ],
    edges: [
      ['start', 'approval'],
      ['approval', 'end'],
    ],
  } as workflowsApi.WorkflowContent
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

  // 業務物件留空時沿用 workflow_key。多數情況兩者相同，
  // 強迫使用者填兩次只是徒增困擾。
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
    // 建完直接進設計器，不要讓使用者自己再點一次
    router.push(`/designer/workflows/${key}`)
  } catch (error) {
    // 後端以 409 表達代碼重複。把它的訊息原樣顯示——
    // 換成「建立失敗，請稍後再試」會讓使用者一直重試同一個代碼。
    createError.value =
      error instanceof ApiError ? error.message : '建立失敗，請稍後再試'
  } finally {
    creating.value = false
  }
}

/** 流程的發布狀態。未發布與有草稿是兩件事，要分開顯示。 */
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
  return { text: `v${w.published_version}`, class: 'bg-[#DCFCE7] text-[#15803D]' }
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

      <!--
        空狀態不只說「沒有資料」。使用者第一次進來看到的就是這個畫面，
        要明確告訴他下一步做什麼。
      -->
      <div
        v-else-if="isEmpty"
        data-testid="workflow-empty"
        class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
      >
        <span class="material-symbols-outlined text-[40px] text-outline">
          account_tree
        </span>
        <p class="font-body-dense text-body-dense">尚未建立任何流程</p>
        <p class="font-body-dense text-body-dense text-outline">
          按右上角的「建立流程」開始，或先參考操作手冊
        </p>
      </div>

      <table
        v-else
        class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
        data-testid="workflow-table"
      >
        <thead>
          <tr class="bg-surface-container-low border-b border-outline-variant">
            <th
              class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
            >
              名稱
            </th>
            <th
              class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
            >
              代碼
            </th>
            <th
              class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
            >
              業務物件
            </th>
            <th
              class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
            >
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
            <td
              class="px-space-md py-space-sm font-body-dense text-body-dense text-on-surface"
            >
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
              placeholder="例如：請假簽核流程"
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
            <!-- 事後不能改，所以要先講清楚 -->
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
            <span class="font-label-caption text-label-caption text-outline">
              這個流程處理哪一種單據。要與表單的業務物件一致才接得起來。
            </span>
          </label>

          <!--
            先講清楚會拿到什麼，使用者才不會以為系統擅自加了東西。
          -->
          <p
            class="px-3 py-2 rounded-lg bg-surface-container-low font-label-caption text-label-caption text-secondary"
          >
            會先建立「送出申請 → 主管簽核 → 完成」的基本骨架，
            進設計器後可自由增刪節點。
          </p>

          <p
            v-if="createError"
            role="alert"
            data-testid="create-error"
            class="px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
          >
            {{ createError }}
          </p>
        </div>

        <div
          class="px-space-lg py-space-md border-t border-outline-variant flex justify-end gap-space-sm"
        >
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
