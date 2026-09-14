/**
 * 設計器狀態測試
 *
 * 重點是 undo / redo 與欄位操作的正確性。
 * 伺服器往返用 MSW 攔截。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'
import { server } from '../msw/server'
import { useDesigner } from '@/designer/useDesigner'
import type { FormContent } from '@/api/types'

const API = '*/api'

function content(): FormContent {
  return {
    form_key: 'quotation_form',
    version: 1,
    business_object: 'quotation',
    name: '報價單',
    fields: [
      {
        key: 'customer_name',
        ui: { component: 'input', label: '客戶名稱', width: 6 },
        data: { path: 'customer.name', type: 'string' },
      },
      {
        key: 'discount_rate',
        ui: { component: 'number', label: '折扣率', width: 4, precision: 4 },
        data: { path: 'quotation.discount_rate', type: 'decimal' },
        workflow: { readonly_when: "node.id != 'start'" },
      },
    ],
  }
}

function mockGetForm(c: FormContent = content()) {
  server.use(
    http.get(`${API}/forms/:key`, () =>
      HttpResponse.json({
        id: 'form-1',
        form_key: c.form_key,
        business_object: c.business_object,
        name: c.name,
        created_at: '2026-09-14T00:00:00Z',
        updated_at: '2026-09-14T00:00:00Z',
        draft: {
          id: 'v-draft',
          form_id: 'form-1',
          version: null,
          content: c,
          status: 'DRAFT',
          created_by: null,
          created_at: '2026-09-14T00:00:00Z',
          updated_at: '2026-09-14T00:00:00Z',
          published_by: null,
          published_at: null,
        },
        published: null,
      }),
    ),
  )
}

describe('載入', () => {
  it('有草稿時用草稿當編輯起點', async () => {
    mockGetForm()
    const d = useDesigner('quotation_form')
    await d.load()

    expect(d.content.value?.fields).toHaveLength(2)
    expect(d.error.value).toBeNull()
    expect(d.dirty.value).toBe(false)
  })

  it('沒有任何版本時顯示錯誤', async () => {
    server.use(
      http.get(`${API}/forms/:key`, () =>
        HttpResponse.json({
          id: 'f', form_key: 'k', business_object: 'b', name: 'n',
          created_at: '', updated_at: '', draft: null, published: null,
        }),
      ),
    )
    const d = useDesigner('empty_form')
    await d.load()

    expect(d.content.value).toBeNull()
    expect(d.error.value).toContain('尚無任何版本')
  })

  it('沒有草稿時用已發布版本', async () => {
    const c = content()
    server.use(
      http.get(`${API}/forms/:key`, () =>
        HttpResponse.json({
          id: 'f', form_key: 'k', business_object: 'quotation', name: 'n',
          created_at: '', updated_at: '',
          draft: null,
          published: {
            id: 'v1', form_id: 'f', version: 2, content: c, status: 'PUBLISHED',
            created_by: null, created_at: '', updated_at: '',
            published_by: null, published_at: '2026-09-14T00:00:00Z',
          },
        }),
      ),
    )
    const d = useDesigner('published_only')
    await d.load()

    expect(d.content.value?.fields).toHaveLength(2)
    expect(d.publishedVersion.value).toBe(2)
  })
})

describe('欄位操作', () => {
  let d: ReturnType<typeof useDesigner>

  beforeEach(async () => {
    mockGetForm()
    d = useDesigner('quotation_form')
    await d.load()
  })

  it('新增欄位並自動選取', () => {
    d.addField({
      key: 'new_field',
      ui: { component: 'date', label: '日期' },
      data: { path: '', type: 'date' },
    })

    expect(d.content.value?.fields).toHaveLength(3)
    expect(d.selectedKey.value).toBe('new_field')
    expect(d.dirty.value).toBe(true)
  })

  it('新增於指定欄位之後', () => {
    d.addField(
      { key: 'inserted', ui: { component: 'input', label: '插入' }, data: { path: '', type: 'string' } },
      'customer_name',
    )
    expect(d.content.value?.fields[1].key).toBe('inserted')
  })

  it('更新欄位時深層合併，不洗掉其他屬性', () => {
    d.updateField('discount_rate', { ui: { component: 'number', label: '新標籤' } })

    const f = d.content.value!.fields.find((x) => x.key === 'discount_rate')!
    expect(f.ui.label).toBe('新標籤')
    expect(f.ui.precision, 'precision 不該被洗掉').toBe(4)
    expect(f.workflow?.readonly_when, 'workflow 不該被洗掉').toBeTruthy()
  })

  it('複製欄位時同時改 key 與資料路徑', () => {
    d.duplicateField('discount_rate')

    const copy = d.content.value!.fields.find((f) => f.key === 'discount_rate_copy')!
    expect(copy).toBeDefined()
    // 兩欄位綁同一路徑會在發布時被後端擋下，所以複製時要一併改
    expect(copy.data.path).toBe('quotation.discount_rate_copy')
  })

  it('刪除欄位時清除選取', () => {
    d.selectedKey.value = 'customer_name'
    d.removeField('customer_name')

    expect(d.content.value?.fields).toHaveLength(1)
    expect(d.selectedKey.value).toBeNull()
  })

  it('移動欄位', () => {
    d.moveField('discount_rate', -1)
    expect(d.content.value?.fields.map((f) => f.key)).toEqual([
      'discount_rate',
      'customer_name',
    ])
  })

  it('移動超出範圍時不動作', () => {
    const before = d.content.value!.fields.map((f) => f.key)
    d.moveField('customer_name', -1)
    expect(d.content.value?.fields.map((f) => f.key)).toEqual(before)
  })
})

