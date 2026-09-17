/**
 * AppShell 使用者區塊測試
 *
 * 這一區原本是寫死的「林建志 協理」，與實際登入者無關。
 * 測試釘住的是：顯示的必須是登入者本人，登出必須真的清掉狀態。
 */

import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import AppShell from '@/components/AppShell.vue'
import { useSession } from '@/auth/useSession'

const push = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ path: '/designer/forms/quotation_form' }),
  RouterLink: { template: '<a><slot /></a>' },
}))

function loginAs(name: string, email: string, roles: string[]) {
  localStorage.setItem(
    'workflow.session',
    JSON.stringify({
      access_token: 'h.p.s',
      user: { id: 'u1', name, email, tenant_id: 't1', roles },
    }),
  )
  useSession().restore()
}

beforeEach(() => {
  localStorage.clear()
  useSession().reset()
  push.mockClear()
})

describe('使用者資訊', () => {
  it('顯示登入者姓名，而非寫死的值', () => {
    loginAs('陳雅婷', 'designer@demo.local', ['designer'])
    const w = mount(AppShell)
    const name = w.find('[data-testid="user-name"]').text()

    expect(name).toBe('陳雅婷')
    expect(name).not.toBe('林建志 協理')
  })

  it('頭像顯示姓名第一個字', () => {
    loginAs('陳雅婷', 'designer@demo.local', ['designer'])
    const w = mount(AppShell)
    expect(w.find('[data-testid="user-menu-toggle"]').text()).toContain('陳')
  })

  it('切換使用者後顯示新的人', async () => {
    loginAs('陳雅婷', 'designer@demo.local', ['designer'])
    const w = mount(AppShell)
    expect(w.find('[data-testid="user-name"]').text()).toBe('陳雅婷')

    useSession().logout()
    loginAs('張文華', 'cfo@demo.local', ['cfo', 'approver'])
    await w.vm.$nextTick()

    expect(w.find('[data-testid="user-name"]').text()).toBe('張文華')
  })
})

describe('使用者選單', () => {
  it('預設收合', () => {
    loginAs('陳雅婷', 'designer@demo.local', ['designer'])
    const w = mount(AppShell)
    expect(w.find('[data-testid="user-menu-panel"]').exists()).toBe(false)
  })

  it('點擊展開，顯示 email 與登出', async () => {
    loginAs('陳雅婷', 'designer@demo.local', ['designer'])
    const w = mount(AppShell)
    await w.find('[data-testid="user-menu-toggle"]').trigger('click')

    const panel = w.find('[data-testid="user-menu-panel"]')
    expect(panel.exists()).toBe(true)
    expect(panel.text()).toContain('designer@demo.local')
    expect(w.find('[data-testid="logout-button"]').exists()).toBe(true)
  })

  it('再次點擊收合', async () => {
    loginAs('陳雅婷', 'designer@demo.local', ['designer'])
    const w = mount(AppShell)
    const toggle = w.find('[data-testid="user-menu-toggle"]')
    await toggle.trigger('click')
    await toggle.trigger('click')
    expect(w.find('[data-testid="user-menu-panel"]').exists()).toBe(false)
  })
})

describe('登出', () => {
  it('清除 session 並導向登入頁', async () => {
    loginAs('陳雅婷', 'designer@demo.local', ['designer'])
    const w = mount(AppShell)
    await w.find('[data-testid="user-menu-toggle"]').trigger('click')
    await w.find('[data-testid="logout-button"]').trigger('click')

    expect(useSession().isAuthenticated.value).toBe(false)
    expect(localStorage.getItem('workflow.session')).toBeNull()
    expect(push).toHaveBeenCalledWith('/login')
  })

  it('登出後選單收起', async () => {
    loginAs('陳雅婷', 'designer@demo.local', ['designer'])
    const w = mount(AppShell)
    await w.find('[data-testid="user-menu-toggle"]').trigger('click')
    await w.find('[data-testid="logout-button"]').trigger('click')
    expect(w.find('[data-testid="user-menu-panel"]').exists()).toBe(false)
  })
})
