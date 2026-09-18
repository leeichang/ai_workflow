<script setup lang="ts">
/**
 * 應用外框
 *
 * 依設計稿 docs/UI 設計/.../_7 實作：
 *   左側 240px 側邊欄、頂部 56px 列、主內容區。
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useSession } from '@/auth/useSession'
import * as tasksApi from '@/api/tasks'

interface NavItem {
  path: string
  label: string
  icon: string
  badge?: number
}

interface SubNavItem {
  key: string
  path: string
  label: string
  icon: string
}

const route = useRoute()
const router = useRouter()
const session = useSession()

/**
 * 使用者選單
 *
 * 展開狀態不存 localStorage——選單開著跨頁面保留沒有意義。
 */
const userMenuOpen = ref(false)

/** 頭像用姓名第一個字。中文姓名取姓，英文取首字母。 */
const userInitial = computed(() => session.user.value?.name.charAt(0) ?? '?')

const userName = computed(() => session.user.value?.name ?? '未登入')

/**
 * 角色顯示
 *
 * 一個人可能有多個角色（例如 cfo + approver），全列出來太長，
 * 顯示第一個即可——需要完整清單時看設定頁。
 */
const userRole = computed(() => session.user.value?.roles[0] ?? '')

const userEmail = computed(() => session.user.value?.email ?? '')

function logout(): void {
  userMenuOpen.value = false
  session.logout()
  void router.push('/login')
}

const SIDEBAR_WIDTH = 240
const SIDEBAR_COLLAPSED_WIDTH = 56
const STORAGE_KEY = 'app.sidebar.collapsed'

/**
 * 側邊欄收合
 *
 * 設計器類畫面需要盡量寬的畫布，240px 的側邊欄佔掉可觀的空間。
 * 收合後只留圖示，主內容區跟著變寬——因此主內容用 margin 而非
 * 固定 padding，否則側邊欄縮了畫布也不會變大。
 *
 * 狀態存 localStorage：使用者在設計器收起側邊欄後切到別的頁面
 * 又跳回來，預期它還是收著的。
 */
const collapsed = ref(readCollapsed())

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) === '1'
  } catch {
    // 隱私模式下 localStorage 可能拋錯。收合狀態不值得讓畫面掛掉。
    return false
  }
}

watch(collapsed, (value) => {
  try {
    localStorage.setItem(STORAGE_KEY, value ? '1' : '0')
  } catch {
    /* 同上，忽略 */
  }
})

const sidebarWidth = computed(() =>
  collapsed.value ? SIDEBAR_COLLAPSED_WIDTH : SIDEBAR_WIDTH,
)

/**
 * 待辦數徽章
 *
 * 設計稿寫死 5，改為實際查詢。查詢失敗時不顯示徽章——
 * 顯示 0 會讓使用者以為真的沒有待辦，而實際上是沒查到。
 */
const pendingCount = ref<number | null>(null)

onMounted(async () => {
  if (!session.isAuthenticated.value) return
  try {
    pendingCount.value = (await tasksApi.list()).length
  } catch {
    pendingCount.value = null
  }
})

/**
 * 設計器子選單
 *
 * 四個設計器都已實作，但先前「設計器」只是 redirect 到表單設計器，
 * 另外三個沒有入口——使用者無從得知它們存在。
 *
 * 表單與流程都先進清單頁再選——固定指向報價單會讓使用者
 * 以為系統只能編輯那一張，無從建立自己的定義。
 */
const DESIGNER_SUB_ITEMS: SubNavItem[] = [
  {
    key: 'forms',
    path: '/designer/forms',
    label: '表單設計器',
    icon: 'dashboard_customize',
  },
  {
    key: 'workflows',
    path: '/designer/workflows',
    label: '流程設計器',
    icon: 'account_tree',
  },
  {
    key: 'templates',
    path: '/designer/templates',
    label: '單據套版設計器',
    icon: 'picture_as_pdf',
  },
  {
    key: 'permissions',
    path: '/designer/permissions',
    label: '權限矩陣',
    icon: 'grid_on',
  },
]

const designerSubItems = DESIGNER_SUB_ITEMS

const inDesigner = computed(() => route.path.startsWith('/designer'))

/**
 * 子選單展開狀態
 *
 * 進到設計器頁面時預設展開——使用者從網址直接進來或重新整理，
 * 子選單收著的話看不出自己在哪一層。
 */
const designerOpen = ref(inDesigner.value)

// 導覽到設計器時自動展開，但離開後不強制收起：
// 使用者可能正要切到另一個設計器
watch(inDesigner, (nowInside) => {
  if (nowInside) designerOpen.value = true
})

/** 設計器之前的項目 */
const navItemsBefore = computed<NavItem[]>(() => [
  { path: '/', label: '首頁', icon: 'dashboard' },
  {
    path: '/tasks',
    label: '我的待辦',
    icon: 'check_box',
    badge: pendingCount.value ?? undefined,
  },
  { path: '/quotations', label: '報價單', icon: 'description' },
  { path: '/purchase-requests', label: '採購申請', icon: 'shopping_cart' },
])