describe('復原與重做', () => {
  let d: ReturnType<typeof useDesigner>

  beforeEach(async () => {
    mockGetForm()
    d = useDesigner('quotation_form')
    await d.load()
  })

  it('載入後無法復原', () => {
    expect(d.canUndo.value).toBe(false)
    expect(d.canRedo.value).toBe(false)
  })

  it('修改後可復原', () => {
    d.updateField('customer_name', { ui: { component: 'input', label: '改過' } })
    expect(d.canUndo.value).toBe(true)

    d.undo()
    expect(d.content.value!.fields[0].ui.label).toBe('客戶名稱')
    expect(d.canRedo.value).toBe(true)
  })

  it('復原後可重做', () => {
    d.updateField('customer_name', { ui: { component: 'input', label: '改過' } })
    d.undo()
    d.redo()
    expect(d.content.value!.fields[0].ui.label).toBe('改過')
  })

  it('復原後再修改會清掉重做歷史', () => {
    d.updateField('customer_name', { ui: { component: 'input', label: 'A' } })
    d.undo()
    expect(d.canRedo.value).toBe(true)

    d.updateField('customer_name', { ui: { component: 'input', label: 'B' } })
    expect(d.canRedo.value, '新的修改應清掉 redo 分支').toBe(false)
  })

  it('連續多次修改可逐步復原', () => {
    d.updateField('customer_name', { ui: { component: 'input', label: 'A' } })
    d.updateField('customer_name', { ui: { component: 'input', label: 'B' } })
    d.updateField('customer_name', { ui: { component: 'input', label: 'C' } })

    d.undo()
    expect(d.content.value!.fields[0].ui.label).toBe('B')
    d.undo()
    expect(d.content.value!.fields[0].ui.label).toBe('A')
    d.undo()
    expect(d.content.value!.fields[0].ui.label).toBe('客戶名稱')
    expect(d.canUndo.value).toBe(false)
  })

  it('刪除也可復原', () => {
    d.removeField('discount_rate')
    expect(d.content.value?.fields).toHaveLength(1)

    d.undo()
    expect(d.content.value?.fields).toHaveLength(2)
  })
})

describe('儲存與發布', () => {
  let d: ReturnType<typeof useDesigner>

  beforeEach(async () => {
    mockGetForm()
    d = useDesigner('quotation_form')
    await d.load()
  })

  it('儲存後清除 dirty 並記錄時間', async () => {
    let body: unknown
    server.use(
      http.put(`${API}/forms/:key/draft`, async ({ request }) => {
        body = await request.json()
        return HttpResponse.json({ id: 'v', version: null, status: 'DRAFT' })
      }),
    )

    d.updateField('customer_name', { ui: { component: 'input', label: '改過' } })
    await d.save()

    expect(d.dirty.value).toBe(false)
    expect(d.lastSavedAt.value).toBeInstanceOf(Date)
    expect(body).toHaveProperty('content')
  })

  it('發布前先儲存未存的變更', async () => {
    const calls: string[] = []
    server.use(
      http.put(`${API}/forms/:key/draft`, () => {
        calls.push('save')
        return HttpResponse.json({ id: 'v', version: null, status: 'DRAFT' })
      }),
      http.post(`${API}/forms/:key/draft/publish`, () => {
        calls.push('publish')
        return HttpResponse.json({ id: 'v', version: 1, status: 'PUBLISHED' })
      }),
    )
    mockGetForm() // publish 後會重新載入

    d.updateField('customer_name', { ui: { component: 'input', label: '改過' } })
    await d.publish()

    expect(calls, '必須先存再發布，否則發布的是舊草稿').toEqual(['save', 'publish'])
  })

  it('發布失敗時保留驗證錯誤', async () => {
    server.use(
      http.post(`${API}/forms/:key/draft/publish`, () =>
        HttpResponse.json(
          {
            code: 'VALIDATION_FAILED',
            message: '欄位 a 與 b 綁定同一資料路徑',
            details: { errors: ['欄位 a 與 b 綁定同一資料路徑'] },
          },
          { status: 422 },
        ),
      ),
    )

    const ok = await d.publish()
    expect(ok).toBe(false)
    expect(d.validationErrors.value).toHaveLength(1)
  })

  it('驗證失敗時回傳錯誤清單', async () => {
    server.use(
      http.post(`${API}/forms/:key/draft/validate`, () =>
        HttpResponse.json({ valid: false, errors: ['表格欄位未定義 columns'] }),
      ),
    )

    const ok = await d.validate()
    expect(ok).toBe(false)
    expect(d.validationErrors.value).toEqual(['表格欄位未定義 columns'])
  })
})
