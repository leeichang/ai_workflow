<script setup lang="ts">
/**
 * 未實作頁面
 *
 * 選單上有、但功能還沒做的項目共用這一頁。
 *
 * 做這個而不是把選單項目藏起來：路線圖對使用者是有價值的資訊，
 * 知道「這個之後會有」跟「這個不存在」是兩回事。
 *
 * 也不做假畫面——放一個填著假資料的報表頁，使用者會以為數字是真的。
 */
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import AppShell from '@/components/AppShell.vue'

interface Info {
  title: string
  reason: string
}

/**
 * 各頁的說明
 *
 * reason 寫「缺什麼」而不是「開發中」——後者等於沒說。
 */
const PAGES: Record<string, Info> = {
  '/purchase-requests': {
    title: '採購申請',
    reason:
      '採購申請的表單與流程定義尚未建立。第一階段以報價單流程為主，採購申請沿用同一套設計器，預計下一階段完成。',
  },
  '/reports': {
    title: '報表',
    reason:
      '報表設計器在規劃文件中列為 P2，排在單據套版設計器之後。目前流程資料可從報價單頁查看。',
  },
  '/settings': {
    title: '設定',
    reason:
      '使用者、角色與部門的管理介面尚未實作。目前這些資料由 server/seed.sh 建立，需要調整時直接改資料庫。',
  },
}

const route = useRoute()
const info = computed<Info>(
  () =>
    PAGES[route.path] ?? {
      title: '尚未實作',
      reason: '這個功能還在規劃中。',
    },
)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-on-surface font-semibold">{{ info.title }}</span>
    </template>

    <div class="max-w-[600px]">
      <h1 class="font-title-md text-title-md text-on-surface">{{ info.title }}</h1>

      <div
        class="mt-space-lg bg-surface-container-lowest border border-outline-variant rounded-xl p-space-xl flex flex-col items-center text-center gap-space-md"
        data-testid="not-built"
      >
        <span class="material-symbols-outlined text-[40px] text-outline">
          construction
        </span>
        <div>
          <p class="font-title-md text-[15px] text-on-surface">此功能尚未實作</p>
          <p class="font-body-dense text-body-dense text-secondary mt-space-sm leading-relaxed">
            {{ info.reason }}
          </p>
        </div>
      </div>
    </div>
  </AppShell>
</template>
