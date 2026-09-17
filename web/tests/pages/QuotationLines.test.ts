/**
 * 報價明細表格測試
 *
 * `lines` 的 required_when 是 true——沒有明細的報價單沒有意義。
 * 先前這個欄位被當成「子表格尚未實作」跳過，結果是：
 * 畫面上看不到它，validate() 也看不到它，於是可以送出一張
 * 沒有任何明細的報價單。
 *
 * 金額計算的公式來自表單定義的 data.computed：
 *   gross_margin = (subtotal - total_cost) / subtotal
 *   total_amount = subtotal + tax
 * subtotal / total_cost / tax 沒有定義在任何地方，本檔釘住前端的實作。
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

/** 與真實表單定義同形狀的 lines 欄位 */
const LINES_FIELD = {
  key: 'lines',
  section: 'lines',
  data: { path: 'quotation.lines', type: 'array', item: 'quotation_line' },
  ui: {
    component: 'table',
    label: '報價明細表格',
    width: 12,
    columns: [
      { key: 'part_no', label: '料號', component: 'input', width: '200px' },
      { key: 'qty', label: '數量', component: 'number', precision: 0, align: 'right', width: '90px' },
      { key: 'unit', label: '單位', component: 'input', width: '70px' },
      { key: 'material_cost', label: '單位材料費', component: 'number', precision: 2, align: 'right', width: '110px' },
      { key: 'process_cost', label: '單位加工費', component: 'number', precision: 2, align: 'right', width: '110px' },
      { key: 'target_price', label: '目標報價', component: 'number', precision: 2, align: 'right', width: '110px' },
    ],
  },
  workflow: { required_when: 'true' },
}

const FORM_DEF = {
  id: 'f1',
  form_key: 'quotation_form',
  business_object: 'quotation',
  name: '報價單',
  created_at: '',
  updated_at: '',
  draft: null,
  published: {
    version: 1,
    content: {
      form_key: 'quotation_form',
      business_object: 'quotation',
      name: '報價單',
      sections: [
        { key: 'customer', title: '客戶資訊' },
        { key: 'lines', title: '報價明細清單' },
        { key: 'amount', title: '金額與稅額' },
      ],
      fields: [
        {
          key: 'customer_name',
          section: 'customer',
          data: { path: 'customer.name', type: 'string' },
          ui: { component: 'input', label: '客戶名稱', width: 6 },
          workflow: { required_when: 'true' },
        },
        LINES_FIELD,
        {
          key: 'gross_margin',
          section: 'lines',
          data: {
            path: 'quotation.gross_margin',
            type: 'decimal',
            computed: '(subtotal - total_cost) / subtotal',
          },
          ui: { component: 'display', label: '預估毛利率 (%)', suffix: '%', precision: 2, width: 4 },
        },
        {
          key: 'total_amount',
          section: 'amount',
          data: {
            path: 'quotation.total_amount',
            type: 'decimal',
            computed: 'subtotal + tax',
          },
          ui: { component: 'display', label: '報價總計 (含稅)', precision: 2, width: 4 },
        },
      ],
    },
  },
}

function mockForm() {
  server.use(
    http.get(`${API}/forms/quotation_form`, () => HttpResponse.json(FORM_DEF)),
  )
}

async function render() {
  mockForm()
  const w = mount(QuotationCreate)
  await flushPromises()
  return w
}

