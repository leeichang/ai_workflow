<script setup lang="ts">
/**
 * 同步中心
 *
 * 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §7、§12.2。
 *
 * 兩個分頁：
 *
 *   **待停用**（§7 規則 3、4）——來源查不到的人。連續消失達門檻後
 *   由管理員確認才停用，**不自動停用**：那等於平台替客戶做人事決定。
 *   每一筆都即時算停用前置檢查，是別人主管或有未完成待辦的人擋下。
 *
 *   **同步歷史**（§10.3）——每次同步的完整計數。同步不能只回
 *   「success」，管理員要看得出兩邊差異有多大。
 *
 * 放同一頁而非兩頁：管理員看完歷史常會想看待停用，反之亦然。
 */
import { computed, onMounted, ref } from 'vue'
import AppShell from '@/components/AppShell.vue'
import * as integrationApi from '@/api/integration'
import type {
  PendingDeactivation,
  SyncIssueRow,
  SyncRun,
} from '@/api/integration'
import { ApiError } from '@/api/types'
import { useSession } from '@/auth/useSession'

const { hasRole } = useSession()
const canManage = computed(() => hasRole('admin'))

type Tab = 'pending' | 'history'
const tab = ref<Tab>('pending')

const pending = ref<PendingDeactivation[]>([])
const runs = ref<SyncRun[]>([])
const loading = ref(true)
const errorMessage = ref('')
const busyId = ref('')

/** 展開中的那一次同步的問題明細 */
const expandedRun = ref<string | null>(null)
const issues = ref<SyncIssueRow[]>([])

/** 達到門檻、且沒有阻擋理由的才可以停用 */
function canDeactivate(row: PendingDeactivation): boolean {
  return row.miss_count >= row.threshold && row.blockers.length === 0
}

async function load(): Promise<void> {
  loading.value = true
  errorMessage.value = ''
  try {
    // 兩支 API 互不相依，平行發出
    const [p, r] = await Promise.all([
      integrationApi.listPendingDeactivations(),
      integrationApi.listRuns(),
    ])
    pending.value = p
    runs.value = r
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法載入，請稍後再試'
  } finally {
    loading.value = false
  }
}

async function confirm(row: PendingDeactivation): Promise<void> {
  busyId.value = row.id
  errorMessage.value = ''
  try {
    await integrationApi.confirmDeactivation(row.id)
    await load()
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '停用失敗，請稍後再試'
  } finally {
    busyId.value = ''
  }
}

async function dismiss(row: PendingDeactivation): Promise<void> {
  busyId.value = row.id
  errorMessage.value = ''
  try {
    await integrationApi.dismissDeactivation(row.id)
    await load()
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '操作失敗，請稍後再試'
  } finally {
    busyId.value = ''
  }
}

async function toggleIssues(run: SyncRun): Promise<void> {
  if (expandedRun.value === run.id) {
    expandedRun.value = null
    return
  }

  expandedRun.value = run.id
  issues.value = []
  try {
    issues.value = await integrationApi.listIssues(run.id)
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法載入問題明細'
  }
}

const STATUS_LABEL: Record<string, { text: string; class: string }> = {
  SUCCESS: { text: '成功', class: 'bg-[#DCFCE7] text-[#15803D]' },
  PARTIAL_SUCCESS: { text: '部分成功', class: 'bg-[#FEF3C7] text-[#92400E]' },
  ABORTED_INCOMPLETE: {
    text: '整批中止',
    class: 'bg-error-container text-on-error-container',
  },
  FAILED: { text: '失敗', class: 'bg-error-container text-on-error-container' },
  RUNNING: { text: '執行中', class: 'bg-primary-container text-on-primary' },
}

const ISSUE_LABEL: Record<string, string> = {
  AMBIGUOUS: '比對到多筆',
  VALIDATION_FAILED: '驗證未過',
  SKIPPED_BY_OWNERSHIP: '平台維護跳過',
  MISSING_IN_SOURCE: '來源消失',
}

