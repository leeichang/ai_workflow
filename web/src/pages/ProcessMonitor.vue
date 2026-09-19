<script setup lang="ts">
/**
 * 流程監控（S0）
 *
 * 交辦（§5 S0）：
 *   「選單有『流程監控』，看得到所有執行中流程的清單與詳情。
 *     權限正確（一般使用者看不到別人的單），欄位遮蔽正確。」
 *
 * 可見範圍由後端決定（`instances.rs` 的 `visible_to`）——
 * 前端不做過濾。前端過濾等於把資料送到瀏覽器再假裝看不到，
 * 打開開發者工具就破功。
 */
import { computed, onMounted, ref } from 'vue'
import AppShell from '@/components/AppShell.vue'
import * as instancesApi from '@/api/instances'
import * as orgApi from '@/api/org'
import type { HumanTask, InstanceDetail, WorkflowInstance } from '@/api/instances'
import { ApiError } from '@/api/types'
import { useSession } from '@/auth/useSession'

const items = ref<WorkflowInstance[]>([])
const loading = ref(true)
const errorMessage = ref('')
const statusFilter = ref('')
/** 只看需要人介入的。導入期通常先把卡住的處理完 */
const attentionOnly = ref(false)

const detail = ref<InstanceDetail | null>(null)
const detailLoading = ref(false)
const detailError = ref('')

/**
 * 介入權限（N1，2026-09-19 定案）
 *
 * `hasRole` 對 admin 一律回 true，與後端 `can_monitor_all`
 * （admin || process_monitor）的語意一致。
 *
 * 這只是 UX 層的隱藏，**不是權限控制**——後端每個端點都會再擋。
 * 只靠前端不顯示按鈕等於沒有權限。
 */
const { hasRole } = useSession()
const canIntervene = computed(() => hasRole('process_monitor'))

/** 卡在誰身上。流程狀態只說 RUNNING，卡在誰是待辦才知道 */
const tasks = ref<HumanTask[]>([])
const actionMessage = ref('')
const actionError = ref('')
const acting = ref(false)

/** 改派的目標人選。只在真的要改派時才載入，不佔開啟詳情的時間 */
const employees = ref<orgApi.Employee[]>([])
const reassigning = ref<HumanTask | null>(null)
const reassignTo = ref('')

const TASK_STATUS_LABEL: Record<string, string> = {
  PENDING: '待處理',
  APPROVED: '已核准',
  REJECTED: '已退回',
  CANCELLED: '已取消',
}

/**
 * 異常代碼的說明與處理建議
 *
 * 用代碼分支而非比對 `attention_detail` 的中文：訊息會改寫，
 * 代碼不會。未知代碼落回 detail 原文，不要顯示空白——
 * 後端新增代碼時前端還沒跟上是正常的，但不能因此什麼都不說。
 */
const ATTENTION_HINT: Record<string, string> = {
  ESCALATE_UNRESOLVED:
    '逾時要加簽給別人，但解析不到對象。通常是該角色沒有成員，或主管欄位是空的。到組織健康檢查看看，修好後回來按「已處理」。',
}

const attentionHint = computed(() => {
  const d = detail.value
  if (d === null || !d.needs_attention) return ''
  return ATTENTION_HINT[d.attention_code ?? ''] ?? ''
})

const STATUS_LABEL: Record<string, string> = {
  RUNNING: '執行中',
  COMPLETED: '已完成',
  REJECTED: '已退回',
  CANCELLED: '已取消',
  FAILED: '失敗',
}

const STATUS_CLASS: Record<string, string> = {
  RUNNING: 'bg-[#DBEAFE] text-[#1D4ED8]',
  COMPLETED: 'bg-[#DCFCE7] text-[#166534]',
  REJECTED: 'bg-[#FEE2E2] text-[#991B1B]',
  CANCELLED: 'bg-surface-container-low text-secondary',
  FAILED: 'bg-[#FEE2E2] text-[#991B1B]',
}

async function load(): Promise<void> {
  loading.value = true
  errorMessage.value = ''
  try {
    items.value = await instancesApi.list({
      ...(statusFilter.value ? { status: statusFilter.value } : {}),
      needs_attention: attentionOnly.value,
      // 後端預設 50。監控是營運要「看完」的清單，不是收件匣，
      // 50 筆在有幾十條流程同時在跑的租戶會看不到全部——
      // 而看不到的那幾筆正可能是卡住的。上限 200 由後端 clamp。
      limit: 200,
    })
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法載入流程清單，請稍後再試'
  } finally {
    loading.value = false
  }
}

