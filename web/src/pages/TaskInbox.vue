<script setup lang="ts">
/**
 * 我的待辦
 *
 * 簽核流程的操作入口。在此之前只能用 curl 打 API。
 *
 * 後端依 user_id 與角色過濾，前端不自行篩選——
 * 「我能看到哪些待辦」是授權問題，不是顯示問題。
 */
import { onMounted, ref } from 'vue'
import AppShell from '@/components/AppShell.vue'
import * as tasksApi from '@/api/tasks'
import type { HumanTask } from '@/api/tasks'
import { ApiError } from '@/api/types'

const tasks = ref<HumanTask[]>([])
const loading = ref(true)
const errorMessage = ref('')
/** 正在送出決策的待辦 id，用來停用該列按鈕 */
const submitting = ref<string | null>(null)
/** 各列的意見，key 是 task id */
const comments = ref<Record<string, string>>({})

/**
 * 載入待辦
 *
 * 不主動清除 errorMessage：409 之後會重新載入列表，
 * 若在這裡清掉，「待辦已被處理」的說明會跟著消失，
 * 使用者只看到自己按的那一筆憑空不見。清除的時機由呼叫端決定。
 */
async function load(): Promise<void> {
  loading.value = true
  try {
    tasks.value = await tasksApi.list()
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法載入待辦，請稍後再試'
  } finally {
    loading.value = false
  }
}

/** 重新整理：這是使用者主動觸發的，舊錯誤該清掉 */
async function refresh(): Promise<void> {
  errorMessage.value = ''
  await load()
}

/**
 * 送出決策
 *
 * 409 與 503 要分開處理：
 *   409 代表別人已經處理掉了，我們手上的列表過期，必須重抓。
 *   503 代表流程引擎斷線，決策根本沒生效，待辦要留著讓使用者重試。
 * 兩者都顯示「失敗」的話，使用者不知道該不該再按一次。
 */
async function decide(
  task: HumanTask,
  decision: 'APPROVE' | 'REJECT',
): Promise<void> {
  if (submitting.value !== null) return

  submitting.value = task.id
  errorMessage.value = ''

  try {
    await tasksApi.decide(task.id, {
      decision,
      comment: comments.value[task.id] ?? '',
    })
    delete comments.value[task.id]
    errorMessage.value = ''
    await load()
  } catch (error) {
    if (error instanceof ApiError) {
      errorMessage.value = error.message
      // 別人搶先處理了，列表已經不準，重抓一次
      if (error.status === 409) {
        await load()
      }
    } else {
      errorMessage.value = '送出決策失敗，請稍後再試'
    }
  } finally {
    submitting.value = null
  }
}

function formatDate(iso: string): string {
  const d = new Date(iso)
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-on-surface font-semibold">我的待辦</span>
    </template>

    <div class="max-w-[1000px]">
      <div class="flex items-center justify-between mb-space-lg">
        <div>
          <h1 class="font-title-md text-title-md text-on-surface">我的待辦</h1>
          <p class="font-body-dense text-body-dense text-secondary mt-1">
            等待您簽核的項目
          </p>
        </div>
        <button
          type="button"
          class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
          data-testid="inbox-refresh"
          @click="refresh"
        >
          <span class="material-symbols-outlined text-[16px]">refresh</span>
          重新整理
        </button>
      </div>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="inbox-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <div
        v-if="loading"
        data-testid="inbox-loading"
        class="py-space-xl text-center font-body-dense text-body-dense text-secondary"
      >
        載入中…
      </div>

      <div
        v-else-if="tasks.length === 0"
        data-testid="inbox-empty"
        class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
      >
        <span class="material-symbols-outlined text-[40px] text-outline">inbox</span>
        <p class="font-body-dense text-body-dense">目前沒有待辦事項</p>
      </div>

      <ul v-else class="flex flex-col gap-space-md">
        <li
          v-for="task in tasks"
          :key="task.id"
          :data-testid="`task-row-${task.id}`"
          class="bg-surface-container-lowest border border-outline-variant rounded-xl p-space-lg"
        >
          <div class="flex items-start justify-between gap-space-md">
            <div class="min-w-0">
              <div class="flex items-center gap-space-sm">
                <span class="font-title-md text-[15px] text-on-surface">
                  {{ task.node_label ?? task.node_id }}
                </span>
                <span
                  v-if="task.participant_kind === 'external'"
                  class="px-1.5 py-0.5 rounded bg-secondary-container text-on-secondary-fixed font-label-caption text-[10px]"
                >
                  外部
                </span>
              </div>
              <div
                class="font-data-mono text-[12px] text-secondary mt-1"
                :data-testid="`task-key-${task.id}`"
              >
                {{ task.business_key ?? '（無單號）' }}
              </div>
            </div>
            <span class="font-label-caption text-label-caption text-secondary shrink-0">
              {{ formatDate(task.created_at) }}
            </span>
          </div>

          <input
            v-model="comments[task.id]"
            type="text"
            placeholder="簽核意見（選填）"
            :data-testid="`comment-${task.id}`"
            class="w-full h-8 mt-space-md px-3 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense text-on-surface placeholder:text-outline focus:outline-none focus:border-primary-container transition-all"
          />

          <div class="flex items-center gap-space-sm mt-space-md">
            <button
              type="button"
              :disabled="submitting === task.id"
              :data-testid="`approve-${task.id}`"
              class="h-8 px-4 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed transition-opacity"
              @click="decide(task, 'APPROVE')"
            >
              核准
            </button>
            <button
              type="button"
              :disabled="submitting === task.id"
              :data-testid="`reject-${task.id}`"
              class="h-8 px-4 rounded-lg border border-outline-variant text-on-surface-variant font-label-header text-label-header hover:bg-surface-container-low disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              @click="decide(task, 'REJECT')"
            >
              退回
            </button>
          </div>
        </li>
      </ul>
    </div>
  </AppShell>
</template>
