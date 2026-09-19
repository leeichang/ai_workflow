<script setup lang="ts">
/**
 * 模擬簽核（開發模式）
 *
 * 使用者的原話：
 *   「執行時可以直接由系統依據目前的系統角色設定解析出實際簽核的人員，
 *     測試的人可以直接點選模擬這個人簽核，系統依據權限設定顯示每個
 *     簽核人員的畫面，讓使用者模擬簽核！一一完成後最後整個流程完成！」
 *
 * 核心是附身模式：測試者始終是同一個真實使用者，
 * 點一下就以某個人的身分簽核，不必換身分登入五次。
 */
import { computed, onMounted, ref } from 'vue'
import AppShell from '@/components/AppShell.vue'
import * as sandboxApi from '@/api/sandbox'
import type {
  BranchProgress,
  PendingTask,
  SandboxSession,
  SimulatedForm,
} from '@/api/sandbox'
import { ApiError } from '@/api/types'

const sessions = ref<SandboxSession[]>([])
const tasks = ref<PendingTask[]>([])
const loading = ref(true)
const errorMessage = ref('')

// ── 扮演 ──────────────────────────────────────────
const acting = ref<PendingTask | null>(null)
const comment = ref('')
const submitting = ref(false)
const simError = ref('')

// ── 被扮演者會看到的畫面 ──────────────────────────
const form = ref<SimulatedForm | null>(null)
const formLoading = ref(false)
const formError = ref('')

const current = computed(() => sessions.value[0] ?? null)
const sandboxId = computed(() => current.value?.sandbox_tenant_id ?? null)

async function load(): Promise<void> {
  loading.value = true
  errorMessage.value = ''
  try {
    sessions.value = await sandboxApi.listSandboxes()
    tasks.value = sandboxId.value
      ? await sandboxApi.listSandboxTasks(sandboxId.value)
      : []
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法載入開發模式，請稍後再試'
  } finally {
    loading.value = false
  }
}

async function create(): Promise<void> {
  errorMessage.value = ''
  try {
    await sandboxApi.createSandbox()
    await load()
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '建立開發模式失敗'
  }
}

async function startActing(task: PendingTask): Promise<void> {
  acting.value = task
  comment.value = ''
  simError.value = ''
  form.value = null
  formError.value = ''

  const sid = sandboxId.value
  if (sid === null) return

  // 權限過濾畫面：欄位可見性依**被扮演者**的角色重算。
  // 這是使用者原話裡的第三件事——「系統依據權限設定顯示每個
  // 簽核人員的畫面」。少了它，模擬只證明流程會動，
  // 沒證明那個人簽核時看得到該看的、看不到不該看的。
  formLoading.value = true
  try {
    form.value = await sandboxApi.getSimulatedForm(sid, task.task_id)
  } catch (error) {
    formError.value =
      error instanceof ApiError ? error.message : '無法載入欄位權限'
  } finally {
    formLoading.value = false
  }
}

/** 隱藏的欄位要單獨數——「看不到什麼」和「看得到什麼」一樣重要 */
const hiddenCount = computed(
  () => form.value?.fields.filter((f) => f.permission === 'HIDDEN').length ?? 0,
)
const visibleFields = computed(
  () => form.value?.fields.filter((f) => f.permission !== 'HIDDEN') ?? [],
)
const hiddenFields = computed(
  () => form.value?.fields.filter((f) => f.permission === 'HIDDEN') ?? [],
)

/**
 * 把待簽依會簽組分群
 *
 * 平行分支同時產生多張待辦。平鋪列出的話使用者看不出
 * 「這兩張是同一個會簽、要兩邊都簽完才會往下走」，
 * 會誤以為流程分岔成兩條路。
 */
interface TaskGroup {
  key: string
  branch: BranchProgress | null
  tasks: PendingTask[]
}

