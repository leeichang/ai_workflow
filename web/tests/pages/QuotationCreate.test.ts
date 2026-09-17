/**
 * 建立報價單測試
 *
 * 在此之前只能用 curl 打 API 建立，畫面上沒有任何入口。
 *
 * 表單由表單設計器的定義驅動渲染，而非寫死欄位——
 * 設計器改了欄位，這裡要跟著變，否則兩邊會各說各話。
 *
 * 送出時依 data.path 組出巢狀的業務物件。這一點不能錯：
 * 流程定義的條件式讀 `quotation.discount_rate`，
 * 客戶簽核的 resolver 讀 `quotation.customer_contact_ids`，
 * 放錯層級的話條件式會靜默當成 false，resolver 則讓流程 FAILED。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { server } from '../msw/server'
import QuotationCreate from '@/pages/QuotationCreate.vue'
import { useSession } from '@/auth/useSession'

const API = '*/api'

const push = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ path: '/quotations/new', query: {} }),
  RouterLink: { props: ['to'], template: '<a><slot /></a>' },
}))

/** 精簡版表單定義，欄位形狀與真實定義一致 */
const FORM_DEF = {
  id: 'f1',
  form_key: 'quotation_form',
  business_object: 'quotation',
  name: '報價單',
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-01T00:00:00Z',
  draft: null,
  published: {
    version: 1,
    content: {
      form_key: 'quotation_form',
      business_object: 'quotation',
      name: '報價單',
      sections: [
        { key: 'customer', title: '客戶資訊' },
        { key: 'terms', title: '報價條件與折扣' },
      ],
      fields: [
        {
          key: 'customer_name',
          section: 'customer',
          data: { path: 'customer.name', type: 'string' },
          ui: { component: 'input', label: '客戶名稱', width: 6 },
          workflow: { required_when: 'true' },
        },
        {
          key: 'tax_id',
          section: 'customer',
          data: { path: 'customer.tax_id', type: 'string', pattern: '^[0-9]{8}$' },
          ui: { component: 'input', label: '統一編號', width: 6 },
        },
        {
          key: 'discount_rate',
          section: 'terms',
          // 真實表單定義用的是 decimal，不是 number
          data: { path: 'quotation.discount_rate', type: 'decimal', min: 0, max: 0.5 },
          ui: { component: 'number', label: '折扣率', width: 4 },
        },
        {
          key: 'internal_notes',
          section: 'terms',
          data: { path: 'quotation.internal_notes', type: 'string' },
          ui: { component: 'textarea', label: '內部備註', width: 12 },
        },
      ],
    },
  },
}

function mockForm(def: unknown = FORM_DEF) {
  server.use(http.get(`${API}/forms/quotation_form`, () => HttpResponse.json(def)))
}

async function render() {
  const w = mount(QuotationCreate)
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
        name: '王景榮',
        email: 'sales@demo.local',
        tenant_id: 't1',
        roles: ['requester'],
      },
    }),
  )
  useSession().restore()
  push.mockClear()
})

describe('依表單定義渲染', () => {
  it('列出定義中的欄位', async () => {
    mockForm()
    const w = await render()

    expect(w.find('[data-testid="field-customer_name"]').exists()).toBe(true)
    expect(w.find('[data-testid="field-tax_id"]').exists()).toBe(true)
    expect(w.find('[data-testid="field-discount_rate"]').exists()).toBe(true)
  })

  it('用定義裡的標籤，而非欄位代碼', async () => {
    mockForm()
    const w = await render()
    expect(w.text()).toContain('客戶名稱')
    expect(w.text()).toContain('統一編號')
  })

  it('依 section 分組顯示', async () => {
    mockForm()
    const w = await render()
    expect(w.text()).toContain('客戶資訊')
    expect(w.text()).toContain('報價條件與折扣')
  })

  it('textarea 元件渲染成多行輸入', async () => {
    mockForm()
    const w = await render()
    expect(w.find('[data-testid="field-internal_notes"]').element.tagName).toBe(
      'TEXTAREA',
    )
  })

  it('number 元件渲染成數字輸入', async () => {
    mockForm()
    const w = await render()
    expect(
      w.find('[data-testid="field-discount_rate"]').attributes('type'),
    ).toBe('number')
  })

  it('載入失敗顯示錯誤而非空白表單', async () => {
    server.use(
      http.get(`${API}/forms/quotation_form`, () =>
        HttpResponse.json(
          { code: 'NOT_FOUND', message: '找不到表單定義' },
          { status: 404 },
        ),
      ),
    )
    const w = await render()
    expect(w.find('[data-testid="create-error"]').exists()).toBe(true)
    expect(w.find('[data-testid="create-submit"]').exists()).toBe(false)
  })
})

