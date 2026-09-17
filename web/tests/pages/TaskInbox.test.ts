/**
 * 我的待辦測試
 *
 * 這是簽核流程唯一的操作入口，先前只能用 curl。
 * 重點在決策的錯誤處理：409（別人先處理了）與 503（引擎斷線）
 * 對使用者的意義完全不同，不能都顯示「失敗」。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { server } from '../msw/server'
import TaskInbox from '@/pages/TaskInbox.vue'
import { useSession } from '@/auth/useSession'

const API = '*/api'

vi.mock('vue-router', () => ({
  useRouter: () => ({ push: vi.fn() }),
  useRoute: () => ({ path: '/tasks', query: {} }),
  RouterLink: { template: '<a><slot /></a>' },
}))

function task(over: Record<string, unknown> = {}) {
  return {
    id: 'task-1',
    instance_id: 'inst-1',
    node_id: 'manager_approval',
    node_label: '主管簽核',
    assignee_user_id: 'u1',
    assignee_role: null,
    participant_kind: 'internal',
    form_key: 'quotation_form',
    status: 'PENDING',
    decision: null,
    comment: null,
    due_at: null,
    created_at: '2026-09-16T06:08:38Z',
    decided_at: null,
    business_key: 'QT-MANUAL-001',
    business_object: 'quotation',
    ...over,
  }
}

function mockList(items: unknown[]) {
  server.use(http.get(`${API}/tasks`, () => HttpResponse.json(items)))
}

async function render() {
  const w = mount(TaskInbox)
  await flushPromises()
  return w
}

beforeEach(() => {
  localStorage.setItem(
    'workflow.session',
    JSON.stringify({
      access_token: 'h.p.s',
      user: {
        id: 'u1',
        name: '張文華',
        email: 'cfo@demo.local',
        tenant_id: 't1',
        roles: ['cfo', 'approver'],
      },
    }),
  )
  useSession().restore()
})

describe('列表', () => {
  it('顯示待辦的節點名稱與單號', async () => {
    mockList([task()])
    const w = await render()

    const text = w.text()
    expect(text).toContain('主管簽核')
    expect(text).toContain('QT-MANUAL-001')
  })

  it('空收件匣顯示提示而非空白畫面', async () => {
    mockList([])
    const w = await render()

    expect(w.find('[data-testid="inbox-empty"]').exists()).toBe(true)
  })

  it('載入失敗顯示錯誤，不是無盡的載入中', async () => {
    server.use(
      http.get(`${API}/tasks`, () =>
        HttpResponse.json(
          { code: 'INTERNAL_ERROR', message: '資料庫連線失敗' },
          { status: 500 },
        ),
      ),
    )
    const w = await render()

    expect(w.find('[data-testid="inbox-error"]').exists()).toBe(true)
    expect(w.find('[data-testid="inbox-loading"]').exists()).toBe(false)
  })

  it('多筆待辦全部列出', async () => {
    mockList([
      task(),
      task({ id: 'task-2', node_label: '財務簽核', business_key: 'QT-002' }),
    ])
    const w = await render()

    expect(w.findAll('[data-testid^="task-row-"]')).toHaveLength(2)
  })
})

describe('決策', () => {
  it('核准後該筆從列表消失', async () => {
    mockList([task()])
    const w = await render()

    server.use(
      http.post(`${API}/tasks/task-1/decision`, () =>
        HttpResponse.json({
          task_id: 'task-1',
          decision: 'APPROVE',
          signalled: true,
        }),
      ),
      // 重新載入後已無待辦
      http.get(`${API}/tasks`, () => HttpResponse.json([])),
    )

    await w.find('[data-testid="approve-task-1"]').trigger('click')
    await flushPromises()

    expect(w.findAll('[data-testid^="task-row-"]')).toHaveLength(0)
  })

  it('退回會送出 REJECT', async () => {
    mockList([task()])
    const w = await render()

    let body: unknown
    server.use(
      http.post(`${API}/tasks/task-1/decision`, async ({ request }) => {
        body = await request.json()
        return HttpResponse.json({
          task_id: 'task-1',
          decision: 'REJECT',
          signalled: true,
        })
      }),
      http.get(`${API}/tasks`, () => HttpResponse.json([])),
    )

    await w.find('[data-testid="reject-task-1"]').trigger('click')
    await flushPromises()

    expect((body as { decision: string }).decision).toBe('REJECT')
  })

  it('意見欄的內容會一起送出', async () => {
    mockList([task()])
    const w = await render()

    let body: unknown
    server.use(
      http.post(`${API}/tasks/task-1/decision`, async ({ request }) => {
        body = await request.json()
        return HttpResponse.json({
          task_id: 'task-1',
          decision: 'APPROVE',
          signalled: true,
        })
      }),
      http.get(`${API}/tasks`, () => HttpResponse.json([])),
    )

    await w.find('[data-testid="comment-task-1"]').setValue('金額確認無誤')
    await w.find('[data-testid="approve-task-1"]').trigger('click')
    await flushPromises()

    expect((body as { comment: string }).comment).toBe('金額確認無誤')
  })

  it('409 顯示「已被處理」並重新載入列表', async () => {
    mockList([task()])
    const w = await render()

    let listCalls = 0
    server.use(
      http.post(`${API}/tasks/task-1/decision`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '待辦已被處理' },
          { status: 409 },
        ),
      ),
      http.get(`${API}/tasks`, () => {
        listCalls += 1
        return HttpResponse.json([])
      }),
    )

    await w.find('[data-testid="approve-task-1"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="inbox-error"]').text()).toContain('已被處理')
    // 別人處理掉了，列表已經過期，必須重抓
    expect(listCalls).toBeGreaterThan(0)
  })

  it('503 保留該筆待辦，讓使用者能重試', async () => {
    mockList([task()])
    const w = await render()

    server.use(
      http.post(`${API}/tasks/task-1/decision`, () =>
        HttpResponse.json(
          { code: 'SERVICE_UNAVAILABLE', message: '流程引擎未連線' },
          { status: 503 },
        ),
      ),
    )

    await w.find('[data-testid="approve-task-1"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="inbox-error"]').text()).toContain('流程引擎')
    // 決策沒生效，待辦必須還在
    expect(w.find('[data-testid="task-row-task-1"]').exists()).toBe(true)
  })

  it('送出期間按鈕停用，避免重複決策', async () => {
    mockList([task()])
    const w = await render()

    let resolve: (() => void) | null = null
    const gate = new Promise<void>((r) => {
      resolve = r
    })
    server.use(
      http.post(`${API}/tasks/task-1/decision`, async () => {
        await gate
        return HttpResponse.json({
          task_id: 'task-1',
          decision: 'APPROVE',
          signalled: true,
        })
      }),
    )

    void w.find('[data-testid="approve-task-1"]').trigger('click')
    await w.vm.$nextTick()

    const btn = w.find('[data-testid="approve-task-1"]').element as HTMLButtonElement
    expect(btn.disabled).toBe(true)

    resolve?.()
  })
})
