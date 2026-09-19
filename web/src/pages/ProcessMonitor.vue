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
import type { InstanceDetail, WorkflowInstance } from '@/api/instances'
import { ApiError } from '@/api/types'

const items = ref<WorkflowInstance[]>([])
const loading = ref(true)
const errorMessage = ref('')
const statusFilter = ref('')

const detail = ref<InstanceDetail | null>(null)
const detailLoading = ref(false)
const detailError = ref('')

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
    items.value = await instancesApi.list(
      statusFilter.value ? { status: statusFilter.value } : {},
    )
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
  try {
    detail.value = await instancesApi.getOne(item.id)
  } catch (error) {
    detailError.value =
      error instanceof ApiError ? error.message : '無法載入流程詳情'
  } finally {
    detailLoading.value = false
  }
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
          </template>
        </div>

        <div class="px-space-lg py-space-md border-t border-outline-variant flex justify-end">
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
