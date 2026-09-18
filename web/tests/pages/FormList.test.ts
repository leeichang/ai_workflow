/**
 * 表單清單與新增測試
 *
 * 這個畫面補的是一個實際存在的缺口：後端有 POST /forms，
 * 前端的 createForm() 也寫好了，但**沒有任何畫面呼叫它**——
 * 設計器只能開既有表單（路由寫死 :formKey），
 * /designer 的 redirect 直接指向 quotation_form。
 *
 * 換句話說：使用者無法從零建立一張表單。
 * 這正是產品定位「讓使用者自己是 Owner」最核心的一步。
 *
 * 重點在建立時的輸入驗證：form_key 會變成 API 路徑與資料表關聯，
 * 事後不能改，所以要在送出前就擋下不合法的值。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { server } from '../msw/server'
import FormList from '@/pages/FormList.vue'
import { useSession } from '@/auth/useSession'

const API = '*/api'

const push = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ path: '/designer/forms', query: {} }),
  RouterLink: { template: '<a><slot /></a>' },
}))

function form(over: Record<string, unknown> = {}) {
  return {
    id: 'f1',
    tenant_id: 't1',
    form_key: 'quotation_form',
    business_object: 'quotation',
    name: '報價單',
    published_version: 1,
    has_draft: false,
    created_at: '2026-09-01T00:00:00Z',
    updated_at: '2026-09-01T00:00:00Z',
    ...over,
  }
}

function mockList(items: unknown[]) {
  server.use(http.get(`${API}/forms`, () => HttpResponse.json(items)))
}

async function render() {
  const w = mount(FormList)
  await flushPromises()
  return w
}

describe('表單清單', () => {
  beforeEach(() => {
    push.mockClear()
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
  })

  it('列出既有表單', async () => {
    mockList([form(), form({ id: 'f2', form_key: 'pr_form', name: '採購申請' })])
    const w = await render()

    expect(w.find('[data-testid="form-row-quotation_form"]').exists()).toBe(true)
    expect(w.find('[data-testid="form-row-pr_form"]').exists()).toBe(true)
  })

  it('沒有表單時顯示空狀態與新增指引', async () => {
    mockList([])
    const w = await render()

    // 空狀態不能只說「沒有資料」——使用者第一次進來看到的就是這個畫面，
    // 要告訴他下一步做什麼
    expect(w.find('[data-testid="form-empty"]').exists()).toBe(true)
    expect(w.find('[data-testid="form-create"]').exists()).toBe(true)
  })

  it('顯示草稿狀態', async () => {
    mockList([form({ has_draft: true, published_version: 2 })])
    const w = await render()

    const row = w.find('[data-testid="form-row-quotation_form"]')
    expect(row.text()).toContain('草稿')
  })

  it('未發布的表單標示為尚未發布', async () => {
    mockList([form({ published_version: null, has_draft: true })])
    const w = await render()

    expect(w.find('[data-testid="form-row-quotation_form"]').text()).toContain(
      '尚未發布',
    )
  })
})

describe('新增表單', () => {
  beforeEach(() => {
    push.mockClear()
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
    mockList([])
  })

  it('按新增開啟對話框', async () => {
    const w = await render()
    await w.find('[data-testid="form-create"]').trigger('click')

    expect(w.find('[data-testid="create-dialog"]').exists()).toBe(true)
  })

  it('送出後呼叫 API 並導向設計器', async () => {
    let captured: unknown
    server.use(
      http.post(`${API}/forms`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({ definition: form() }, { status: 201 })
      }),
    )

    const w = await render()
    await w.find('[data-testid="form-create"]').trigger('click')

    await w.find('[data-testid="new-form-key"]').setValue('employee_form')
    await w.find('[data-testid="new-form-name"]').setValue('員工基本資料')
    await w.find('[data-testid="new-business-object"]').setValue('employee')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(captured).toMatchObject({
      form_key: 'employee_form',
      business_object: 'employee',
      name: '員工基本資料',
    })

    // schemas/form.schema.json 對 fields 設了 minItems: 1，
    // 送零欄位會被擋（「/fields：[] has less than 1 item」）。
    // 新表單必須帶一個起始欄位。
    const content = (captured as {
      content: {
        sections: unknown[]
        fields: { data: { path: string } }[]
      }
    }).content
    expect(content.sections.length).toBeGreaterThan(0)
    expect(content.fields.length).toBeGreaterThan(0)

    // 起始欄位指向業務欄位，不是 workflow_instance 的 business_key。
    // 用 business_key 會建立一條永遠不在
    // schemas/business-objects/*.json 裡的路徑。
    expect(content.fields[0].data.path).toBe('employee.number')

    // 建完直接進設計器，不要讓使用者自己再點一次
    expect(push).toHaveBeenCalledWith('/designer/forms/employee_form')
  })

  it('form_key 空白時擋下並提示', async () => {
    const w = await render()
    await w.find('[data-testid="form-create"]').trigger('click')
    await w.find('[data-testid="new-form-name"]').setValue('員工基本資料')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').text()).toContain('代碼')
  })

  it('form_key 含大寫或連字號時擋下', async () => {
    const w = await render()
    await w.find('[data-testid="form-create"]').trigger('click')

    // form_key 會進 API 路徑與資料表關聯，事後不能改。
    // 允許大小寫混用會讓 URL 難以預測，連字號則與既有慣例不一致。
    await w.find('[data-testid="new-form-key"]').setValue('Employee-Form')
    await w.find('[data-testid="new-form-name"]').setValue('員工資料')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').exists()).toBe(true)
  })

  it('重複的 form_key 顯示後端的衝突訊息', async () => {
    server.use(
      // client.ts 要 code 與 message 兩個欄位才會當成結構化錯誤，
      // 少一個就降級成「伺服器回應異常（HTTP 409）」
      http.post(`${API}/forms`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '表單代碼已存在' },
          { status: 409 },
        ),
      ),
    )

    const w = await render()
    await w.find('[data-testid="form-create"]').trigger('click')
    await w.find('[data-testid="new-form-key"]').setValue('quotation_form')
    await w.find('[data-testid="new-form-name"]').setValue('報價單')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="create-error"]').text()).toContain('已存在')
    // 失敗時不該導頁，使用者要能改了再送
    expect(push).not.toHaveBeenCalled()
  })

  it('業務物件留空時沿用 form_key', async () => {
    let captured: unknown
    server.use(
      http.post(`${API}/forms`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({ definition: form() }, { status: 201 })
      }),
    )

    const w = await render()
    await w.find('[data-testid="form-create"]').trigger('click')
    await w.find('[data-testid="new-form-key"]').setValue('leave_request')
    await w.find('[data-testid="new-form-name"]').setValue('請假單')
    await w.find('[data-testid="create-confirm"]').trigger('click')
    await flushPromises()

    expect((captured as { business_object: string }).business_object).toBe(
      'leave_request',
    )
  })
})