function formatDateTime(iso: string): string {
  const d = new Date(iso)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}/${pad(d.getMonth() + 1)}/${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-on-surface font-semibold">同步中心</span>
    </template>

    <div class="max-w-[1000px]">
      <div class="flex items-center justify-between mb-space-lg">
        <div>
          <h1 class="font-title-md text-title-md text-on-surface">同步中心</h1>
          <p class="font-body-dense text-body-dense text-secondary mt-1">
            待處理的人員異動與同步紀錄
          </p>
        </div>
        <button
          type="button"
          class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
          data-testid="sync-refresh"
          @click="load"
        >
          <span class="material-symbols-outlined text-[16px]">refresh</span>
          重新整理
        </button>
      </div>

      <p
        v-if="!canManage"
        data-testid="sync-readonly-hint"
        class="mb-space-md px-3 py-2 rounded-lg bg-surface-container-low text-secondary font-body-dense text-body-dense"
      >
        同步中心需要管理員權限。
      </p>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="sync-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <!-- 分頁 -->
      <div class="flex gap-1 mb-space-md border-b border-outline-variant">
        <button
          type="button"
          class="px-space-md py-space-sm font-label-header text-label-header transition-colors border-b-2 -mb-px"
          :class="
            tab === 'pending'
              ? 'border-primary text-primary'
              : 'border-transparent text-secondary hover:text-on-surface'
          "
          data-testid="sync-tab-pending"
          @click="tab = 'pending'"
        >
          待停用
          <span
            v-if="pending.length > 0"
            class="ml-1 inline-flex items-center justify-center h-4 min-w-4 px-1 rounded-full bg-error text-on-error font-label-caption text-[10px] font-bold"
          >
            {{ pending.length }}
          </span>
        </button>
        <button
          type="button"
          class="px-space-md py-space-sm font-label-header text-label-header transition-colors border-b-2 -mb-px"
          :class="
            tab === 'history'
              ? 'border-primary text-primary'
              : 'border-transparent text-secondary hover:text-on-surface'
          "
          data-testid="sync-tab-history"
          @click="tab = 'history'"
        >
          同步歷史
        </button>
      </div>

      <div
        v-if="loading"
        data-testid="sync-loading"
        class="py-space-xl text-center font-body-dense text-body-dense text-secondary"
      >
        載入中…
      </div>

      <!-- 待停用 -->
      <template v-else-if="tab === 'pending'">
        <div
          v-if="pending.length === 0"
          data-testid="sync-pending-empty"
          class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
        >
          <span class="material-symbols-outlined text-[40px] text-[#15803D]">
            check_circle
          </span>
          <p class="font-body-dense text-body-dense">
            沒有待處理的人員異動
          </p>
        </div>

        <template v-else>
          <p
            class="mb-space-md px-3 py-2 rounded-lg bg-surface-container-low text-secondary font-body-dense text-body-dense"
          >
            這些人在最近的同步中查不到。來源缺漏與離職是兩回事——
            連續消失達到門檻後才建議停用，而且要您確認。
          </p>

          <table
            class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
            data-testid="sync-pending-table"
          >
            <thead>
              <tr class="bg-surface-container-low border-b border-outline-variant">
                <th
                  class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
                >
                  員工
                </th>
                <th
                  class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary w-[120px]"
                >
                  連續消失
                </th>
                <th
                  class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
                >
                  狀況
                </th>
                <th
                  class="text-right px-space-md py-space-sm font-label-header text-label-header text-secondary w-[160px]"
                >
                  &nbsp;
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="row in pending"
                :key="row.id"
                :data-testid="`pending-row-${row.user_id}`"
                class="border-b border-outline-variant last:border-b-0"
              >
                <td class="px-space-md py-space-sm align-top">
                  <p class="font-body-dense text-body-dense text-on-surface">
                    {{ row.name }}
                  </p>
                  <p
                    class="font-label-caption text-label-caption text-secondary mt-0.5"
                  >
                    {{ row.employee_no || '—' }} ·
                    {{ row.department_name ?? '未指定部門' }}
                  </p>
                </td>
                <td class="px-space-md py-space-sm align-top">
                  <p
                    class="font-title-sm text-title-sm"
                    :class="
                      row.miss_count >= row.threshold
                        ? 'text-error'
                        : 'text-on-surface'
                    "
                  >
                    {{ row.miss_count }} / {{ row.threshold }}
                  </p>
                  <p
                    class="font-label-caption text-label-caption text-secondary mt-0.5"
                  >
                    自 {{ formatDateTime(row.first_missed_at) }}
                  </p>
                </td>
                <td class="px-space-md py-space-sm align-top">
                  <!--
                    §7 規則 4：是別人主管、是部門主管、或有未完成待辦的人
                    停用後會讓流程找不到簽核人。阻擋理由要在清單上就看得到，
                    不是按了才知道
                  -->
                  <ul
                    v-if="row.blockers.length > 0"
                    class="flex flex-col gap-0.5"
                    :data-testid="`pending-blockers-${row.user_id}`"
                  >
                    <li
                      v-for="(blocker, index) in row.blockers"
                      :key="index"
                      class="font-body-dense text-body-dense text-error"
                    >
                      {{ blocker }}
                    </li>
                  </ul>
                  <p
                    v-else-if="row.miss_count < row.threshold"
                    class="font-body-dense text-body-dense text-secondary"
                  >
                    還差 {{ row.threshold - row.miss_count }} 次達到門檻
                  </p>
                  <p
                    v-else
                    class="font-body-dense text-body-dense text-on-surface-variant"
                  >
                    可以停用
                  </p>
                </td>
                <td class="px-space-md py-space-sm text-right align-top">
                  <div
                    v-if="canManage"
                    class="inline-flex items-center gap-space-sm"
                  >
                    <button
                      type="button"
                      class="h-7 px-2 rounded-lg text-secondary hover:bg-surface-container-low font-label-header text-label-header disabled:opacity-40"
                      :disabled="busyId === row.id"
                      :data-testid="`pending-dismiss-${row.user_id}`"
                      @click="dismiss(row)"
                    >
                      仍在職
                    </button>
                    <button
                      type="button"
                      class="h-7 px-2 rounded-lg border border-outline-variant text-error hover:bg-error-container font-label-header text-label-header disabled:opacity-40 disabled:cursor-not-allowed"
                      :disabled="!canDeactivate(row) || busyId === row.id"
                      :data-testid="`pending-confirm-${row.user_id}`"
                      @click="confirm(row)"
                    >
                      停用
                    </button>
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </template>
      </template>

      <!-- 同步歷史 -->
      <template v-else>
        <div
          v-if="runs.length === 0"
          data-testid="sync-history-empty"
          class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
        >
          <span class="material-symbols-outlined text-[40px] text-outline">
            history
          </span>
          <p class="font-body-dense text-body-dense">尚無同步紀錄</p>
        </div>

        <table
          v-else
          class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
          data-testid="sync-history-table"
        >
          <thead>
            <tr class="bg-surface-container-low border-b border-outline-variant">
              <th
                class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary w-[150px]"
              >
                時間
              </th>
              <th
                class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary w-[110px]"
              >
                結果
              </th>
              <th
                class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
              >
                計數
              </th>
            </tr>
          </thead>
          <tbody>
            <template v-for="run in runs" :key="run.id">
              <tr
                :data-testid="`run-row-${run.id}`"
                class="border-b border-outline-variant hover:bg-surface-container-low transition-colors cursor-pointer"
                @click="toggleIssues(run)"
              >
                <td class="px-space-md py-space-sm align-top">
                  <p class="font-body-dense text-body-dense text-on-surface">
                    {{ formatDateTime(run.started_at) }}
                  </p>
                  <!-- 試跑也留紀錄，要標示出來否則會被誤認為真的同步過 -->
                  <p
                    v-if="run.dry_run"
                    class="font-label-caption text-label-caption text-secondary mt-0.5"
                  >
                    試跑
                  </p>
                </td>
                <td class="px-space-md py-space-sm align-top">
                  <span
                    class="inline-block px-2 py-0.5 rounded font-label-caption text-label-caption whitespace-nowrap"
                    :class="STATUS_LABEL[run.status]?.class"
                  >
                    {{ STATUS_LABEL[run.status]?.text ?? run.status }}
                  </span>
                </td>
                <td class="px-space-md py-space-sm align-top">
                  <p class="font-body-dense text-body-dense text-on-surface">
                    讀到 {{ run.received_count }}、新增
                    {{ run.inserted_count }}、更新 {{ run.updated_count }}
                  </p>
                  <p
                    class="font-label-caption text-label-caption text-secondary mt-0.5"
                  >
                    <span v-if="run.rejected_count > 0" class="text-error">
                      擋下 {{ run.rejected_count }}、
                    </span>
                    <span v-if="run.skipped_by_ownership_count > 0">
                      平台維護跳過 {{ run.skipped_by_ownership_count }} 個欄位、
                    </span>
                    <span v-if="run.missing_in_source_count > 0">
                      來源消失 {{ run.missing_in_source_count }} 人
                    </span>
                    <span
                      v-if="
                        run.rejected_count === 0 &&
                        run.skipped_by_ownership_count === 0 &&
                        run.missing_in_source_count === 0
                      "
                    >
                      沒有問題
                    </span>
                  </p>
                  <p
                    v-if="run.message"
                    class="font-body-dense text-body-dense text-error mt-1"
                    :data-testid="`run-message-${run.id}`"
                  >
                    {{ run.message }}
                  </p>
                </td>
              </tr>

              <tr v-if="expandedRun === run.id" class="border-b border-outline-variant">
                <td
                  colspan="3"
                  class="px-space-md py-space-sm bg-surface-container-low"
                  :data-testid="`run-issues-${run.id}`"
                >
                  <p
                    v-if="issues.length === 0"
                    class="font-body-dense text-body-dense text-secondary"
                  >
                    這次同步沒有問題明細
                  </p>
                  <ul v-else class="flex flex-col gap-1">
                    <li
                      v-for="issue in issues"
                      :key="issue.id"
                      class="font-body-dense text-body-dense text-on-surface-variant"
                    >
                      <span
                        class="inline-block px-1 rounded bg-surface-container-lowest font-label-caption text-label-caption text-secondary mr-1"
                      >
                        {{ ISSUE_LABEL[issue.kind] ?? issue.kind }}
                      </span>
                      <span
                        v-if="issue.row_number"
                        class="font-data-mono text-[11px] text-secondary"
                      >
                        第 {{ issue.row_number }} 列：
                      </span>
                      {{ issue.message }}
                    </li>
                  </ul>
                </td>
              </tr>
            </template>
          </tbody>
        </table>
      </template>
    </div>
  </AppShell>
</template>