const groups = computed<TaskGroup[]>(() => {
  const out: TaskGroup[] = []
  const index = new Map<string, TaskGroup>()

  for (const t of tasks.value) {
    // 沒有會簽的各自成組，維持原本一列一張的樣子。
    // 用寬鬆比較：欄位缺漏（舊版後端、部分回應）時要當成沒有會簽，
    // 而不是讓整張待簽清單因為一個欄位而消失。
    if (t.branch == null) {
      out.push({ key: t.task_id, branch: null, tasks: [t] })
      continue
    }
    // 同一個實例的同一個 parallel 才是同一組——
    // 只用 parallel_id 會把不同單據的同名會簽混在一起
    const key = `${t.instance_id}:${t.branch.parallel_id}`
    const existing = index.get(key)
    if (existing) {
      existing.tasks.push(t)
      continue
    }
    const group: TaskGroup = { key, branch: t.branch, tasks: [t] }
    index.set(key, group)
    out.push(group)
  }
  return out
})

// ── 時間快轉 ──────────────────────────────────────
const skipping = ref<string | null>(null)
const skipMessage = ref('')
const skipError = ref('')

/** ISO 8601 duration 講成人話。P2D → 2 天 */
function humanDuration(iso: string): string {
  const m = /^P(?:(\d+)D)?(?:T(?:(\d+)H)?(?:(\d+)M)?)?$/.exec(iso)
  if (m === null) return iso
  const [, d, h, min] = m
  const parts: string[] = []
  if (d) parts.push(`${d} 天`)
  if (h) parts.push(`${h} 小時`)
  if (min) parts.push(`${min} 分`)
  return parts.length > 0 ? parts.join('') : iso
}

const POLICY_LABEL: Record<string, string> = {
  WAIT: '繼續等待',
  AUTO_APPROVE: '視為通過',
  AUTO_REJECT: '視為退回',
  ESCALATE: '加簽給上級',
}

/**
 * 讓等待立刻到期
 *
 * 不假造決策——後端跑的是真正的逾時處理。所以按鈕上要寫清楚
 * 「快轉後會發生什麼」：AUTO_REJECT 的節點快轉後是退回，
 * 使用者以為是通過的話，會帶著錯誤的結論上線。
 */
async function skip(task: PendingTask): Promise<void> {
  const sid = sandboxId.value
  if (sid === null || task.timeout == null) return

  skipping.value = task.task_id
  skipError.value = ''
  skipMessage.value = ''
  try {
    const r = await sandboxApi.skipTime(sid, task.task_id)
    skipMessage.value =
      `已快轉「${task.node_label ?? task.node_id}」的 ${humanDuration(r.after)} 等待，` +
      `套用逾時策略：${POLICY_LABEL[r.policy] ?? r.policy}`
    await load()
  } catch (error) {
    skipError.value =
      error instanceof ApiError ? error.message : '快轉失敗，請稍後再試'
  } finally {
    skipping.value = null
  }
}

/** 會簽的通過條件，用使用者看得懂的話講 */
function joinRule(b: BranchProgress): string {
  if (b.completion === 'ANY') return '任一分支通過即可'
  if (b.completion === 'N_OF_M') return `${b.n ?? '?'} 條分支通過即可`
  return '所有分支都要通過'
}

const PERMISSION_LABEL: Record<string, string> = {
  EDITABLE: '可編輯',
  READONLY: '唯讀',
  HIDDEN: '看不到',
}

/**
 * 以扮演的身分送出決策
 *
 * 稽核會同時記真正按下按鈕的人與扮演對象——只記扮演對象的話，
 * 會變成「張文華核准了他不知道的事」。
 */
async function decide(decision: 'APPROVE' | 'REJECT'): Promise<void> {
  const task = acting.value
  const sid = sandboxId.value
  if (task === null || task.assignee_user_id === null || sid === null) return

  submitting.value = true
  simError.value = ''
  try {
    await sandboxApi.simulateDecision(
      sid,
      task.task_id,
      task.assignee_user_id,
      decision,
      comment.value,
    )
    acting.value = null
    // 流程前進需要 Worker 走到下一個節點，重載會看到新的待簽項目
    await load()
  } catch (error) {
    simError.value =
      error instanceof ApiError ? error.message : '送出失敗，請稍後再試'
  } finally {
    submitting.value = false
  }
}