describe('送出', () => {
  async function fill(w: Awaited<ReturnType<typeof render>>) {
    await w.find('[data-testid="field-business_key"]').setValue('QT-2026-0001')
    await w.find('[data-testid="field-customer_name"]').setValue('台灣松下')
    await w.find('[data-testid="field-tax_id"]').setValue('11093847')
    await w.find('[data-testid="field-discount_rate"]').setValue('0.2')
    await w.find('[data-testid="field-customer_contacts"]').setValue(
      'cust@example.com',
    )
  }

  it('依 data.path 組出巢狀業務物件', async () => {
    mockForm()
    const w = await render()

    let body: Record<string, unknown> = {}
    server.use(
      http.post(`${API}/instances`, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ id: 'i1', business_key: 'QT-2026-0001' }, { status: 201 })
      }),
    )

    await fill(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(body.workflow_key).toBe('quotation_approval')
    expect(body.business_key).toBe('QT-2026-0001')

    const input = body.input as Record<string, Record<string, unknown>>
    // customer.* 與 quotation.* 必須分別巢狀，不能全部攤平在頂層
    expect(input.customer.name).toBe('台灣松下')
    expect(input.customer.tax_id).toBe('11093847')
    expect(input.quotation.discount_rate).toBe(0.2)
  })

  it('number 型別送出數字而非字串', async () => {
    mockForm()
    const w = await render()

    let body: Record<string, unknown> = {}
    server.use(
      http.post(`${API}/instances`, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await fill(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    const input = body.input as Record<string, Record<string, unknown>>
    // 條件式是 `quotation.discount_rate > 0.15`，
    // 送字串的話比較結果不可預期
    expect(typeof input.quotation.discount_rate).toBe('number')
  })

  it('客戶聯絡人送成陣列，流程的 resolver 需要', async () => {
    mockForm()
    const w = await render()

    let body: Record<string, unknown> = {}
    server.use(
      http.post(`${API}/instances`, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await fill(w)
    await w
      .find('[data-testid="field-customer_contacts"]')
      .setValue('a@x.com, b@y.com')
    await w.find('form').trigger('submit')
    await flushPromises()

    const input = body.input as Record<string, Record<string, unknown>>
    expect(input.quotation.customer_contact_ids).toEqual(['a@x.com', 'b@y.com'])
  })

  it('成功後導向報價單列表', async () => {
    mockForm()
    const w = await render()
    server.use(
      http.post(`${API}/instances`, () =>
        HttpResponse.json({ id: 'i1' }, { status: 201 }),
      ),
    )

    await fill(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(push).toHaveBeenCalledWith('/quotations')
  })
})

describe('驗證', () => {
  it('單號未填時不送出', async () => {
    mockForm()
    const w = await render()

    let called = false
    server.use(
      http.post(`${API}/instances`, () => {
        called = true
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await w.find('[data-testid="field-customer_name"]').setValue('台灣松下')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(called).toBe(false)
    expect(w.find('[data-testid="create-error"]').text()).toContain('單號')
  })

  it('required_when 為 true 的欄位未填時不送出', async () => {
    mockForm()
    const w = await render()

    let called = false
    server.use(
      http.post(`${API}/instances`, () => {
        called = true
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await w.find('[data-testid="field-business_key"]').setValue('QT-1')
    // customer_name 的 required_when 是 'true'，沒填
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(called).toBe(false)
    expect(w.find('[data-testid="create-error"]').text()).toContain('客戶名稱')
  })

  it('客戶聯絡人未填時不送出，否則流程會在客戶簽核失敗', async () => {
    mockForm()
    const w = await render()

    let called = false
    server.use(
      http.post(`${API}/instances`, () => {
        called = true
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await w.find('[data-testid="field-business_key"]').setValue('QT-1')
    await w.find('[data-testid="field-customer_name"]').setValue('台灣松下')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(called).toBe(false)
    expect(w.find('[data-testid="create-error"]').text()).toContain('聯絡人')
  })

  it('pattern 不符時不送出', async () => {
    mockForm()
    const w = await render()

    let called = false
    server.use(
      http.post(`${API}/instances`, () => {
        called = true
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await w.find('[data-testid="field-business_key"]').setValue('QT-1')
    await w.find('[data-testid="field-customer_name"]').setValue('台灣松下')
    await w.find('[data-testid="field-customer_contacts"]').setValue('a@x.com')
    // 統編規則是 8 位數字
    await w.find('[data-testid="field-tax_id"]').setValue('123')
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(called).toBe(false)
    expect(w.find('[data-testid="create-error"]').text()).toContain('統一編號')
  })
})

describe('錯誤處理', () => {
  async function fillValid(w: Awaited<ReturnType<typeof render>>) {
    await w.find('[data-testid="field-business_key"]').setValue('QT-1')
    await w.find('[data-testid="field-customer_name"]').setValue('台灣松下')
    await w.find('[data-testid="field-customer_contacts"]').setValue('a@x.com')
  }

  it('流程引擎斷線時顯示 503 訊息', async () => {
    mockForm()
    const w = await render()
    server.use(
      http.post(`${API}/instances`, () =>
        HttpResponse.json(
          { code: 'SERVICE_UNAVAILABLE', message: '流程引擎未連線' },
          { status: 503 },
        ),
      ),
    )

    await fillValid(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').text()).toContain('流程引擎')
    expect(push).not.toHaveBeenCalled()
  })

  it('單號重複時顯示訊息', async () => {
    mockForm()
    const w = await render()
    server.use(
      http.post(`${API}/instances`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '單號已存在' },
          { status: 409 },
        ),
      ),
    )

    await fillValid(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').text()).toContain('單號已存在')
  })

  it('送出期間按鈕停用，避免建立兩張', async () => {
    mockForm()
    const w = await render()

    let resolve: (() => void) | null = null
    const gate = new Promise<void>((r) => {
      resolve = r
    })
    server.use(
      http.post(`${API}/instances`, async () => {
        await gate
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await fillValid(w)
    void w.find('form').trigger('submit')
    await w.vm.$nextTick()

    expect(
      (w.find('[data-testid="create-submit"]').element as HTMLButtonElement)
        .disabled,
    ).toBe(true)

    resolve?.()
  })
})
