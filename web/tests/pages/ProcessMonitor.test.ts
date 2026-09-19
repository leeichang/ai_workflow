/**
 * 流程監控畫面測試
 *
 * 交辦（§5 S0）：
 *   「選單有『流程監控』，看得到所有執行中流程的清單與詳情。
 *     權限正確（一般使用者看不到別人的單），欄位遮蔽正確。」
 *
 * 可見範圍由後端決定（見 instance_visibility.rs 的永久回歸測試）。
 * 前端不做過濾——前端過濾等於把資料送到瀏覽器再假裝看不到。
 * 這裡測的是「後端回什麼就呈現什麼，而且呈現得讓人看得懂」。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { server } from '../msw/server'
import ProcessMonitor from '@/pages/ProcessMonitor.vue'
import { useSession } from '@/auth/useSession'

const API = '*/api'

const push = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ path: '/monitor', params: {}, query: {} }),
  RouterLink: { template: '<a><slot /></a>' },
}))

function instance(over: Record<string, unknown> = {}) {
  return {
    id: 'in-1',
    workflow_version_id: 'v1',
    business_object: 'quotation',
    business_key: 'QT-001',
    temporal_workflow_id: 't:quotation:QT-001',
    temporal_run_id: 'r1',
    status: 'RUNNING',
    input: {},
    output: null,
    error: null,
    started_by: 'u1',
    started_at: '2026-09-19T01:00:00Z',
    ended_at: null,
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

function mockList(items: unknown[]) {
  server.use(http.get(`${API}/instances`, () => HttpResponse.json(items)))
}

function mockDetail(d: unknown, status = 200) {
  server.use(
    http.get(`${API}/instances/in-1`, () => HttpResponse.json(d, { status })),
  )
}

async function render() {
  const w = mount(ProcessMonitor)
  await flushPromises()
  return w
}

describe('流程清單', () => {
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('列出流程與狀態', async () => {
    mockList([instance()])
    const w = await render()

    const row = w.find('[data-testid="monitor-row-QT-001"]')
    expect(row.exists()).toBe(true)
    expect(row.text()).toContain('quotation')
    // 狀態用看得懂的字，不是 RUNNING
    expect(row.text()).toContain('執行中')
  })

  it('空清單說的是「沒有你看得到的單」而非「系統沒有單」', async () => {
    // 兩者差很多。說成後者會讓使用者以為流程根本沒跑起來，
    // 而實際上只是他看不到別人的單。
    mockList([])
    const w = await render()

    const empty = w.find('[data-testid="monitor-empty"]')
    expect(empty.exists()).toBe(true)
    expect(empty.text()).toContain('你看得到')
  })

  it('載入失敗顯示後端訊息，不是空清單', async () => {
    // 靜默回空清單會被讀成「沒有單」，是錯誤的結論
    server.use(
      http.get(`${API}/instances`, () =>
        HttpResponse.json(
          { code: 'INTERNAL', message: '資料庫連線失敗' },
          { status: 500 },
        ),
      ),
    )
    const w = await render()

    expect(w.find('[data-testid="monitor-error"]').text()).toContain(
      '資料庫連線失敗',
    )
    expect(w.find('[data-testid="monitor-empty"]').exists()).toBe(false)
  })
})

describe('流程詳情', () => {
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('顯示本地狀態與引擎回報兩個欄位', async () => {
    mockList([instance()])
    mockDetail({ ...instance(), live_status: 'RUNNING' })
    const w = await render()

    await w.find('[data-testid="monitor-open-QT-001"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="monitor-detail-status"]').text()).toContain(
      '執行中',
    )
    expect(w.find('[data-testid="monitor-detail-live"]').text()).toContain(
      'RUNNING',
    )
  })

  it('本地說執行中、引擎說結束了，要標出來', async () => {
    // 本地狀態是投影，可能落後於 Temporal。
    // 這種不一致是有用的訊號——藏起來就查不出對帳問題。
    mockList([instance()])
    mockDetail({ ...instance(), status: 'RUNNING', live_status: 'COMPLETED' })
    const w = await render()

    await w.find('[data-testid="monitor-open-QT-001"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="monitor-status-mismatch"]').exists()).toBe(
      true,
    )
  })

  it('取消的單不跳不一致警告', async () => {
    // 流程自己處理完取消後是正常返回的，Temporal 看到的是
    // 「執行完成」，本地記的是業務結果「已取消」——兩者都對。
    //
    // 直接比字串的話每一筆取消的單都會跳警告，
    // 而**會叫錯的警告比沒有警告更糟**：真的出問題時沒人會看。
    mockList([instance({ status: 'CANCELLED' })])
    mockDetail({
      ...instance({ status: 'CANCELLED' }),
      live_status: 'COMPLETED',
    })
    const w = await render()

    await w.find('[data-testid="monitor-open-QT-001"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="monitor-status-mismatch"]').exists()).toBe(
      false,
    )
  })

  it('本地說結束了、引擎說還在跑，也要標出來', async () => {
    // 反方向同樣是對帳問題
    mockList([instance({ status: 'COMPLETED' })])
    mockDetail({
      ...instance({ status: 'COMPLETED' }),
      live_status: 'RUNNING',
    })
    const w = await render()

    await w.find('[data-testid="monitor-open-QT-001"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="monitor-status-mismatch"]').exists()).toBe(
      true,
    )
  })

  it('引擎查不到即時狀態時明說，不留空白', async () => {
    // 查不到不是錯誤（可能超過保留期），但要說出來
    mockList([instance()])
    mockDetail({ ...instance(), live_status: null })
    const w = await render()

    await w.find('[data-testid="monitor-open-QT-001"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="monitor-detail-live"]').text()).toContain(
      '查不到',
    )
    expect(w.find('[data-testid="monitor-status-mismatch"]').exists()).toBe(
      false,
    )
  })

  it('看不到的單回 404 時顯示訊息，不顯示空白詳情', async () => {
    // 後端對看不到的單回 404（403 等於告訴對方這張單存在）。
    // 前端要照實呈現，不可顯示一個空的詳情框讓人以為單是空的。
    mockList([instance()])
    server.use(
      http.get(`${API}/instances/in-1`, () =>
        HttpResponse.json(
          { code: 'NOT_FOUND', message: '找不到這筆流程實例' },
          { status: 404 },
        ),
      ),
    )
    const w = await render()

    await w.find('[data-testid="monitor-open-QT-001"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="monitor-detail-error"]').text()).toContain(
      '找不到',
    )
    expect(w.find('[data-testid="monitor-detail-status"]').exists()).toBe(false)
  })

  it('失敗的流程顯示錯誤原因', async () => {
    mockList([instance({ status: 'FAILED' })])
    mockDetail({
      ...instance({ status: 'FAILED' }),
      live_status: 'FAILED',
      error: 'odoo.create_sale_order 重試 5 次後失敗',
    })
    const w = await render()

    await w.find('[data-testid="monitor-open-QT-001"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="monitor-detail-failure"]').text()).toContain(
      'odoo.create_sale_order',
    )
  })
})