async function open(item: WorkflowInstance): Promise<void> {
  detail.value = null
  detailError.value = ''
  detailLoading.value = true
  tasks.value = []
  actionMessage.value = ''
  actionError.value = ''
  reassigning.value = null
  try {
    detail.value = await instancesApi.getOne(item.id)
    // 待辦另外抓。抓不到不該讓整個詳情開不起來——
    // 流程資訊本身仍然有用，少的只是「卡在誰身上」
    if (canIntervene.value) {
      try {
        tasks.value = await instancesApi.listTasks(item.id)
      } catch {
        actionError.value = '無法載入待辦清單'
      }
    }
  } catch (error) {
    detailError.value =
      error instanceof ApiError ? error.message : '無法載入流程詳情'
  } finally {
    detailLoading.value = false
  }
}

/** 重新載入詳情與待辦。介入動作之後畫面要反映結果 */
async function refreshDetail(): Promise<void> {
  const current = detail.value
  if (current === null) return
  detail.value = await instancesApi.getOne(current.id)
  tasks.value = await instancesApi.listTasks(current.id)
}

/**
 * 包住介入動作的共用流程：擋重複點擊、清訊息、統一錯誤處理
 *
 * 動作本身與「重新載入畫面」分成兩段 try，因為兩者的失敗意義不同：
 * 催辦成功但重載失敗時，信**已經寄出去了**。把它一起當成失敗會讓
 * 使用者以為沒寄到而再按一次，於是收件人收到兩封。
 *
 * 所以成功訊息照留，重載失敗只在旁邊提醒畫面可能不是最新的。
 */
async function act(run: () => Promise<string>): Promise<void> {
  acting.value = true
  actionMessage.value = ''
  actionError.value = ''
  try {
    actionMessage.value = await run()
  } catch (error) {
    actionError.value =
      error instanceof ApiError ? error.message : '操作失敗，請稍後再試'
    acting.value = false
    return
  }

  try {
    await refreshDetail()
  } catch {
    actionError.value = '動作已完成，但畫面沒有更新成功，請重新整理'
  } finally {
    acting.value = false
  }
}

async function remind(): Promise<void> {
  await act(async () => {
    const r = await instancesApi.remind(detail.value!.id)
    if (r.notified === 0) {
      // 催不到也要說清楚原因，不能回一句「已送出」讓人以為催過了
      return r.note ?? '沒有可以催辦的對象'
    }
    const skipped =
      r.skipped_role_tasks > 0
        ? `（另有 ${r.skipped_role_tasks} 筆指派給角色，定位不到個人信箱）`
        : ''
    return `已寄出 ${r.notified} 封提醒${skipped}`
  })
}

async function resolveAttention(): Promise<void> {
  await act(async () => {
    await instancesApi.resolveAttention(detail.value!.id)
    // 清掉之後這張單會從「只看異常」的清單消失，主清單要跟著更新
    await load()
    return '已標記為處理完成'
  })
}

async function cancelInstance(): Promise<void> {
  // 取消是不可逆的：進行中的單會結束，待辦全部撤掉。
  // 監控角色取消的是**別人的**單，更要確認。
  if (!window.confirm('取消後流程會結束，所有待辦都會撤銷。確定要取消嗎？')) {
    return
  }
  await act(async () => {
    await instancesApi.cancel(detail.value!.id, '流程監控人員取消')
    await load()
    return '取消請求已送出'
  })
}

/** 開啟改派。員工清單延後到這時才載入 */
async function startReassign(task: HumanTask): Promise<void> {
  reassigning.value = task
  reassignTo.value = ''
  actionError.value = ''
  if (employees.value.length === 0) {
    try {
      employees.value = await orgApi.listEmployees({})
    } catch {
      actionError.value = '無法載入員工清單'
    }
  }
}

async function confirmReassign(): Promise<void> {
  const task = reassigning.value
  if (task === null || reassignTo.value === '') return
  await act(async () => {
    await instancesApi.reassignTask(task.id, reassignTo.value, '流程卡住，監控人員改派')
    reassigning.value = null
    return '已改派'
  })
}

