/**
 * 認證 API client 測試
 */

import { HttpResponse, http } from 'msw'
import { describe, expect, it } from 'vitest'
import { server } from '../msw/server'
import * as auth from '@/api/auth'
import { ApiError } from '@/api/types'

const API = '*/api'

describe('login', () => {
  it('送出 tenant_code、email、password', async () => {
    let captured: unknown
    server.use(
      http.post(`${API}/auth/login`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({
          access_token: 'jwt-token',
          user: {
            id: 'u1',
            name: '王小明',
            email: 'a@acme.test',
            tenant_id: 't1',
            roles: ['designer'],
          },
        })
      }),
    )

    await auth.login({
      tenant_code: 'acme',
      email: 'a@acme.test',
      password: 'secret',
    })

    expect(captured).toEqual({
      tenant_code: 'acme',
      email: 'a@acme.test',
      password: 'secret',
    })
  })

  it('不附加 Authorization 標頭', async () => {
    let hasAuth = true
    server.use(
      http.post(`${API}/auth/login`, ({ request }) => {
        hasAuth = request.headers.has('authorization')
        return HttpResponse.json({ access_token: 't', user: {} })
      }),
    )

    // 即使 localStorage 有舊 token，登入也不該帶上
    await auth.login({ tenant_code: 'acme', email: 'a@b.c', password: 'x' })
    expect(hasAuth, '登入端點不該帶 token，否則過期 token 會先被認證層擋下').toBe(false)
  })

  it('解析 token 與使用者資訊', async () => {
    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json({
          access_token: 'jwt-abc',
          user: {
            id: 'u1',
            name: '李小華',
            email: 'b@globex.test',
            tenant_id: 't2',
            roles: ['designer', 'approver'],
          },
        }),
      ),
    )

    const res = await auth.login({ tenant_code: 'globex', email: 'b@globex.test', password: 'x' })
    expect(res.access_token).toBe('jwt-abc')
    expect(res.user.roles).toEqual(['designer', 'approver'])
  })

  it('帳密錯誤回 401', async () => {
    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json({ code: 'UNAUTHORIZED', message: '帳號或密碼錯誤' }, { status: 401 }),
      ),
    )

    const err = await auth
      .login({ tenant_code: 'acme', email: 'a@b.c', password: 'wrong' })
      .catch((e) => e)

    expect(err).toBeInstanceOf(ApiError)
    expect(err.code).toBe('UNAUTHORIZED')
    // 不洩漏帳號是否存在
    expect(err.message).toBe('帳號或密碼錯誤')
  })

  it('停用帳號回 403', async () => {
    server.use(
      http.post(`${API}/auth/login`, () =>
        HttpResponse.json({ code: 'FORBIDDEN', message: '帳號已停用' }, { status: 403 }),
      ),
    )

    const err = await auth.login({ tenant_code: 'acme', email: 'a@b.c', password: 'x' }).catch((e) => e)
    expect(err.code).toBe('FORBIDDEN')
  })
})

describe('me', () => {
  it('帶 token 取得當前使用者', async () => {
    let auth_header: string | null = null
    server.use(
      http.get(`${API}/me`, ({ request }) => {
        auth_header = request.headers.get('authorization')
        return HttpResponse.json({
          id: 'u1',
          tenant_id: 't1',
          name: '王小明',
          roles: ['designer'],
        })
      }),
    )

    const user = await auth.me()
    expect(auth_header).toBe('Bearer test-token')
    expect(user.name).toBe('王小明')
  })

  it('token 過期回 401', async () => {
    server.use(
      http.get(`${API}/me`, () =>
        HttpResponse.json({ code: 'UNAUTHORIZED', message: 'token 已過期' }, { status: 401 }),
      ),
    )

    const err = await auth.me().catch((e) => e)
    expect(err.code).toBe('UNAUTHORIZED')
  })
})
