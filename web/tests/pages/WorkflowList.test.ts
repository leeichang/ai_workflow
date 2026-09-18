/**
 * 流程清單與新增測試
 *
 * 與 FormList 同一個缺口：後端有 POST /workflows、前端的
 * createWorkflow() 也已定義，但先前沒有任何畫面呼叫它——
 * /designer/workflows/:workflowKey 只能開既有流程，
 * 選單寫死指向 quotation_approval。
 *
 * 流程比表單多一層限制：新流程要通過 WF-E001（恰一個 trigger、
 * 至少一個 end），所以初始內容不能只是空殼。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { server } from '../msw/server'
import WorkflowList from '@/pages/WorkflowList.vue'
import { useSession } from '@/auth/useSession'

const API = '*/api'

const push = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ path: '/designer/workflows', query: {} }),
  RouterLink: { template: '<a><slot /></a>' },
}))

function workflow(over: Record<string, unknown> = {}) {
  return {
    id: 'w1',
    tenant_id: 't1',
    workflow_key: 'quotation_approval',
    business_object: 'quotation',
    name: '報價單簽核',
    description: '報價單的多關簽核流程',
    published_version: 3,
    has_draft: false,
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    ...over,
  }
}

function mockList(items: unknown[]) {
  server.use(http.get(`${API}/workflows`, () => HttpResponse.json(items)))
}

function loginAsDesigner(): void {
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

async function render() {
  const w = mount(WorkflowList)
  await flushPromises()
  return w
}

describe('流程清單', () => {
  beforeEach(() => {
    push.mockClear()
    loginAsDesigner()
  })

  it('列出既有流程', async () => {
    mockList([
      workflow(),
      workflow({ id: 'w2', workflow_key: 'pr_approval', name: '採購申請簽核' }),
    ])
    const w = await render()

    expect(
      w.find('[data-testid="workflow-row-quotation_approval"]').exists(),
    ).toBe(true)
    expect(w.find('[data-testid="workflow-row-pr_approval"]').exists()).toBe(
      true,
    )
  })

  it('沒有流程時顯示空狀態與新增指引', async () => {
    mockList([])
    const w = await render()

    expect(w.find('[data-testid="workflow-empty"]').exists()).toBe(true)
    expect(w.find('[data-testid="workflow-create"]').exists()).toBe(true)
  })

  it('顯示草稿狀態', async () => {
    mockList([workflow({ has_draft: true, published_version: 2 })])
    const w = await render()

    expect(
      w.find('[data-testid="workflow-row-quotation_approval"]').text(),
    ).toContain('草稿')
  })

  it('未發布的流程標示為尚未發布', async () => {
    mockList([workflow({ published_version: null, has_draft: true })])
    const w = await render()

    expect(
      w.find('[data-testid="workflow-row-quotation_approval"]').text(),
    ).toContain('尚未發布')
  })
})

describe('新增流程', () => {
  beforeEach(() => {
    push.mockClear()
    loginAsDesigner()
    mockList([])
  })

  it('按新增開啟對話框', async () => {
    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')

    expect(w.find('[data-testid="create-dialog"]').exists()).toBe(true)
  })

  it('送出後呼叫 API 並導向設計器', async () => {
    let captured: unknown
    server.use(
      http.post(`${API}/workflows`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({ definition: workflow() }, { status: 201 })
      }),
    )

    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-key"]').setValue('leave_approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假簽核')
    await w.find('[data-testid="new-business-object"]').setValue('leave_request')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(captured).toMatchObject({
      workflow_key: 'leave_approval',
      business_object: 'leave_request',
      name: '請假簽核',
    })

    expect(push).toHaveBeenCalledWith('/designer/workflows/leave_approval')
  })

  it('初始內容通過 WF-E001：恰一個 trigger 且至少一個 end', async () => {
    let captured: unknown
    server.use(
      http.post(`${API}/workflows`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({ definition: workflow() }, { status: 201 })
      }),
    )

    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-key"]').setValue('leave_approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假簽核')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    const content = (captured as {
      content: {
        trigger: { type: string }
        nodes: { id: string; type: string }[]
        edges: unknown[]
      }
    }).content

    // 空殼流程會被 WF-E001 擋下。送出去才發現不合法的話，
    // 使用者看到的是 422 而不是一個可以開始編輯的流程。
    const triggers = content.nodes.filter((n) => n.type === 'trigger')
    const ends = content.nodes.filter((n) => n.type === 'end')
    expect(triggers).toHaveLength(1)
    expect(ends.length).toBeGreaterThanOrEqual(1)

    // WF-E002：邊的兩端都要存在。edge 是 [from, to] 陣列不是物件。
    expect(content.edges.length).toBeGreaterThan(0)
    expect(Array.isArray(content.edges[0])).toBe(true)

    expect(content.trigger.type).toBe('form_submit')
  })

  it('workflow_key 空白時擋下並提示', async () => {
    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假簽核')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').text()).toContain('代碼')
  })

  it('workflow_key 含大寫或連字號時擋下', async () => {
    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-key"]').setValue('Leave-Approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假簽核')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').exists()).toBe(true)
  })

  it('重複的 workflow_key 顯示後端的衝突訊息', async () => {
    server.use(
      http.post(`${API}/workflows`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '流程代碼已存在' },
          { status: 409 },
        ),
      ),
    )

    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w
      .find('[data-testid="new-workflow-key"]')
      .setValue('quotation_approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('報價單簽核')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').text()).toContain('已存在')
    expect(push).not.toHaveBeenCalled()
  })

  it('業務物件留空時沿用 workflow_key', async () => {
    let captured: unknown
    server.use(
      http.post(`${API}/workflows`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({ definition: workflow() }, { status: 201 })
      }),
    )

    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-key"]').setValue('leave_approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假簽核')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect((captured as { business_object: string }).business_object).toBe(
      'leave_approval',
    )
  })
})