/** 已結束的本地狀態 */
const TERMINAL = ['COMPLETED', 'REJECTED', 'CANCELLED', 'FAILED']

/**
 * 本地狀態與 Temporal 即時狀態是否**真的**不一致
 *
 * 不能直接比字串。流程自己處理完取消後是正常返回的，
 * Temporal 看到的是「執行完成」，本地記的是業務結果「已取消」——
 * 兩者都對，只是講的是不同層次的事。那樣比會讓每一筆取消的單
 * 都跳警告，而**會叫錯的警告比沒有警告更糟**。
 *
 * 真正值得標出來的只有「一邊說還在跑、另一邊說結束了」：
 * 那代表投影落後或對帳出問題。
 */
const statusMismatch = computed(() => {
  const d = detail.value
  if (d === null || d.live_status === null) return false

  const liveRunning = d.live_status.toUpperCase() === 'RUNNING'
  const localRunning = !TERMINAL.includes(d.status.toUpperCase())
  return liveRunning !== localRunning
})

const empty = computed(
  () => !loading.value && errorMessage.value === '' && items.value.length === 0,
)

function at(iso: string | null): string {
  if (iso === null) return '—'
  const d = new Date(iso)
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-secondary">營運</span>
      <span class="text-outline mx-1">/</span>
      <span class="text-on-surface font-semibold">流程監控</span>
    </template>

    <div class="max-w-[1100px]">
      <div class="flex items-center justify-between mb-space-lg">
        <div>
          <h1 class="font-title-md text-title-md text-on-surface">流程監控</h1>
          <p class="font-body-dense text-body-dense text-secondary mt-1">
            執行中與已結束的流程。只列出你看得到的單
          </p>
        </div>
        <div class="flex items-center gap-2">
          <select
            v-model="statusFilter"
            data-testid="monitor-status"
            class="h-8 px-2 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense"
            @change="load"
          >
            <option value="">全部狀態</option>
            <option value="RUNNING">執行中</option>
            <option value="COMPLETED">已完成</option>
            <option value="REJECTED">已退回</option>
            <option value="CANCELLED">已取消</option>
          </select>
          <label
            class="flex items-center gap-2 font-body-dense text-body-dense text-on-surface-variant cursor-pointer"
          >
            <input
              v-model="attentionOnly"
              type="checkbox"
              data-testid="monitor-attention-only"
              class="accent-primary"
              @change="load"
            />
            只看需要處理的
          </label>
          <button
            type="button"
            data-testid="monitor-refresh"
            class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
            @click="load"
          >
            <span class="material-symbols-outlined text-[16px]">refresh</span>
            重新整理
          </button>
        </div>
      </div>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="monitor-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <div
        v-if="loading"
        data-testid="monitor-loading"
        class="py-space-xl text-center font-body-dense text-body-dense text-secondary"
      >
        載入中…
      </div>

      <!--
        空清單要說清楚是「沒有你看得到的單」而非「系統沒有單」——
        兩者差很多，後者會讓使用者以為流程沒跑起來
      -->
      <div
        v-else-if="empty"
        data-testid="monitor-empty"
        class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
      >
        <span class="material-symbols-outlined text-[40px] text-outline">
          monitoring
        </span>
        <p class="font-body-dense text-body-dense">沒有你看得到的流程</p>
        <p class="font-body-dense text-body-dense text-outline max-w-[420px] text-center">
          這裡只會列出你發起的、或你有待辦的單。
        </p>
      </div>

      <table
        v-else
        data-testid="monitor-table"
        class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
      >
        <thead>
          <tr class="bg-surface-container-low border-b border-outline-variant">
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              單號
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              業務物件
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              狀態
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              啟動時間
            </th>
            <th class="px-space-md py-space-sm"></th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="it in items"
            :key="it.id"
            :data-testid="`monitor-row-${it.business_key}`"
            class="border-b border-outline-variant last:border-b-0"
          >
            <td class="px-space-md py-space-sm font-data-mono text-[12px] text-on-surface">
              {{ it.business_key }}
            </td>
            <td class="px-space-md py-space-sm font-body-dense text-body-dense text-secondary">
              {{ it.business_object }}
            </td>
            <td class="px-space-md py-space-sm">
              <span
                class="px-2 py-0.5 rounded font-label-caption text-label-caption"
                :class="STATUS_CLASS[it.status] ?? 'bg-surface-container-low text-secondary'"
              >
                {{ STATUS_LABEL[it.status] ?? it.status }}
              </span>
              <!--
                異常與狀態並列而非取代：這張單同時「執行中」且
                「需要處理」，兩件事都要看得到
              -->
              <span
                v-if="it.needs_attention"
                :data-testid="`monitor-attention-${it.business_key}`"
                class="ml-1 px-2 py-0.5 rounded font-label-caption text-label-caption bg-error-container text-on-error-container whitespace-nowrap"
              >
                需要處理
              </span>
            </td>
            <td class="px-space-md py-space-sm font-body-dense text-body-dense text-secondary">
              {{ at(it.started_at) }}
            </td>
            <td class="px-space-md py-space-sm text-right">
              <button
                type="button"
                :data-testid="`monitor-open-${it.business_key}`"
                class="h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
                @click="open(it)"
              >
                詳情
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- ── 詳情 ──────────────────────────────────── -->
    <div
      v-if="detail !== null || detailLoading || detailError"
      class="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-8"
      data-testid="monitor-detail"
      @click.self="detail = null; detailError = ''"
    >
      <div class="bg-surface-container-lowest rounded-xl shadow-2xl w-[620px] max-w-full max-h-[80vh] overflow-auto">
        <div class="px-space-lg py-space-md border-b border-outline-variant">
          <h2 class="font-title-sm text-title-sm text-on-surface">流程詳情</h2>
        </div>

        <div class="px-space-lg py-space-md flex flex-col gap-space-md">
          <p v-if="detailLoading" class="font-body-dense text-body-dense text-secondary">
            載入中…
          </p>
          <p
            v-else-if="detailError"
            role="alert"
            data-testid="monitor-detail-error"
            class="px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
          >
            {{ detailError }}
          </p>

          <template v-else-if="detail !== null">
            <dl class="grid grid-cols-[120px_1fr] gap-y-2 font-body-dense text-body-dense">
              <dt class="text-secondary">單號</dt>
              <dd class="font-data-mono text-[12px]">{{ detail.business_key }}</dd>
              <dt class="text-secondary">狀態</dt>
              <dd data-testid="monitor-detail-status">
                {{ STATUS_LABEL[detail.status] ?? detail.status }}
              </dd>
              <dt class="text-secondary">引擎回報</dt>
              <dd data-testid="monitor-detail-live">
                {{ detail.live_status ?? '查不到（可能已超過保留期）' }}
              </dd>
              <dt class="text-secondary">啟動</dt>
              <dd>{{ at(detail.started_at) }}</dd>
              <dt class="text-secondary">結束</dt>
              <dd>{{ at(detail.ended_at) }}</dd>
            </dl>

            <!--
              本地狀態是投影，可能落後於 Temporal。
              不一致本身就是有用的訊號，要標出來而不是挑一個顯示。
            -->
            <p
              v-if="statusMismatch"
              data-testid="monitor-status-mismatch"
              class="px-3 py-2 rounded-lg bg-[#FEF3C7] text-[#B45309] font-body-dense text-body-dense"
            >
              本地狀態與流程引擎回報不一致，資料可能還在同步中。
            </p>

            <p
              v-if="detail.error"
              data-testid="monitor-detail-failure"
              class="px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
            >
              {{ detail.error }}
            </p>

            <!--
              異常說明（N3）
              先寫發生什麼事，再寫怎麼處理。只說「解析不到簽核人」
              使用者不知道要去哪裡修，那個提示就等於沒有用
            -->
            <div
              v-if="detail.needs_attention"
              data-testid="monitor-attention-box"
              class="px-3 py-2 rounded-lg bg-error-container text-on-error-container flex flex-col gap-1"
            >
              <p class="font-label-header text-label-header">需要人介入</p>
              <p class="font-body-dense text-body-dense">
                {{ detail.attention_detail }}
              </p>
              <p
                v-if="attentionHint"
                class="font-body-dense text-body-dense opacity-80"
              >
                {{ attentionHint }}
              </p>
            </div>

            <!-- ── 卡在誰身上 ────────────────────────── -->
            <template v-if="canIntervene && tasks.length > 0">
              <div class="flex flex-col gap-1">
                <p class="font-label-header text-label-header text-secondary">
                  待辦
                </p>
                <div
                  v-for="t in tasks"
                  :key="t.id"
                  :data-testid="`monitor-task-${t.node_id}`"
                  class="flex items-center justify-between gap-2 px-3 py-2 rounded-lg border border-outline-variant font-body-dense text-body-dense"
                >
                  <div>
                    <p class="text-on-surface">{{ t.node_label ?? t.node_id }}</p>
                    <p class="font-label-caption text-label-caption text-secondary">
                      {{ TASK_STATUS_LABEL[t.status] ?? t.status }}
                      <template v-if="t.assignee_role">
                        ・角色：{{ t.assignee_role }}
                      </template>
                    </p>
                  </div>
                  <button
                    v-if="t.status === 'PENDING'"
                    type="button"
                    :disabled="acting"
                    :data-testid="`monitor-reassign-${t.node_id}`"
                    class="h-7 px-2 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header disabled:opacity-50 whitespace-nowrap"
                    @click="startReassign(t)"
                  >
                    改派
                  </button>
                </div>
              </div>

              <!-- 改派的目標 -->
              <div
                v-if="reassigning !== null"
                data-testid="monitor-reassign-panel"
                class="flex items-center gap-2 px-3 py-2 rounded-lg bg-surface-container-low"
              >
                <select
                  v-model="reassignTo"
                  data-testid="monitor-reassign-to"
                  class="flex-1 h-8 px-2 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense"
                >
                  <option value="">選擇改派對象…</option>
                  <option v-for="e in employees" :key="e.id" :value="e.id">
                    {{ e.name }}{{ e.department_name ? `（${e.department_name}）` : '' }}
                  </option>
                </select>
                <button
                  type="button"
                  :disabled="acting || reassignTo === ''"
                  data-testid="monitor-reassign-confirm"
                  class="h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header disabled:opacity-50"
                  @click="confirmReassign"
                >
                  確認改派
                </button>
                <button
                  type="button"
                  class="h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant font-label-header text-label-header"
                  @click="reassigning = null"
                >
                  取消
                </button>
              </div>
            </template>

            <p
              v-if="actionMessage"
              data-testid="monitor-action-message"
              class="px-3 py-2 rounded-lg bg-[#DCFCE7] text-[#166534] font-body-dense text-body-dense"
            >
              {{ actionMessage }}
            </p>
            <p
              v-if="actionError"
              role="alert"
              data-testid="monitor-action-error"
              class="px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
            >
              {{ actionError }}
            </p>
          </template>
        </div>

        <div class="px-space-lg py-space-md border-t border-outline-variant flex justify-between items-center gap-2">
          <!--
            介入動作（N1）。只有 admin 與 process_monitor 看得到。
            後端每個端點都會再擋一次——前端隱藏只是 UX，不是權限。

            催辦放最左：卡住最常見的解法是催人，不是取消。
            取消放最右且用紅字：它是不可逆的。
          -->
          <div v-if="canIntervene && detail !== null" class="flex items-center gap-2">
            <button
              v-if="detail.status === 'RUNNING'"
              type="button"
              :disabled="acting"
              data-testid="monitor-remind"
              class="h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header disabled:opacity-50"
              @click="remind"
            >
              催辦
            </button>
            <button
              v-if="detail.needs_attention"
              type="button"
              :disabled="acting"
              data-testid="monitor-resolve-attention"
              class="h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header disabled:opacity-50"
              @click="resolveAttention"
            >
              標記已處理
            </button>
            <button
              v-if="detail.status === 'RUNNING'"
              type="button"
              :disabled="acting"
              data-testid="monitor-cancel"
              class="h-8 px-3 rounded-lg border border-error text-error hover:bg-error-container font-label-header text-label-header disabled:opacity-50"
              @click="cancelInstance"
            >
              取消流程
            </button>
          </div>
          <div v-else></div>
          <button
            type="button"
            data-testid="monitor-detail-close"
            class="h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
            @click="detail = null; detailError = ''"
          >
            關閉
          </button>
        </div>
      </div>
    </div>
  </AppShell>
</template>
