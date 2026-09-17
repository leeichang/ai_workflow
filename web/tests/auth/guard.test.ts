/**
 * 路由守衛測試
 *
 * 守衛是前端唯一擋住未登入者的地方。它擋不住有心人（改 JS 就繞過了），
 * 真正的防線在後端 Actor extractor——這裡的目的是讓一般使用者
 * 看到登入頁而不是一堆 401 錯誤的空白畫面。
 */

import { beforeEach, describe, expect, it } from 'vitest'
import { createRouter, createMemoryHistory } from 'vue-router'
import { installAuthGuard } from '@/auth/guard'
import { useSession } from '@/auth/useSession'

const Blank = { template: '<div />' }

function makeRouter() {
  const router = createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', name: 'home', component: Blank },
      { path: '/login', name: 'login', component: Blank, meta: { public: true } },
      { path: '/tasks', name: 'tasks', component: Blank },
      {
        path: '/designer/forms/:formKey',
        name: 'form-designer',
        component: Blank,
      },
    ],
  })
  installAuthGuard(router)
  return router
}

function loginAs(roles: string[] = ['designer']) {
  const s = useSession()
  s.reset()
  // 直接寫 storage 再 restore，避開 API 呼叫
  localStorage.setItem(
    'workflow.session',
    JSON.stringify({
      access_token: 'header.payload.sig',
      user: {
        id: 'u1',
        name: '王小明',
        email: 'a@demo.local',
        tenant_id: 't1',
        roles,
      },
    }),
  )
  s.restore()
}

beforeEach(() => {
  localStorage.clear()
  useSession().reset()
})

describe('未登入', () => {
  it('存取受保護路由會導向登入頁', async () => {
    const router = makeRouter()
    await router.push('/tasks')
    expect(router.currentRoute.value.path).toBe('/login')
  })

  it('原本要去的路徑記在 redirect query', async () => {
    const router = makeRouter()
    await router.push('/designer/forms/quotation_form')
    expect(router.currentRoute.value.query.redirect).toBe(
      '/designer/forms/quotation_form',
    )
  })

  it('可以進入登入頁本身，不會無限重導', async () => {
    const router = makeRouter()
    await router.push('/login')
    expect(router.currentRoute.value.path).toBe('/login')
  })
})

describe('已登入', () => {
  it('可以進入受保護路由', async () => {
    loginAs()
    const router = makeRouter()
    await router.push('/tasks')
    expect(router.currentRoute.value.path).toBe('/tasks')
  })

  it('進登入頁會被導回首頁', async () => {
    loginAs()
    const router = makeRouter()
    await router.push('/login')
    expect(router.currentRoute.value.path).toBe('/')
  })
})

describe('登出後', () => {
  it('原本可進的路由變成要重新登入', async () => {
    loginAs()
    const router = makeRouter()
    await router.push('/tasks')
    expect(router.currentRoute.value.path).toBe('/tasks')

    useSession().logout()
    await router.push('/designer/forms/quotation_form')
    expect(router.currentRoute.value.path).toBe('/login')
  })
})