const hasSandbox = computed(() => current.value !== null)
const noTasks = computed(
  () => !loading.value && hasSandbox.value && tasks.value.length === 0,
)

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-secondary">開發模式</span>
      <span class="text-outline mx-1">/</span>
      <span class="text-on-surface font-semibold">模擬簽核</span>
    </template>

    <!--
      不可關閉的開發模式標示。
      使用者在開發模式核准了，若以為正式也核准了，那比不給模擬更糟。
    -->
    <div
      v-if="hasSandbox"
      data-testid="sandbox-banner"
      class="mb-space-lg px-space-md py-space-sm rounded-lg bg-[#FEF3C7] border border-[#F59E0B] flex items-center gap-2"
    >
      <span class="material-symbols-outlined text-[18px] text-[#B45309]">
        science
      </span>
      <div class="font-body-dense text-body-dense text-[#B45309]">
        <strong>開發模式</strong>
        ——這裡的操作不影響正式資料，通知也只會寄給你自己。
      </div>
    </div>

    <div class="max-w-[1000px]">
      <div class="flex items-center justify-between mb-space-lg">
        <div>
          <h1 class="font-title-md text-title-md text-on-surface">模擬簽核</h1>
          <p class="font-body-dense text-body-dense text-secondary mt-1">
            由系統解析實際簽核人員，點選即可以那個人的身分簽核
          </p>
        </div>
        <button
          type="button"
          data-testid="sim-refresh"
          class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
          @click="load"
        >
          <span class="material-symbols-outlined text-[16px]">refresh</span>
          重新整理
        </button>
      </div>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="sim-load-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <p
        v-if="skipMessage"
        data-testid="sim-skip-message"
        class="mb-space-md px-3 py-2 rounded-lg bg-[#EFF6FF] text-[#1D4ED8] font-body-dense text-body-dense"
      >
        {{ skipMessage }}
      </p>
      <p
        v-if="skipError"
        role="alert"
        data-testid="sim-skip-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ skipError }}
      </p>

      <div
        v-if="loading"
        data-testid="sim-loading"
        class="py-space-xl text-center font-body-dense text-body-dense text-secondary"
      >
        載入中…
      </div>

      <!-- 還沒有開發模式 -->
      <div
        v-else-if="!hasSandbox"
        data-testid="sandbox-empty"
        class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
      >
        <span class="material-symbols-outlined text-[40px] text-outline">
          science
        </span>
        <p class="font-body-dense text-body-dense">尚未進入開發模式</p>
        <p class="font-body-dense text-body-dense text-outline max-w-[420px] text-center">
          開發模式會複製一份目前的表單、流程與組織設定，
          在裡面怎麼改都不影響正式資料。
        </p>
        <button
          type="button"
          data-testid="sandbox-create"
          class="mt-2 flex items-center gap-1 h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 transition-opacity"
          @click="create"
        >
          <span class="material-symbols-outlined text-[16px]">add</span>
          進入開發模式
        </button>
      </div>

      <!-- 沒有待簽 -->
      <div
        v-else-if="noTasks"
        data-testid="sim-no-tasks"
        class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
      >
        <span class="material-symbols-outlined text-[40px] text-outline">
          task_alt
        </span>
        <p class="font-body-dense text-body-dense">目前沒有待簽項目</p>
        <p class="font-body-dense text-body-dense text-outline">
          流程已經走完，或還沒有啟動任何單據
        </p>
      </div>

      <!-- 待簽清單 -->
      <table
        v-else
        class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
        data-testid="sim-task-table"
      >
        <thead>
          <tr class="bg-surface-container-low border-b border-outline-variant">
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              單號
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              目前節點
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              解析出的簽核人
            </th>
            <th class="px-space-md py-space-sm"></th>
          </tr>
        </thead>
        <tbody>
          <template v-for="g in groups" :key="g.key">
            <!--
              會簽組的標頭。平行分支會同時產生多張待辦——
              平鋪列出使用者會以為流程分岔成兩條獨立的路，
              而實際上要兩邊都簽完才會往下走。
            -->
            <tr
              v-if="g.branch !== null"
              :data-testid="`sim-branch-${g.branch.parallel_id}`"
              class="bg-[#EFF6FF] border-b border-outline-variant"
            >
              <td colspan="4" class="px-space-md py-space-sm">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="material-symbols-outlined text-[16px] text-[#1D4ED8]">
                    call_split
                  </span>
                  <span class="font-label-header text-label-header text-[#1D4ED8]">
                    {{ g.branch.parallel_label ?? '平行分支' }}
                  </span>
                  <span
                    :data-testid="`sim-branch-progress-${g.branch.parallel_id}`"
                    class="font-body-dense text-body-dense text-[#1D4ED8]"
                  >
                    {{ g.branch.branch_total }} 條分支中已完成
                    {{ g.branch.done }} 條
                  </span>
                  <span class="font-label-caption text-label-caption text-secondary">
                    · {{ joinRule(g.branch) }}
                  </span>
                </div>
              </td>
            </tr>

            <tr
              v-for="t in g.tasks"
              :key="t.task_id"
              :data-testid="`sim-task-${t.task_id}`"
              class="border-b border-outline-variant last:border-b-0"
              :class="g.branch !== null ? 'bg-[#F8FAFF]' : ''"
            >
            <td class="px-space-md py-space-sm font-data-mono text-[12px] text-secondary">
              {{ t.business_key }}
            </td>
            <td class="px-space-md py-space-sm font-body-dense text-body-dense text-on-surface">
              {{ t.node_label ?? t.node_id }}
            </td>
            <td class="px-space-md py-space-sm font-body-dense text-body-dense">
              <!--
                解析不到人時明說，不可假裝有人——
                頂替會讓使用者以為設定是對的
              -->
              <span v-if="t.assignee_name === null" class="text-error">
                無法解析簽核人員
                <span v-if="t.assignee_role" class="text-outline">
                  （角色 {{ t.assignee_role }} 沒有指派任何人）
                </span>
              </span>
              <span v-else class="text-on-surface">
                {{ t.assignee_name }}
                <!-- 解析依據：讓使用者看得出「為什麼是這個人」 -->
                <span v-if="t.assignee_role" class="text-outline font-data-mono text-[12px]">
                  （{{ t.assignee_role }}）
                </span>
              </span>
            </td>
            <td class="px-space-md py-space-sm text-right whitespace-nowrap">
              <!--
                時間快轉。只在節點真的有設逾時時出現——
                沒有等待可以快轉的話，按鈕什麼都不會做，
                比不給更讓人困惑。
              -->
              <button
                v-if="t.timeout != null"
                type="button"
                :data-testid="`skip-time-${t.task_id}`"
                :disabled="skipping === t.task_id"
                class="h-8 px-3 mr-2 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors disabled:opacity-50"
                :title="`快轉 ${humanDuration(t.timeout.after)} 後套用：${POLICY_LABEL[t.timeout.policy] ?? t.timeout.policy}`"
                @click="skip(t)"
              >
                <span class="material-symbols-outlined text-[16px] align-middle">
                  fast_forward
                </span>
                快轉 {{ humanDuration(t.timeout.after) }}
                <span class="text-outline">
                  （{{ POLICY_LABEL[t.timeout.policy] ?? t.timeout.policy }}）
                </span>
              </button>
              <button
                v-if="t.assignee_user_id !== null"
                type="button"
                :data-testid="`act-as-${t.task_id}`"
                class="h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 transition-opacity"
                @click="startActing(t)"
              >
                以 {{ t.assignee_name }} 簽核
              </button>
              </td>
            </tr>
          </template>
        </tbody>
      </table>
    </div>

    <!-- ── 扮演面板 ──────────────────────────────── -->
    <div
      v-if="acting !== null"
      class="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-8"
      data-testid="sim-panel"
      @click.self="acting = null"
    >
      <div class="bg-surface-container-lowest rounded-xl shadow-2xl w-[560px] max-w-full">
        <div class="px-space-lg py-space-md border-b border-outline-variant">
          <h2 class="font-title-sm text-title-sm text-on-surface">
            以 {{ acting.assignee_name }} 的身分簽核
          </h2>
          <p class="font-label-caption text-label-caption text-secondary mt-1">
            {{ acting.business_key }} · {{ acting.node_label ?? acting.node_id }}
          </p>
        </div>

        <div class="px-space-lg py-space-md flex flex-col gap-space-md">
          <!-- 稽核會同時記「誰按的」與「扮演誰」，先講清楚 -->
          <p class="px-3 py-2 rounded-lg bg-surface-container-low font-label-caption text-label-caption text-secondary">
            這筆決策會記錄成「由你操作、扮演 {{ acting.assignee_name }}」，
            兩個身分都會留在稽核紀錄裡。
          </p>

          <!--
            這個人會看到的畫面。
            欄位權限用**被扮演者**的角色重算，不是測試者的——
            兩者不同的話，模擬就只是走過場。
          -->
          <div data-testid="sim-form-view" class="flex flex-col gap-1">
            <div class="flex items-center justify-between">
              <span class="font-label-header text-label-header text-secondary">
                {{ acting.assignee_name }} 會看到的欄位
              </span>
              <span
                v-if="form !== null"
                data-testid="sim-form-roles"
                class="font-data-mono text-[12px] text-outline"
              >
                {{ form.acting_as_roles.join('、') || '無角色' }}
              </span>
            </div>

            <p
              v-if="formLoading"
              data-testid="sim-form-loading"
              class="px-3 py-2 font-body-dense text-body-dense text-secondary"
            >
              載入欄位權限中…
            </p>

            <!--
              載不到就明說。靜默顯示全部欄位會讓使用者以為
              「這個人什麼都看得到」，比不顯示更糟。
            -->
            <p
              v-else-if="formError"
              role="alert"
              data-testid="sim-form-error"
              class="px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
            >
              {{ formError }}
            </p>

            <div
              v-else-if="form !== null"
              class="border border-outline-variant rounded-lg overflow-hidden"
            >
              <table class="w-full">
                <tbody>
                  <tr
                    v-for="f in visibleFields"
                    :key="f.key"
                    :data-testid="`sim-field-${f.key}`"
                    class="border-b border-outline-variant last:border-b-0"
                  >
                    <td class="px-3 py-1.5 font-data-mono text-[12px] text-on-surface">
                      {{ f.key }}
                      <span v-if="f.required" class="text-error">*</span>
                    </td>
                    <td class="px-3 py-1.5 text-right whitespace-nowrap">
                      <span
                        :data-testid="`sim-perm-${f.key}`"
                        class="px-2 py-0.5 rounded font-label-caption text-label-caption"
                        :class="
                          f.permission === 'EDITABLE'
                            ? 'bg-[#DCFCE7] text-[#166534]'
                            : 'bg-surface-container-low text-secondary'
                        "
                      >
                        {{ PERMISSION_LABEL[f.permission] }}
                      </span>
                    </td>
                  </tr>
                </tbody>
              </table>

              <!--
                看不到的欄位要說出來，而且要說為什麼——
                「為什麼這個人看不到成本」正是設定對不對的關鍵。
              -->
              <div
                v-if="hiddenCount > 0"
                data-testid="sim-hidden-fields"
                class="px-3 py-2 bg-surface-container-low border-t border-outline-variant"
              >
                <p class="font-label-caption text-label-caption text-secondary">
                  另有 {{ hiddenCount }} 個欄位這個人看不到：
                </p>
                <p
                  v-for="f in hiddenFields"
                  :key="f.key"
                  class="font-label-caption text-label-caption text-outline"
                >
                  <span class="font-data-mono">{{ f.key }}</span>
                  —— {{ f.reason }}
                </p>
              </div>
            </div>
          </div>

          <label class="flex flex-col gap-1">
            <span class="font-label-header text-label-header text-secondary">意見</span>
            <textarea
              v-model="comment"
              rows="3"
              placeholder="選填"
              data-testid="sim-comment"
              class="px-3 py-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
            ></textarea>
          </label>

          <p
            v-if="simError"
            role="alert"
            data-testid="sim-error"
            class="px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
          >
            {{ simError }}
          </p>
        </div>

        <div class="px-space-lg py-space-md border-t border-outline-variant flex justify-end gap-space-sm">
          <button
            type="button"
            data-testid="sim-cancel"
            class="h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
            @click="acting = null"
          >
            取消
          </button>
          <button
            type="button"
            data-testid="sim-reject"
            :disabled="submitting"
            class="h-8 px-3 rounded-lg border border-error text-error hover:bg-error-container font-label-header text-label-header transition-colors disabled:opacity-50"
            @click="decide('REJECT')"
          >
            退回
          </button>
          <button
            type="button"
            data-testid="sim-approve"
            :disabled="submitting"
            class="h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 transition-opacity disabled:opacity-50"
            @click="decide('APPROVE')"
          >
            {{ submitting ? '送出中…' : '核准' }}
          </button>
        </div>
      </div>
    </div>
  </AppShell>
</template>
