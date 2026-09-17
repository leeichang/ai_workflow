<script setup lang="ts">
/**
 * 首頁
 *
 * 顯示待辦數與報價單狀態的概況，兩者都從既有 API 取得。
 * 不做假的統計圖表——沒有對應後端的數字只會誤導人。
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import * as tasksApi from '@/api/tasks'
import * as instancesApi from '@/api/instances'
import { useSession } from '@/auth/useSession'

const router = useRouter()
const session = useSession()

const pendingCount = ref(0)
const runningCount = ref(0)
const completedCount = ref(0)
const loading = ref(true)

const greeting = computed(() => {
  const name = session.user.value?.name ?? ''
  const hour = new Date().getHours()
  const period = hour < 12 ? '早安' : hour < 18 ? '午安' : '晚安'
  return `${period}，${name}`
})

/**
 * 載入概況
 *
 * 個別失敗不影響其他卡片——待辦查不到不代表報價單也查不到。
 * 統一 try/catch 會讓一個錯誤清空整個畫面。
 */
async function load(): Promise<void> {
  loading.value = true

  const [pending, instances] = await Promise.allSettled([
    tasksApi.list(),
    instancesApi.list({ business_object: 'quotation' }),
  ])

  if (pending.status === 'fulfilled') {
    pendingCount.value = pending.value.length
  }
  if (instances.status === 'fulfilled') {
    runningCount.value = instances.value.filter((i) => i.status === 'RUNNING').length
    completedCount.value = instances.value.filter(
      (i) => i.status === 'COMPLETED',
    ).length
  }

  loading.value = false
}

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-on-surface font-semibold">工作台</span>
    </template>

    <div class="max-w-[1000px]">
      <h1 class="font-title-md text-title-md text-on-surface" data-testid="home-greeting">
        {{ greeting }}
      </h1>
      <p class="font-body-dense text-body-dense text-secondary mt-1 mb-space-xl">
        以下是目前的工作概況
      </p>

      <div class="grid grid-cols-1 sm:grid-cols-3 gap-space-md">
        <button
          type="button"
          data-testid="home-card-pending"
          class="text-left bg-surface-container-lowest border border-outline-variant rounded-xl p-space-lg hover:border-primary-container transition-colors"
          @click="router.push('/tasks')"
        >
          <div class="flex items-center gap-space-sm text-secondary">
            <span class="material-symbols-outlined text-[18px]">check_box</span>
            <span class="font-label-header text-label-header">待我簽核</span>
          </div>
          <div class="font-title-md text-[28px] text-on-surface mt-space-sm">
            {{ loading ? '—' : pendingCount }}
          </div>
        </button>

        <button
          type="button"
          data-testid="home-card-running"
          class="text-left bg-surface-container-lowest border border-outline-variant rounded-xl p-space-lg hover:border-primary-container transition-colors"
          @click="router.push('/quotations')"
        >
          <div class="flex items-center gap-space-sm text-secondary">
            <span class="material-symbols-outlined text-[18px]">pending_actions</span>
            <span class="font-label-header text-label-header">簽核中報價單</span>
          </div>
          <div class="font-title-md text-[28px] text-on-surface mt-space-sm">
            {{ loading ? '—' : runningCount }}
          </div>
        </button>

        <button
          type="button"
          data-testid="home-card-completed"
          class="text-left bg-surface-container-lowest border border-outline-variant rounded-xl p-space-lg hover:border-primary-container transition-colors"
          @click="router.push('/quotations')"
        >
          <div class="flex items-center gap-space-sm text-secondary">
            <span class="material-symbols-outlined text-[18px]">task_alt</span>
            <span class="font-label-header text-label-header">已完成</span>
          </div>
          <div class="font-title-md text-[28px] text-on-surface mt-space-sm">
            {{ loading ? '—' : completedCount }}
          </div>
        </button>
      </div>
    </div>
  </AppShell>
</template>