/** 填一列明細 */
async function fillRow(
  w: Awaited<ReturnType<typeof render>>,
  row: number,
  vals: Record<string, string>,
) {
  for (const [col, v] of Object.entries(vals)) {
    await w.find(`[data-testid="line-${row}-${col}"]`).setValue(v)
  }
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

describe('渲染', () => {
  it('顯示明細表格，不再被跳過', async () => {
    const w = await render()
    expect(w.find('[data-testid="lines-table"]').exists()).toBe(true)
  })

  it('依 ui.columns 產生表頭', async () => {
    const w = await render()
    const headers = w.findAll('[data-testid="lines-table"] thead th')
    const texts = headers.map((h) => h.text())

    expect(texts).toContain('料號')
    expect(texts).toContain('數量')
    expect(texts).toContain('單位材料費')
  })

  it('預設有一列空白，不必先按新增', async () => {
    const w = await render()
    expect(w.findAll('[data-testid^="line-row-"]')).toHaveLength(1)
  })

  it('display 欄位顯示為唯讀計算結果，不是輸入框', async () => {
    const w = await render()
    const gm = w.find('[data-testid="computed-gross_margin"]')
    expect(gm.exists()).toBe(true)
    expect(gm.element.tagName).not.toBe('INPUT')
  })
})

describe('增刪列', () => {
  it('新增列', async () => {
    const w = await render()
    await w.find('[data-testid="lines-add"]').trigger('click')
    expect(w.findAll('[data-testid^="line-row-"]')).toHaveLength(2)
  })

  it('刪除列', async () => {
    const w = await render()
    await w.find('[data-testid="lines-add"]').trigger('click')
    await w.find('[data-testid="line-remove-0"]').trigger('click')
    expect(w.findAll('[data-testid^="line-row-"]')).toHaveLength(1)
  })

  it('只剩一列時不能刪到空，至少保留一列可填', async () => {
    const w = await render()
    expect(w.find('[data-testid="line-remove-0"]').exists()).toBe(false)
  })

  it('刪除的是指定那一列，不是最後一列', async () => {
    const w = await render()
    await w.find('[data-testid="lines-add"]').trigger('click')
    await fillRow(w, 0, { part_no: 'AAA' })
    await fillRow(w, 1, { part_no: 'BBB' })

    await w.find('[data-testid="line-remove-0"]').trigger('click')

    const remaining = w.find('[data-testid="line-0-part_no"]')
      .element as HTMLInputElement
    expect(remaining.value).toBe('BBB')
  })
})

describe('金額計算', () => {
  it('小計 = 各列 數量 × 目標報價', async () => {
    const w = await render()
    await fillRow(w, 0, { qty: '10', target_price: '100' })
    await w.find('[data-testid="lines-add"]').trigger('click')
    await fillRow(w, 1, { qty: '5', target_price: '200' })

    // 10*100 + 5*200 = 2000
    expect(w.find('[data-testid="computed-subtotal"]').text()).toContain('2,000')
  })

  it('稅額為小計的 5%', async () => {
    const w = await render()
    await fillRow(w, 0, { qty: '10', target_price: '100' })
    expect(w.find('[data-testid="computed-tax"]').text()).toContain('50')
  })

  it('總計 = 小計 + 稅額', async () => {
    const w = await render()
    await fillRow(w, 0, { qty: '10', target_price: '100' })
    // 1000 + 50
    expect(w.find('[data-testid="computed-total_amount"]').text()).toContain(
      '1,050',
    )
  })

  it('毛利率 = (小計 - 成本) / 小計', async () => {
    const w = await render()
    // 成本 = (材料 40 + 加工 20) × 10 = 600，小計 = 100 × 10 = 1000
    // 毛利率 = (1000-600)/1000 = 40%
    await fillRow(w, 0, {
      qty: '10',
      material_cost: '40',
      process_cost: '20',
      target_price: '100',
    })
    expect(w.find('[data-testid="computed-gross_margin"]').text()).toContain(
      '40',
    )
  })

  it('小計為 0 時毛利率不顯示 NaN', async () => {
    const w = await render()
    await fillRow(w, 0, { qty: '0', target_price: '0', material_cost: '10' })
    const text = w.find('[data-testid="computed-gross_margin"]').text()
    expect(text).not.toContain('NaN')
    expect(text).not.toContain('Infinity')
  })

  it('空白列不計入小計', async () => {
    const w = await render()
    await fillRow(w, 0, { qty: '10', target_price: '100' })
    await w.find('[data-testid="lines-add"]').trigger('click')
    // 第二列留空
    expect(w.find('[data-testid="computed-subtotal"]').text()).toContain('1,000')
  })
})

describe('送出', () => {
  async function fillValid(w: Awaited<ReturnType<typeof render>>) {
    await w.find('[data-testid="field-business_key"]').setValue('QT-1')
    await w.find('[data-testid="field-customer_name"]').setValue('台灣松下')
    await w.find('[data-testid="field-customer_contacts"]').setValue('a@x.com')
    await fillRow(w, 0, {
      part_no: 'SP-4402',
      qty: '10',
      unit: 'PCS',
      material_cost: '40',
      process_cost: '20',
      target_price: '100',
    })
  }

  it('明細送成陣列', async () => {
    const w = await render()
    let body: Record<string, unknown> = {}
    server.use(
      http.post(`${API}/instances`, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await fillValid(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    const input = body.input as Record<string, Record<string, unknown>>
    const lines = input.quotation.lines as Array<Record<string, unknown>>

    expect(Array.isArray(lines)).toBe(true)
    expect(lines).toHaveLength(1)
    expect(lines[0].part_no).toBe('SP-4402')
  })

  it('明細的數值欄位送數字而非字串', async () => {
    const w = await render()
    let body: Record<string, unknown> = {}
    server.use(
      http.post(`${API}/instances`, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await fillValid(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    const input = body.input as Record<string, Record<string, unknown>>
    const lines = input.quotation.lines as Array<Record<string, unknown>>

    expect(typeof lines[0].qty).toBe('number')
    expect(typeof lines[0].target_price).toBe('number')
  })

  it('計算結果一併送出，後端與 PDF 不必重算', async () => {
    const w = await render()
    let body: Record<string, unknown> = {}
    server.use(
      http.post(`${API}/instances`, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await fillValid(w)
    await w.find('form').trigger('submit')
    await flushPromises()

    const q = (body.input as Record<string, Record<string, unknown>>).quotation
    expect(q.subtotal).toBe(1000)
    expect(q.tax).toBe(50)
    expect(q.total_amount).toBe(1050)
    expect(q.gross_margin).toBeCloseTo(0.4, 5)
  })

  it('空白列不送出', async () => {
    const w = await render()
    let body: Record<string, unknown> = {}
    server.use(
      http.post(`${API}/instances`, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ id: 'i1' }, { status: 201 })
      }),
    )

    await fillValid(w)
    await w.find('[data-testid="lines-add"]').trigger('click')
    // 新增的列留空
    await w.find('form').trigger('submit')
    await flushPromises()

    const input = body.input as Record<string, Record<string, unknown>>
    expect(input.quotation.lines).toHaveLength(1)
  })
})

describe('驗證', () => {
  it('沒有任何明細時不送出', async () => {
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
    // 明細整列留空
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(called).toBe(false)
    expect(w.find('[data-testid="create-error"]').text()).toContain('明細')
  })

  it('明細只填料號沒填數量時不送出', async () => {
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
    await fillRow(w, 0, { part_no: 'SP-4402' })
    await w.find('form').trigger('submit')
    await flushPromises()

    expect(called).toBe(false)
    expect(w.find('[data-testid="create-error"]').text()).toContain('數量')
  })
})
