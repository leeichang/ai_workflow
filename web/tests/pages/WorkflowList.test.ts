/**
 * 流程清單與新增測試
 *
 * 與表單那半（FormList）同一個缺口：後端有 POST /workflows、
 * 前端的 createWorkflow() 也寫好了，但**沒有任何畫面呼叫它**——
 * 設計器只能開既有流程（路由寫死 :workflowKey），
 * 選單直接指向 quotation_approval。
 *
 * 換句話說：使用者無法從零建立一個流程。
 * 表單那半補完之後，這半補上，「從零建一張表單 + 一個流程」
 * 的操作手冊才寫得出來。
 *
 * 重點在初始 DSL 必須是**可發布的**。流程與表單不同：
 * 表單只要有一個欄位就合法，流程則要通過 WF-E001～E012
 * 的圖結構驗證——缺 start／end、有孤兒節點都會擋下發布。
 * 產生一個建了卻發布不了的流程，比不給建還糟。
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
    name: '報價單簽核流程',
    description: null,
    published_version: 1,
    has_draft: false,
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    ...over,
  }
}

function mockList(items: unknown[]) {
  server.use(http.get(`${API}/workflows`, () => HttpResponse.json(items)))
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

async function render() {
  const w = mount(WorkflowList)
  await flushPromises()
  return w
}

describe('流程清單', () => {
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('列出既有流程', async () => {
    mockList([
      workflow(),
      workflow({ id: 'w2', workflow_key: 'pr_approval', name: '採購簽核' }),
    ])
    const w = await render()

    expect(w.find('[data-testid="workflow-row-quotation_approval"]').exists()).toBe(
      true,
    )
    expect(w.find('[data-testid="workflow-row-pr_approval"]').exists()).toBe(true)
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

    expect(w.find('[data-testid="workflow-row-quotation_approval"]').text()).toContain(
      '草稿',
    )
  })

  it('未發布的流程標示為尚未發布', async () => {
    mockList([workflow({ published_version: null, has_draft: true })])
    const w = await render()

    expect(w.find('[data-testid="workflow-row-quotation_approval"]').text()).toContain(
      '尚未發布',
    )
  })

  it('載入失敗時顯示錯誤而非空白畫面', async () => {
    server.use(
      http.get(`${API}/workflows`, () =>
        HttpResponse.json({ message: '伺服器錯誤' }, { status: 500 }),
      ),
    )
    const w = await render()

    expect(w.find('[data-testid="workflow-error"]').exists()).toBe(true)
  })
})

describe('新增流程', () => {
  beforeEach(() => {
    push.mockClear()
    login()
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
        return HttpResponse.json(workflow(), { status: 201 })
      }),
    )

    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')

    await w.find('[data-testid="new-workflow-key"]').setValue('leave_approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假流程')
    await w.find('[data-testid="new-business-object"]').setValue('leave')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(captured).toMatchObject({
      workflow_key: 'leave_approval',
      business_object: 'leave',
      name: '請假流程',
    })

    // 建完直接進設計器，不要讓使用者自己再點一次
    expect(push).toHaveBeenCalledWith('/designer/workflows/leave_approval')
  })

  it('初始 DSL 含 trigger、nodes、edges 三個必填', async () => {
    // schemas/workflow-dsl.schema.json 的頂層 required 是
    // workflow_key / version / business_object / trigger / nodes / edges
    let captured: { content: Record<string, unknown> } | undefined
    server.use(
      http.post(`${API}/workflows`, async ({ request }) => {
        captured = (await request.json()) as { content: Record<string, unknown> }
        return HttpResponse.json(workflow(), { status: 201 })
      }),
    )

    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-key"]').setValue('leave_approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假流程')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    const c = captured!.content
    expect(c.trigger).toBeDefined()
    expect(Array.isArray(c.nodes)).toBe(true)
    expect(Array.isArray(c.edges)).toBe(true)
    expect(c.business_object).toBe('leave_approval') // 留空時沿用 key
  })

  it('初始 DSL 有 start 與 end，且沒有孤兒節點', async () => {
    // 這是流程與表單最大的差別：圖結構要通過 WF-E001～E012 才能發布。
    // 產生一個建了卻發布不了的流程，比不給建還糟——
    // 使用者會以為是自己設計錯了。
    let captured: { content: { nodes: { id: string; type: string }[]; edges: unknown[] } } | undefined
    server.use(
      http.post(`${API}/workflows`, async ({ request }) => {
        captured = (await request.json()) as typeof captured
        return HttpResponse.json(workflow(), { status: 201 })
      }),
    )

    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-key"]').setValue('leave_approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假流程')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    const { nodes, edges } = captured!.content
    expect(nodes.some((n) => n.type === 'trigger')).toBe(true)
    expect(nodes.some((n) => n.type === 'end')).toBe(true)

    // 每個節點都要被至少一條邊碰到，否則 WF-E003 會判為孤兒
    const touched = new Set(
      (edges as [string, string][]).flatMap(([from, to]) => [from, to]),
    )
    for (const n of nodes) {
      expect(touched.has(n.id), `節點 ${n.id} 沒有任何邊相連`).toBe(true)
    }
  })

  it('初始 DSL 不引用任何資料路徑', async () => {
    // WF-E012 會檢查條件式與 resolver 的路徑是否存在於業務物件定義。
    // 新流程的業務物件多半還沒有定義檔，引用任何路徑都會讓發布失敗。
    let captured: { content: unknown } | undefined
    server.use(
      http.post(`${API}/workflows`, async ({ request }) => {
        captured = (await request.json()) as { content: unknown }
        return HttpResponse.json(workflow(), { status: 201 })
      }),
    )

    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-key"]').setValue('leave_approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假流程')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    const text = JSON.stringify(captured!.content)
    expect(text).not.toContain('"expression"')
    expect(text).not.toContain('"path"')
  })

  it('workflow_key 空白時擋下並提示', async () => {
    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假流程')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').text()).toContain('代碼')
  })

  it('workflow_key 含大寫或連字號時擋下', async () => {
    // workflow_key 會進 API 路徑與資料表關聯，事後不能改
    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-key"]').setValue('Leave-Approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('請假流程')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').exists()).toBe(true)
    expect(push).not.toHaveBeenCalled()
  })

  it('名稱空白時擋下並提示', async () => {
    const w = await render()
    await w.find('[data-testid="workflow-create"]').trigger('click')
    await w.find('[data-testid="new-workflow-key"]').setValue('leave_approval')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').text()).toContain('名稱')
  })

  it('代碼重複時顯示後端訊息而非泛用錯誤', async () => {
    // 後端以 409 表達重複。使用者需要知道是「這個代碼已經有人用了」，
    // 而不是「建立失敗，請稍後再試」——後者會讓他一直重試同一個代碼。
    // 錯誤 body 必須同時有 code 與 message——client.ts 的 normalizeError
    // 兩者缺一就退回泛用訊息。少寫 code 的 mock 會讓這條測試
    // 測到「泛用訊息」而不是它想測的東西。
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
    await w.find('[data-testid="new-workflow-key"]').setValue('quotation_approval')
    await w.find('[data-testid="new-workflow-name"]').setValue('重複的流程')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').text()).toContain('已存在')
    expect(push).not.toHaveBeenCalled()
  })
})