/** 設計器之後的項目 */
const navItemsAfter: NavItem[] = [
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
    class="fixed left-0 top-0 h-full bg-surface-container-lowest border-r border-outline-variant z-50 flex flex-col justify-between select-none transition-[width] duration-200"
    :style="{ width: `${sidebarWidth}px` }"
    :data-collapsed="collapsed ? 'true' : 'false'"
    data-testid="app-sidebar"
  >
    <div class="flex flex-col min-w-0">
      <div
        class="h-[56px] border-b border-outline-variant flex items-center gap-space-sm"
        :class="collapsed ? 'justify-center px-0' : 'px-space-md'"
      >
        <div class="w-8 h-8 rounded-lg bg-primary-container flex items-center justify-center text-on-primary font-title-md shrink-0">
          M
        </div>
        <div v-if="!collapsed" class="flex flex-col min-w-0">
          <span class="font-title-md text-title-md text-on-surface truncate leading-none">
            台製精工 ERP
          </span>
          <span class="font-label-caption text-label-caption text-secondary truncate mt-0.5">
            製造營運管理系統
          </span>
        </div>
      </div>

      <div v-if="!collapsed" class="px-space-md pt-space-md pb-space-xs">
        <span class="font-label-header text-label-header text-secondary uppercase tracking-wider">
          營運功能模組
        </span>
      </div>

      <nav
        class="space-y-0.5 flex flex-col"
        :class="collapsed ? 'px-1.5 pt-space-md' : 'px-space-sm'"
        data-testid="app-nav"
      >
        <RouterLink
          v-for="item in navItemsBefore"
          :key="item.path"
          :to="item.path"
          :title="collapsed ? item.label : undefined"
          :data-testid="`nav-${item.path.replace(/\//g, '') || 'home'}`"
          class="relative flex items-center rounded-lg transition-colors"
          :class="[
            collapsed
              ? 'justify-center py-space-sm'
              : 'justify-between px-space-md py-space-sm',
            isActive(item.path)
              ? 'bg-surface-container-low text-primary font-semibold'
              : 'text-on-surface-variant hover:bg-surface-container-low hover:text-on-surface',
          ]"
        >
          <div class="flex items-center gap-space-sm">
            <span class="material-symbols-outlined text-[18px]">{{ item.icon }}</span>
            <span v-if="!collapsed" class="font-body-dense text-body-dense">
              {{ item.label }}
            </span>
          </div>

          <!-- 收合時徽章疊在圖示右上角，否則沒有空間顯示 -->
          <span
            v-if="item.badge"
            class="inline-flex items-center justify-center h-4 min-w-4 px-1 rounded-full bg-error text-on-error font-label-caption text-[10px] font-bold"
            :class="collapsed ? 'absolute top-1 right-1' : ''"
          >
            {{ item.badge }}
          </span>
        </RouterLink>

        <!--
          設計器：四個子項目。側邊欄收合時退化成單一連結指向表單清單，
          因為 56px 寬放不下子選單。
        -->
        <RouterLink
          v-if="collapsed"
          to="/designer/forms"
          title="設計器"
          data-testid="nav-designer"
          class="relative flex items-center justify-center py-space-sm rounded-lg transition-colors"
          :class="
            inDesigner
              ? 'bg-surface-container-low text-primary font-semibold'
              : 'text-on-surface-variant hover:bg-surface-container-low hover:text-on-surface'
          "
        >
          <span class="material-symbols-outlined text-[18px]">layers</span>
        </RouterLink>

        <template v-else>
          <button
            type="button"
            data-testid="nav-designer-toggle"
            :aria-expanded="designerOpen"
            class="w-full flex items-center justify-between px-space-md py-space-sm rounded-lg transition-colors"
            :class="
              inDesigner
                ? 'bg-surface-container-low text-primary font-semibold'
                : 'text-on-surface-variant hover:bg-surface-container-low hover:text-on-surface'
            "
            @click="designerOpen = !designerOpen"
          >
            <div class="flex items-center gap-space-sm">
              <span class="material-symbols-outlined text-[18px]">layers</span>
              <span class="font-body-dense text-body-dense">設計器</span>
            </div>
            <span class="material-symbols-outlined text-[16px]">
              {{ designerOpen ? 'expand_less' : 'expand_more' }}
            </span>
          </button>

          <div v-if="designerOpen" class="flex flex-col space-y-0.5 mt-0.5">
            <RouterLink
              v-for="sub in designerSubItems"
              :key="sub.key"
              :to="sub.path"
              :data-testid="`nav-designer-${sub.key}`"
              class="flex items-center gap-space-sm pl-[34px] pr-space-md py-1.5 rounded-lg transition-colors"
              :class="
                isActive(sub.path)
                  ? 'bg-surface-container-low text-primary font-semibold'
                  : 'text-on-surface-variant hover:bg-surface-container-low hover:text-on-surface'
              "
            >
              <span class="material-symbols-outlined text-[16px]">{{ sub.icon }}</span>
              <span class="font-body-dense text-body-dense">{{ sub.label }}</span>
            </RouterLink>
          </div>
        </template>

        <RouterLink
          v-for="item in navItemsAfter"
          :key="item.path"
          :to="item.path"
          :title="collapsed ? item.label : undefined"
          :data-testid="`nav-${item.path.replace(/\//g, '')}`"
          class="relative flex items-center rounded-lg transition-colors"
          :class="[
            collapsed
              ? 'justify-center py-space-sm'
              : 'justify-between px-space-md py-space-sm',
            isActive(item.path)
              ? 'bg-surface-container-low text-primary font-semibold'
              : 'text-on-surface-variant hover:bg-surface-container-low hover:text-on-surface',
          ]"
        >
          <div class="flex items-center gap-space-sm">
            <span class="material-symbols-outlined text-[18px]">{{ item.icon }}</span>
            <span v-if="!collapsed" class="font-body-dense text-body-dense">
              {{ item.label }}
            </span>
          </div>
        </RouterLink>
      </nav>
    </div>

    <div class="border-t border-outline-variant flex flex-col gap-space-xs p-space-sm">
      <button
        type="button"
        :title="collapsed ? '展開選單' : '收合選單'"
        :aria-expanded="!collapsed"
        data-testid="sidebar-toggle"
        class="flex items-center gap-space-sm px-space-sm py-space-sm rounded-lg text-secondary hover:bg-surface-container-low hover:text-on-surface transition-colors"
        :class="collapsed ? 'justify-center' : ''"
        @click="collapsed = !collapsed"
      >
        <span class="material-symbols-outlined text-[18px]">
          {{ collapsed ? 'chevron_right' : 'chevron_left' }}
        </span>
        <span v-if="!collapsed" class="font-body-dense text-body-dense">收合選單</span>
      </button>

      <div v-if="!collapsed" class="flex items-center justify-between px-space-sm">
        <span class="font-data-mono text-[11px] text-secondary">v0.1.0 (開發版)</span>
        <span class="inline-block w-2 h-2 rounded-full bg-[#15803D]" />
      </div>
    </div>
  </aside>

  <!--
    主內容用 padding-left 跟著側邊欄寬度變動，而非固定 240px。
    固定值會讓側邊欄收合後主內容不變寬，收合就失去意義。
  -->
  <div
    class="transition-[padding] duration-200"
    :style="{ paddingLeft: `${sidebarWidth}px` }"
  >
    <header
      class="fixed top-0 right-0 h-[56px] bg-surface-container-lowest border-b border-outline-variant z-40 flex items-center justify-between px-space-lg transition-[left] duration-200"
      :style="{ left: `${sidebarWidth}px` }"
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
        <div class="relative" data-testid="user-menu">
          <button
            type="button"
            class="flex items-center gap-space-sm rounded-lg px-1 py-1 hover:bg-surface-container-low transition-colors"
            :aria-expanded="userMenuOpen"
            data-testid="user-menu-toggle"
            @click="userMenuOpen = !userMenuOpen"
          >
            <div class="w-8 h-8 rounded-full bg-secondary-container flex items-center justify-center font-label-header text-on-secondary-fixed">
              {{ userInitial }}
            </div>
            <div class="hidden lg:flex flex-col text-left">
              <span
                class="font-label-header text-label-header text-on-surface leading-tight"
                data-testid="user-name"
              >
                {{ userName }}
              </span>
              <span class="font-label-caption text-label-caption text-secondary leading-tight">
                {{ userRole }}
              </span>
            </div>
            <span class="material-symbols-outlined text-[18px] text-secondary">
              {{ userMenuOpen ? 'expand_less' : 'expand_more' }}
            </span>
          </button>

          <!--
            點選單以外的地方要能關閉。用一層透明遮罩而非 document 監聽：
            不必在元件卸載時記得移除監聽器。
          -->
          <div
            v-if="userMenuOpen"
            class="fixed inset-0 z-40"
            @click="userMenuOpen = false"
          />

          <div
            v-if="userMenuOpen"
            class="absolute right-0 top-[calc(100%+4px)] z-50 min-w-[200px] bg-surface-container-lowest border border-outline-variant rounded-lg shadow-lg py-1"
            data-testid="user-menu-panel"
          >
            <div class="px-space-md py-space-sm border-b border-outline-variant">
              <div class="font-label-header text-label-header text-on-surface">
                {{ userName }}
              </div>
              <div class="font-label-caption text-label-caption text-secondary mt-0.5">
                {{ userEmail }}
              </div>
            </div>
            <button
              type="button"
              class="w-full flex items-center gap-space-sm px-space-md py-space-sm text-left font-body-dense text-body-dense text-on-surface-variant hover:bg-surface-container-low hover:text-on-surface transition-colors"
              data-testid="logout-button"
              @click="logout"
            >
              <span class="material-symbols-outlined text-[18px]">logout</span>
              登出
            </button>
          </div>
        </div>
      </div>
    </header>

    <main class="w-full pt-[56px] min-h-screen bg-background p-space-xl">
      <slot />
    </main>
  </div>
</template>
