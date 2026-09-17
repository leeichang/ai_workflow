/**
 * 登入狀態測試
 *
 * 測的是「狀態機」而非 API 呼叫本身——API 層已由 tests/api/auth.test.ts 覆蓋。
 * 這裡關心的是：token 什麼時候寫進 storage、什麼時候被清掉、
 * 重新整理後能不能回復、過期的 token 會不會被當成已登入。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'
import { server } from '../msw/server'
import { setTokenGetter } from '@/api/client'
import { STORAGE_KEY, useSession } from '@/auth/useSession'

const API = '*/api'

/** 產生一個 exp 在指定秒數後的 JWT（僅結構正確，不需真簽章） */
function fakeJwt(expiresInSeconds: number): string {
  const payload = {
    sub: 'u1',
    tenant_id: 't1',
    name: '王小明',
    roles: ['designer'],
    exp: Math.floor(Date.now() / 1000) + expiresInSeconds,
    iat: Math.floor(Date.now() / 1000),
  }
  const b64 = (o: unknown) =>
    btoa(String.fromCharCode(...new TextEncoder().encode(JSON.stringify(o))))
      .replace(/\+/g, '-')
      .replace(/\//g, '_')
      .replace(/=+$/, '')
  return `${b64({ alg: 'HS256', typ: 'JWT' })}.${b64(payload)}.fakesig`
}

const USER = {
  id: 'u1',
  name: '王小明',
  email: 'a@acme.test',
  tenant_id: 't1',
  roles: ['designer'],
}

function mockLoginOk(token = fakeJwt(3600)) {
  server.use(
    http.post(`${API}/auth/login`, () =>
      HttpResponse.json({ access_token: token, user: USER }),
    ),
  )
}

beforeEach(() => {
  localStorage.clear()
  useSession().reset()
  // client.ts 的 tokenGetter 在 setup.ts 被固定成 test-token，
  // 這裡改回讀 storage，才能驗證 session 與 client 的接線。
  setTokenGetter(() => useSession().token.value)
})

describe('初始狀態', () => {
  it('沒有 storage 時為未登入', () => {
    const s = useSession()
    expect(s.isAuthenticated.value).toBe(false)
    expect(s.user.value).toBeNull()
    expect(s.token.value).toBeNull()
  })

  it('從 storage 回復已登入狀態', () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ access_token: fakeJwt(3600), user: USER }),
    )
    const s = useSession()
    s.restore()
    expect(s.isAuthenticated.value).toBe(true)
    expect(s.user.value?.name).toBe('王小明')
  })

  it('storage 內容毀損時視為未登入，不拋錯', () => {
    localStorage.setItem(STORAGE_KEY, '{ this is not json')
    const s = useSession()
    expect(() => s.restore()).not.toThrow()
    expect(s.isAuthenticated.value).toBe(false)
    // 壞掉的資料要清掉，否則每次開啟都重複解析失敗
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull()
  })

  it('token 已過期時視為未登入', () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ access_token: fakeJwt(-60), user: USER }),
    )
    const s = useSession()
    s.restore()
    expect(s.isAuthenticated.value).toBe(false)
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull()
  })
})

describe('登入', () => {
  it('成功後寫入狀態與 storage', async () => {
    mockLoginOk()
    const s = useSession()
    await s.login({ tenant_code: 'acme', email: 'a@acme.test', password: 'pw' })

    expect(s.isAuthenticated.value).toBe(true)
    expect(s.user.value?.email).toBe('a@acme.test')
    expect(localStorage.getItem(STORAGE_KEY)).not.toBeNull()
  })

  it('登入後 API client 會帶上新 token', async () => {
    const token = fakeJwt(3600)
    mockLoginOk(token)
    const s = useSession()
    await s.login({ tenant_code: 'acme', email: 'a@acme.test', password: 'pw' })

    let sent: string | null = null
    server.use(
      http.get(`${API}/me`, ({ request }) => {
        sent = request.headers.get('Authorization')
        return HttpResponse.json(USER)
      }),
    )
    const { me } = await import('@/api/auth')
    await me()

    expect(sent).toBe(`Bearer ${token}`)
  })

  it('失敗時不寫入任何狀態', async () => {
    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json(
          { code: 'UNAUTHORIZED', message: '帳號或密碼錯誤' },
          { status: 401 },
        ),
      ),
    )
    const s = useSession()
    await expect(
      s.login({ tenant_code: 'acme', email: 'a@acme.test', password: 'wrong' }),
    ).rejects.toThrow()

    expect(s.isAuthenticated.value).toBe(false)
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull()
  })

  it('登入失敗不會清掉既有的登入狀態', async () => {
    // 情境：已登入的使用者在另一個分頁誤觸登入並失敗，
    // 不該因此被踢出原本的工作階段
    mockLoginOk()
    const s = useSession()
    await s.login({ tenant_code: 'acme', email: 'a@acme.test', password: 'pw' })

    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json(
          { code: 'UNAUTHORIZED', message: '帳號或密碼錯誤' },
          { status: 401 },
        ),
      ),
    )
    await expect(
      s.login({ tenant_code: 'acme', email: 'b@acme.test', password: 'wrong' }),
    ).rejects.toThrow()

    expect(s.isAuthenticated.value).toBe(true)
    expect(s.user.value?.email).toBe('a@acme.test')
  })
})

describe('登出', () => {
  it('清除狀態與 storage', async () => {
    mockLoginOk()
    const s = useSession()
    await s.login({ tenant_code: 'acme', email: 'a@acme.test', password: 'pw' })
    s.logout()

    expect(s.isAuthenticated.value).toBe(false)
    expect(s.user.value).toBeNull()
    expect(s.token.value).toBeNull()
    expect(localStorage.getItem(STORAGE_KEY)).toBeNull()
  })

  it('未登入時登出不拋錯', () => {
    const s = useSession()
    expect(() => s.logout()).not.toThrow()
  })
})

describe('角色', () => {
  it('hasRole 判斷角色', async () => {
    mockLoginOk()
    const s = useSession()
    await s.login({ tenant_code: 'acme', email: 'a@acme.test', password: 'pw' })

    expect(s.hasRole('designer')).toBe(true)
    expect(s.hasRole('finance_manager')).toBe(false)
  })

  it('admin 視為擁有任何角色，與後端 Actor::is_admin 一致', async () => {
    mockLoginOk()
    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json({
          access_token: fakeJwt(3600),
          user: { ...USER, roles: ['admin'] },
        }),
      ),
    )
    const s = useSession()
    await s.login({ tenant_code: 'acme', email: 'a@acme.test', password: 'pw' })

    expect(s.hasRole('finance_manager')).toBe(true)
  })

  it('未登入時不具備任何角色', () => {
    const s = useSession()
    expect(s.hasRole('designer')).toBe(false)
    expect(s.hasRole('admin')).toBe(false)
  })
})

describe('狀態共享', () => {
  it('多次呼叫 useSession 取得同一份狀態', async () => {
    mockLoginOk()
    await useSession().login({
      tenant_code: 'acme',
      email: 'a@acme.test',
      password: 'pw',
    })
    // 另一個元件取得的 session 應該已經是登入狀態
    expect(useSession().isAuthenticated.value).toBe(true)
  })
})
