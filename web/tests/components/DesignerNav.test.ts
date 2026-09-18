/**
 * 側邊欄設計器子選單測試
 *
 * 四個設計器都已實作，但選單只有一個「設計器」項目直接
 * redirect 到表單設計器，另外三個沒有入口——使用者看不到它們存在。
 */

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import AppShell from '@/components/AppShell.vue'
import { useSession } from '@/auth/useSession'

const push = vi.fn()
let currentPath = '/'

vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ path: currentPath }),
  RouterLink: {
    props: ['to'],
    template: '<a :href="to"><slot /></a>',
  },
}))

function loginAs(roles: string[] = ['designer']) {
  localStorage.setItem(
    'workflow.session',
    JSON.stringify({
      access_token: 'h.p.s',
      user: {
        id: 'u1',
        name: '陳雅婷',
        email: 'designer@demo.local',
        tenant_id: 't1',
        roles,
      },
    }),
  )
  useSession().restore()
}

beforeEach(() => {
  localStorage.clear()
  useSession().reset()
  push.mockClear()
  currentPath = '/'
})

describe('設計器子選單', () => {
  it('四個設計器都有入口', async () => {
    loginAs()
    const w = mount(AppShell)
    await w.find('[data-testid="nav-designer-toggle"]').trigger('click')

    expect(w.find('[data-testid="nav-designer-forms"]').exists()).toBe(true)
    expect(w.find('[data-testid="nav-designer-workflows"]').exists()).toBe(true)
    expect(w.find('[data-testid="nav-designer-templates"]').exists()).toBe(true)
    expect(w.find('[data-testid="nav-designer-permissions"]').exists()).toBe(true)
  })

  it('子項目連到正確路徑', async () => {
    loginAs()
    const w = mount(AppShell)
    await w.find('[data-testid="nav-designer-toggle"]').trigger('click')

    // RouterLink 在這裡沒有真的被 router 解析（測試 mock 掉了），
    // 渲染成 <routerlink to="...">，所以讀 to 屬性而非 href
    const to = (key: string) =>
      w.find(`[data-testid="nav-designer-${key}"]`).attributes('to')

    // 表單與流程都進清單頁而非直接開某一張——固定指向報價單
    // 會讓使用者以為系統只能編輯那一張，無從建立自己的定義
    expect(to('forms')).toBe('/designer/forms')
    expect(to('workflows')).toBe('/designer/workflows')
    expect(to('templates')).toBe('/designer/templates')
    expect(to('permissions')).toBe('/designer/permissions')
  })

  it('預設收合', () => {
    loginAs()
    const w = mount(AppShell)
    expect(w.find('[data-testid="nav-designer-forms"]').exists()).toBe(false)
  })

  it('再次點擊收合', async () => {
    loginAs()
    const w = mount(AppShell)
    const toggle = w.find('[data-testid="nav-designer-toggle"]')
    await toggle.trigger('click')
    await toggle.trigger('click')
    expect(w.find('[data-testid="nav-designer-forms"]').exists()).toBe(false)
  })

  it('目前在設計器頁面時自動展開', () => {
    // 使用者從網址直接進來或重新整理，子選單該是開的，
    // 否則看不出自己在哪一層
    currentPath = '/designer/workflows/quotation_approval'
    loginAs()
    const w = mount(AppShell)
    expect(w.find('[data-testid="nav-designer-workflows"]').exists()).toBe(true)
  })

  it('標示目前所在的設計器', async () => {
    currentPath = '/designer/templates'
    loginAs()
    const w = mount(AppShell)

    const active = w.find('[data-testid="nav-designer-templates"]')
    expect(active.classes().join(' ')).toContain('text-primary')

    const inactive = w.find('[data-testid="nav-designer-forms"]')
    expect(inactive.classes().join(' ')).not.toContain('text-primary')
  })

  it('側邊欄收合時不顯示子項目', async () => {
    loginAs()
    localStorage.setItem('app.sidebar.collapsed', '1')
    const w = mount(AppShell)

    // 收合狀態只有圖示，沒有空間放子選單
    expect(w.find('[data-testid="nav-designer-forms"]').exists()).toBe(false)
  })
})
