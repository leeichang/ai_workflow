<script setup lang="ts">
/**
 * 報價單列表
 *
 * 資料來源是流程實例（business_object = 'quotation'），
 * 而非獨立的報價單資料表——報價單的主檔在客戶的 ERP 裡，
 * 這套系統管的是「這張報價單的簽核走到哪」。
 *
 * 建立走 QuotationCreate.vue。
 */
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import * as instancesApi from '@/api/instances'
import type { WorkflowInstance } from '@/api/instances'
import { ApiError } from '@/api/types'

const router = useRouter()

const instances = ref<WorkflowInstance[]>([])
const loading = ref(true)
const errorMessage = ref('')

/** 狀態的中文與配色。未知狀態原樣顯示，不要硬塞一個預設值。 */
const STATUS_LABELS: Record<string, { text: string; class: string }> = {
  RUNNING: { text: '簽核中', class: 'bg-primary-container text-on-primary' },
  COMPLETED: { text: '已完成', class: 'bg-[#DCFCE7] text-[#15803D]' },
  REJECTED: { text: '已退回', class: 'bg-error-container text-on-error-container' },
  CANCELLED: { text: '已取消', class: 'bg-surface-container-low text-secondary' },
  FAILED: { text: '執行失敗', class: 'bg-error-container text-on-error-container' },
}

function statusOf(status: string) {
  return (
    STATUS_LABELS[status] ?? {
      text: status,
      class: 'bg-surface-container-low text-secondary',
    }
  )
}

async function load(): Promise<void> {
  loading.value = true
  errorMessage.value = ''
  try {
    instances.value = await instancesApi.list({ business_object: 'quotation' })
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法載入報價單，請稍後再試'
  } finally {
    loading.value = false
  }
}

function formatDate(iso: string): string {
  const d = new Date(iso)
  return `${d.getFullYear()}/${String(d.getMonth() + 1).padStart(2, '0')}/${String(d.getDate()).padStart(2, '0')}`
}

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-on-surface font-semibold">報價單</span>
    </template>

    <div class="max-w-[1000px]">
      <div class="flex items-center justify-between mb-space-lg">
        <div>
          <h1 class="font-title-md text-title-md text-on-surface">報價單</h1>
          <p class="font-body-dense text-body-dense text-secondary mt-1">
            報價單的簽核狀態
          </p>
        </div>
        <div class="flex items-center gap-space-sm">
          <button
            type="button"
            class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
            data-testid="quotation-refresh"
            @click="load"
          >
            <span class="material-symbols-outlined text-[16px]">refresh</span>
            重新整理
          </button>
          <button
            type="button"
            class="flex items-center gap-1 h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 transition-opacity"
            data-testid="quotation-create"
            @click="router.push('/quotations/new')"
          >
            <span class="material-symbols-outlined text-[16px]">add</span>
            建立報價單
          </button>
        </div>
      </div>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="quotation-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <div
        v-if="loading"
        data-testid="quotation-loading"
        class="py-space-xl text-center font-body-dense text-body-dense text-secondary"
      >
        載入中…
      </div>

      <div
        v-else-if="instances.length === 0"
        data-testid="quotation-empty"
        class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
      >
        <span class="material-symbols-outlined text-[40px] text-outline">description</span>
        <p class="font-body-dense text-body-dense">尚無報價單</p>
      </div>

      <table
        v-else
        class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
        data-testid="quotation-table"
      >
        <thead>
          <tr class="bg-surface-container-low border-b border-outline-variant">
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              單號
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              狀態
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              建立日期
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="item in instances"
            :key="item.id"
            :data-testid="`quotation-row-${item.business_key}`"
            class="border-b border-outline-variant last:border-b-0 hover:bg-surface-container-low transition-colors"
          >
            <td class="px-space-md py-space-sm font-data-mono text-[12px] text-on-surface">
              {{ item.business_key }}
            </td>
            <td class="px-space-md py-space-sm">
              <span
                class="inline-block px-2 py-0.5 rounded font-label-caption text-label-caption"
                :class="statusOf(item.status).class"
              >
                {{ statusOf(item.status).text }}
              </span>
            </td>
            <td class="px-space-md py-space-sm font-body-dense text-body-dense text-secondary">
              {{ formatDate(item.started_at) }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </AppShell>
</template>
