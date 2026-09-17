/**
 * 登入頁測試
 *
 * 重點在登入失敗的處理：錯誤訊息要顯示、密碼欄要保留（讓使用者
 * 改打而非重填全部）、送出中不能重複點擊。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { server } from '../msw/server'
import LoginPage from '@/pages/LoginPage.vue'
import { useSession } from '@/auth/useSession'

const API = '*/api'

const push = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ query: {} }),
}))

function fakeJwt(): string {
  const payload = { exp: Math.floor(Date.now() / 1000) + 3600 }
  const b64 = (o: unknown) => btoa(JSON.stringify(o)).replace(/=+$/, '')
  return `${b64({ alg: 'HS256' })}.${b64(payload)}.sig`
}

const USER = {
  id: 'u1',
  name: '王景榮',
  email: 'sales@demo.local',
  tenant_id: 't1',
  roles: ['requester'],
}

async function fillAndSubmit(
  wrapper: ReturnType<typeof mount>,
  email = 'sales@demo.local',
  password = 'pw',
) {
  await wrapper.find('[data-testid="login-tenant"]').setValue('demo')
  await wrapper.find('[data-testid="login-email"]').setValue(email)
  await wrapper.find('[data-testid="login-password"]').setValue(password)
  await wrapper.find('[data-testid="login-submit"]').trigger('submit')
  await new Promise((r) => setTimeout(r, 0))
  await wrapper.vm.$nextTick()
}

beforeEach(() => {
  localStorage.clear()
  useSession().reset()
  push.mockClear()
})

describe('版面', () => {
  it('顯示租戶、帳號、密碼三個欄位', () => {
    const w = mount(LoginPage)
    expect(w.find('[data-testid="login-tenant"]').exists()).toBe(true)
    expect(w.find('[data-testid="login-email"]').exists()).toBe(true)
    expect(w.find('[data-testid="login-password"]').exists()).toBe(true)
  })

  it('密碼欄位為 password 型別', () => {
    const w = mount(LoginPage)
    expect(w.find('[data-testid="login-password"]').attributes('type')).toBe('password')
  })

  it('初始不顯示錯誤訊息', () => {
    const w = mount(LoginPage)
    expect(w.find('[data-testid="login-error"]').exists()).toBe(false)
  })
})

describe('登入成功', () => {
  it('導向首頁並建立 session', async () => {
    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json({ access_token: fakeJwt(), user: USER }),
      ),
    )
    const w = mount(LoginPage)
    await fillAndSubmit(w)

    expect(useSession().isAuthenticated.value).toBe(true)
    expect(push).toHaveBeenCalled()
  })
})

describe('登入失敗', () => {
  it('顯示後端回傳的訊息', async () => {
    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json(
          { code: 'UNAUTHORIZED', message: '帳號或密碼錯誤' },
          { status: 401 },
        ),
      ),
    )
    const w = mount(LoginPage)
    await fillAndSubmit(w, 'sales@demo.local', 'wrong')

    const error = w.find('[data-testid="login-error"]')
    expect(error.exists()).toBe(true)
    expect(error.text()).toContain('帳號或密碼錯誤')
    expect(push).not.toHaveBeenCalled()
  })

  it('帳號停用時顯示 403 的訊息', async () => {
    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json({ code: 'FORBIDDEN', message: '帳號已停用' }, { status: 403 }),
      ),
    )
    const w = mount(LoginPage)
    await fillAndSubmit(w)
    expect(w.find('[data-testid="login-error"]').text()).toContain('帳號已停用')
  })

  it('網路錯誤時顯示可理解的訊息，而非原始例外', async () => {
    server.use(http.post(`${API}/auth/login`, () => HttpResponse.error()))
    const w = mount(LoginPage)
    await fillAndSubmit(w)

    const text = w.find('[data-testid="login-error"]').text()
    expect(text.length).toBeGreaterThan(0)
    expect(text).not.toContain('TypeError')
  })

  it('保留已輸入的租戶與帳號，只清密碼', async () => {
    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json(
          { code: 'UNAUTHORIZED', message: '帳號或密碼錯誤' },
          { status: 401 },
        ),
      ),
    )
    const w = mount(LoginPage)
    await fillAndSubmit(w, 'sales@demo.local', 'wrong')

    expect(
      (w.find('[data-testid="login-tenant"]').element as HTMLInputElement).value,
    ).toBe('demo')
    expect(
      (w.find('[data-testid="login-email"]').element as HTMLInputElement).value,
    ).toBe('sales@demo.local')
    expect(
      (w.find('[data-testid="login-password"]').element as HTMLInputElement).value,
    ).toBe('')
  })

  it('再次送出會清掉上一次的錯誤訊息', async () => {
    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json(
          { code: 'UNAUTHORIZED', message: '帳號或密碼錯誤' },
          { status: 401 },
        ),
      ),
    )
    const w = mount(LoginPage)
    await fillAndSubmit(w, 'sales@demo.local', 'wrong')
    expect(w.find('[data-testid="login-error"]').exists()).toBe(true)

    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json({ access_token: fakeJwt(), user: USER }),
      ),
    )
    await fillAndSubmit(w, 'sales@demo.local', 'correct')
    expect(w.find('[data-testid="login-error"]').exists()).toBe(false)
  })
})

describe('送出中', () => {
  it('送出期間按鈕停用，避免重複登入', async () => {
    let resolve: (() => void) | null = null
    const gate = new Promise<void>((r) => {
      resolve = r
    })
    server.use(
      http.post(`${API}/auth/login`, async () => {
        await gate
        return HttpResponse.json({ access_token: fakeJwt(), user: USER })
      }),
    )

    const w = mount(LoginPage)
    await w.find('[data-testid="login-tenant"]').setValue('demo')
    await w.find('[data-testid="login-email"]').setValue('sales@demo.local')
    await w.find('[data-testid="login-password"]').setValue('pw')
    void w.find('[data-testid="login-submit"]').trigger('submit')
    await w.vm.$nextTick()

    expect(
      (w.find('[data-testid="login-submit"]').element as HTMLButtonElement).disabled,
    ).toBe(true)

    resolve?.()
  })
})
