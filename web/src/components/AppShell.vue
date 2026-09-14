<script setup lang="ts">
/**
 * 應用外框
 *
 * 依設計稿 docs/UI 設計/.../_7 實作：
 *   左側 240px 側邊欄、頂部 56px 列、主內容區。
 */
import { computed } from 'vue'
import { useRoute } from 'vue-router'

interface NavItem {
  path: string
  label: string
  icon: string
  badge?: number
}

const route = useRoute()

const navItems: NavItem[] = [
  { path: '/', label: '首頁', icon: 'dashboard' },
  { path: '/tasks', label: '我的待辦', icon: 'check_box', badge: 5 },
  { path: '/quotations', label: '報價單', icon: 'description' },
  { path: '/purchase-requests', label: '採購申請', icon: 'shopping_cart' },
  { path: '/designer', label: '設計器', icon: 'layers' },
  { path: '/reports', label: '報表', icon: 'bar_chart' },
  { path: '/settings', label: '設定', icon: 'settings' },
]

const isActive = (path: string) =>
  path === '/' ? route.path === '/' : route.path.startsWith(path)

const today = computed(() => {
  const d = new Date()
  const weekdays = ['日', '一', '二', '三', '四', '五', '六']
  return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日 (週${weekdays[d.getDay()]})`
})
</script>

<template>
  <aside
    class="fixed left-0 top-0 h-full w-[240px] bg-surface-container-lowest border-r border-outline-variant z-50 flex flex-col justify-between select-none"
    data-testid="app-sidebar"
  >
    <div class="flex flex-col">
      <div class="h-[56px] px-space-md border-b border-outline-variant flex items-center gap-space-sm">
        <div class="w-8 h-8 rounded-lg bg-primary-container flex items-center justify-center text-on-primary font-title-md shrink-0">
          M
        </div>
        <div class="flex flex-col min-w-0">
          <span class="font-title-md text-title-md text-on-surface truncate leading-none">
            台製精工 ERP
          </span>
          <span class="font-label-caption text-label-caption text-secondary truncate mt-0.5">
            製造營運管理系統
          </span>
        </div>
      </div>

      <div class="px-space-md pt-space-md pb-space-xs">
        <span class="font-label-header text-label-header text-secondary uppercase tracking-wider">
          營運功能模組
        </span>
      </div>

      <nav class="px-space-sm space-y-0.5 flex flex-col" data-testid="app-nav">
        <RouterLink
          v-for="item in navItems"
          :key="item.path"
          :to="item.path"
          :data-testid="`nav-${item.path.replace(/\//g, '') || 'home'}`"
          class="flex items-center justify-between px-space-md py-space-sm rounded-lg transition-colors"
          :class="
            isActive(item.path)
              ? 'bg-surface-container-low text-primary font-semibold'
              : 'text-on-surface-variant hover:bg-surface-container-low hover:text-on-surface'
          "
        >
          <div class="flex items-center gap-space-sm">
            <span class="material-symbols-outlined text-[18px]">{{ item.icon }}</span>
            <span class="font-body-dense text-body-dense">{{ item.label }}</span>
          </div>
          <span
            v-if="item.badge"
            class="inline-flex items-center justify-center h-4 min-w-4 px-1 rounded-full bg-error text-on-error font-label-caption text-[10px] font-bold"
          >
            {{ item.badge }}
          </span>
        </RouterLink>
      </nav>
    </div>

    <div class="p-space-md border-t border-outline-variant flex flex-col gap-space-xs">
      <div class="flex items-center justify-between">
        <span class="font-data-mono text-[11px] text-secondary">v0.1.0 (開發版)</span>
        <span class="inline-block w-2 h-2 rounded-full bg-[#15803D]" />
      </div>
    </div>
  </aside>

  <div class="pl-[240px]">
    <header
      class="fixed top-0 left-[240px] right-0 h-[56px] bg-surface-container-lowest border-b border-outline-variant z-40 flex items-center justify-between px-space-lg"
      data-testid="app-header"
    >
      <nav class="flex items-center gap-1 font-label-header text-label-header text-secondary">
        <slot name="breadcrumb">
          <span class="text-on-surface font-semibold">工作台</span>
        </slot>
      </nav>

      <div class="flex-1 max-w-md mx-space-lg">
        <div class="relative flex items-center">
          <span class="material-symbols-outlined absolute left-2.5 text-[18px] text-outline">
            search
          </span>
          <input
            type="text"
            placeholder="搜尋單號、客戶、物料"
            data-testid="global-search"
            class="w-full h-8 pl-8 pr-3 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense text-on-surface placeholder:text-outline focus:outline-none focus:border-primary-container focus:bg-surface-container-lowest transition-all"
          />
        </div>
      </div>

      <div class="flex items-center gap-space-md">
        <span class="hidden md:flex items-center gap-1 text-secondary font-data-mono text-[12px]">
          <span class="material-symbols-outlined text-[16px]">calendar_today</span>
          {{ today }}
        </span>
        <div class="h-4 w-[1px] bg-outline-variant hidden md:block" />
        <div class="flex items-center gap-space-sm" data-testid="user-menu">
          <div class="w-8 h-8 rounded-full bg-secondary-container flex items-center justify-center font-label-header text-on-secondary-fixed">
            <slot name="user-initial">林</slot>
          </div>
          <div class="hidden lg:flex flex-col text-left">
            <span class="font-label-header text-label-header text-on-surface leading-tight">
              <slot name="user-name">林建志 協理</slot>
            </span>
            <span class="font-label-caption text-label-caption text-secondary leading-tight">
              <slot name="user-dept">營運製造部</slot>
            </span>
          </div>
        </div>
      </div>
    </header>

    <main class="w-full pt-[56px] min-h-screen bg-background p-space-xl">
      <slot />
    </main>
  </div>
</template>
