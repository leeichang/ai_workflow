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
import type { PendingTask, SandboxSession } from '@/api/sandbox'
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

function startActing(task: PendingTask): void {
  acting.value = task
  comment.value = ''
  simError.value = ''
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
          <tr
            v-for="t in tasks"
            :key="t.task_id"
            :data-testid="`sim-task-${t.task_id}`"
            class="border-b border-outline-variant last:border-b-0"
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
            <td class="px-space-md py-space-sm text-right">
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
