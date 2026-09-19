/**
 * 模擬簽核畫面測試
 *
 * 使用者的原話：
 *   「執行時可以直接由系統依據目前的系統角色設定解析出實際簽核的人員，
 *     測試的人可以直接點選模擬這個人簽核」
 *
 * 所以這個畫面的核心是：
 *   1. 顯示流程走到哪、誰在簽（真實解析出來的人，不是假資料）
 *   2. 點一下就以那個人的身分簽核，不換身分登入
 *
 * 另一個硬性要求是**不可關閉的開發模式標示**——
 * 使用者在開發模式核准了，若以為正式也核准了，那比不給模擬更糟。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { server } from '../msw/server'
import SimulationMode from '@/pages/SimulationMode.vue'
import { useSession } from '@/auth/useSession'

const API = '*/api'
const SANDBOX_ID = 'sb-1'

const push = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ path: '/simulation', params: {}, query: {} }),
  RouterLink: { template: '<a><slot /></a>' },
}))

function session(over: Record<string, unknown> = {}) {
  return {
    id: 'ss-1',
    tenant_id: 't1',
    sandbox_tenant_id: SANDBOX_ID,
    created_by: 'u1',
    created_by_in_sandbox: 'u1-sb',
    status: 'ACTIVE',
    created_at: '2026-09-19T00:00:00Z',
    expires_at: '2026-09-26T00:00:00Z',
    ...over,
  }
}

function task(over: Record<string, unknown> = {}) {
  return {
    task_id: 'tk-1',
    instance_id: 'in-1',
    business_key: 'QT-SIM-1',
    node_id: 'manager_approval',
    node_label: '主管簽核',
    assignee_user_id: 'cfo-id',
    assignee_name: '張文華',
    assignee_role: 'cfo',
    ...over,
  }
}

function login() {
  localStorage.setItem(
    'workflow.session',
    JSON.stringify({
      access_token: 'h.p.s',
      user: {
        id: 'u1',
        name: '陳雅婷',
        email: 'designer@demo.local',
        tenant_id: 't1',
        roles: ['designer'],
      },
    }),
  )
  useSession().restore()
}

function mockSandbox(sessions: unknown[], tasks: unknown[]) {
  server.use(
    http.get(`${API}/sandboxes`, () => HttpResponse.json(sessions)),
    http.get(`${API}/sandboxes/${SANDBOX_ID}/tasks`, () =>
      HttpResponse.json(tasks),
    ),
  )
}

async function render() {
  const w = mount(SimulationMode)
  await flushPromises()
  return w
}

describe('開發模式標示', () => {
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('有進行中的開發模式時顯示不可關閉的標示', async () => {
    // 硬性要求。使用者在開發模式核准了，若以為正式也核准了，
    // 那比不給模擬更糟。
    mockSandbox([session()], [task()])
    const w = await render()

    expect(w.find('[data-testid="sandbox-banner"]').exists()).toBe(true)
    // 沒有關閉按鈕
    expect(w.find('[data-testid="sandbox-banner-close"]').exists()).toBe(false)
  })

  it('沒有開發模式時顯示建立入口而非空白', async () => {
    mockSandbox([], [])
    const w = await render()

    expect(w.find('[data-testid="sandbox-empty"]').exists()).toBe(true)
    expect(w.find('[data-testid="sandbox-create"]').exists()).toBe(true)
    expect(w.find('[data-testid="sandbox-banner"]').exists()).toBe(false)
  })
})

describe('待簽清單', () => {
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('顯示流程走到哪、誰在簽', async () => {
    mockSandbox([session()], [task()])
    const w = await render()

    const row = w.find('[data-testid="sim-task-tk-1"]')
    expect(row.exists()).toBe(true)
    expect(row.text()).toContain('主管簽核')
    expect(row.text()).toContain('張文華')
    // 解析依據要看得到——這是模擬要驗證的東西
    expect(row.text()).toContain('cfo')
  })

  it('每一筆都有「以這個人簽核」的按鈕', async () => {
    // 使用者要的是點選扮演，不是登入五次
    mockSandbox([session()], [task()])
    const w = await render()

    expect(w.find('[data-testid="act-as-tk-1"]').exists()).toBe(true)
  })

  it('沒有待簽時明說流程已走完或尚未啟動', async () => {
    mockSandbox([session()], [])
    const w = await render()

    expect(w.find('[data-testid="sim-no-tasks"]').exists()).toBe(true)
  })

  it('解析不到簽核人時顯示為待補，而非假裝有人', async () => {
    // resolver 解析不到人要明確顯示，不可跳過或用預設人員頂替——
    // 頂替會讓使用者以為設定是對的
    mockSandbox(
      [session()],
      [task({ assignee_user_id: null, assignee_name: null, assignee_role: 'finance_manager' })],
    )
    const w = await render()

    const row = w.find('[data-testid="sim-task-tk-1"]')
    expect(row.text()).toContain('無法解析')
    // 不能給扮演按鈕——沒有人可以扮演
    expect(w.find('[data-testid="act-as-tk-1"]').exists()).toBe(false)
  })
})

describe('模擬簽核', () => {
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('送出時帶上扮演對象與決策', async () => {
    let captured: unknown
    mockSandbox([session()], [task()])
    server.use(
      http.post(`${API}/sandboxes/${SANDBOX_ID}/tasks/tk-1/simulate`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({
          task_id: 'tk-1',
          acting_as: 'cfo-id',
          decision: 'APPROVE',
        })
      }),
    )

    const w = await render()
    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    await w.find('[data-testid="sim-approve"]').trigger('click')
    await flushPromises()

    expect(captured).toMatchObject({
      acting_as: 'cfo-id',
      decision: 'APPROVE',
    })
  })

  it('可以退回並填意見', async () => {
    let captured: { decision?: string; comment?: string } | undefined
    mockSandbox([session()], [task()])
    server.use(
      http.post(`${API}/sandboxes/${SANDBOX_ID}/tasks/tk-1/simulate`, async ({ request }) => {
        captured = (await request.json()) as typeof captured
        return HttpResponse.json({
          task_id: 'tk-1',
          acting_as: 'cfo-id',
          decision: 'REJECT',
        })
      }),
    )

    const w = await render()
    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    await w.find('[data-testid="sim-comment"]').setValue('金額需要再確認')
    await w.find('[data-testid="sim-reject"]').trigger('click')
    await flushPromises()

    expect(captured?.decision).toBe('REJECT')
    expect(captured?.comment).toBe('金額需要再確認')
  })

  it('扮演面板顯示正在扮演誰', async () => {
    // 使用者要知道自己現在是以誰的身分在看、在簽
    mockSandbox([session()], [task()])
    const w = await render()

    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    const panel = w.find('[data-testid="sim-panel"]')
    expect(panel.exists()).toBe(true)
    expect(panel.text()).toContain('張文華')
  })

  it('簽核失敗時顯示後端訊息', async () => {
    mockSandbox([session()], [task()])
    server.use(
      http.post(`${API}/sandboxes/${SANDBOX_ID}/tasks/tk-1/simulate`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '待辦已被處理' },
          { status: 409 },
        ),
      ),
    )

    const w = await render()
    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()
    await w.find('[data-testid="sim-approve"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="sim-error"]').text()).toContain('已被處理')
  })
})
