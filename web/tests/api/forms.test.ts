/**
 * 表單定義 API client 測試
 *
 * 用 MSW 攔截真實 fetch，驗證四件事：
 *   請求格式、回應解析、錯誤處理、認證標頭。
 *
 * 回應結構刻意與後端 server/crates/http-public/src/forms.rs 一致。
 * 後端改欄位時這裡會失敗，這正是要的效果。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { server } from '../msw/server'
import { setTokenGetter } from '@/api/client'
import * as forms from '@/api/forms'
import { ApiError } from '@/api/types'
import type { FormContent } from '@/api/types'

const API = '*/api'

function sampleContent(key = 'quotation_form'): FormContent {
  return {
    form_key: key,
    version: 1,
    business_object: 'quotation',
    fields: [
      {
        key: 'amount',
        ui: { component: 'number', label: '金額', precision: 2 },
        data: { path: 'quotation.amount', type: 'decimal' },
      },
    ],
  }
}

function sampleVersion(overrides: Record<string, unknown> = {}) {
  return {
    id: 'ver-1',
    form_id: 'form-1',
    version: null,
    content: sampleContent(),
    status: 'DRAFT',
    created_by: 'user-1',
    created_at: '2026-09-14T08:00:00Z',
    updated_at: '2026-09-14T08:00:00Z',
    published_by: null,
    published_at: null,
    ...overrides,
  }
}

// ── 請求格式 ────────────────────────────────────────────

describe('請求格式', () => {
  it('listForms 送出 GET /forms', async () => {
    const spy = vi.fn()
    server.use(
      http.get(`${API}/forms`, ({ request }) => {
        spy({ method: request.method, url: new URL(request.url).pathname })
        return HttpResponse.json([])
      }),
    )

    await forms.listForms()
    expect(spy).toHaveBeenCalledWith({ method: 'GET', url: '/api/forms' })
  })

  it('createForm 送出完整 body', async () => {
    let captured: unknown
    server.use(
      http.post(`${API}/forms`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({ id: 'f1' }, { status: 201 })
      }),
    )

    const content = sampleContent()
    await forms.createForm({
      form_key: 'quotation_form',
      business_object: 'quotation',
      name: '報價單',
      content,
    })

    expect(captured).toEqual({
      form_key: 'quotation_form',
      business_object: 'quotation',
      name: '報價單',
      content,
    })
  })

  it('saveDraft 把 content 包在 body 物件內', async () => {
    let captured: unknown
    server.use(
      http.put(`${API}/forms/:key/draft`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json(sampleVersion())
      }),
    )

    const content = sampleContent()
    await forms.saveDraft('quotation_form', content)

    // 後端 SaveDraftBody 期望 { content }，不是直接送 content
    expect(captured).toEqual({ content })
  })

  it('form_key 含特殊字元時正確編碼', async () => {
    let path = ''
    server.use(
      http.get(`${API}/forms/:key`, ({ request }) => {
        path = new URL(request.url).pathname
        return HttpResponse.json({})
      }),
    )

    await forms.getForm('form/with slash')
    expect(path).toBe('/api/forms/form%2Fwith%20slash')
  })

  it('DELETE 不帶 Content-Type', async () => {
    let hasContentType = true
    server.use(
      http.delete(`${API}/forms/:key/draft`, ({ request }) => {
        hasContentType = request.headers.has('content-type')
        return new HttpResponse(null, { status: 204 })
      }),
    )

    await forms.discardDraft('quotation_form')
    expect(hasContentType, '無 body 的請求不該宣告 Content-Type').toBe(false)
  })
})

// ── 回應解析 ────────────────────────────────────────────

describe('回應解析', () => {
  it('草稿的 version 為 null', async () => {
    server.use(
      http.get(`${API}/forms/:key`, () =>
        HttpResponse.json({
          id: 'form-1',
          form_key: 'quotation_form',
          business_object: 'quotation',
          name: '報價單',
          created_at: '2026-09-14T08:00:00Z',
          updated_at: '2026-09-14T08:00:00Z',
          draft: sampleVersion(),
          published: null,
        }),
      ),
    )

    const detail = await forms.getForm('quotation_form')
    expect(detail.draft?.version).toBeNull()
    expect(detail.draft?.status).toBe('DRAFT')
    expect(detail.published).toBeNull()
  })

  it('已發布版本帶版本號與發布時間', async () => {
    server.use(
      http.post(`${API}/forms/:key/draft/publish`, () =>
        HttpResponse.json(
          sampleVersion({
            version: 3,
            status: 'PUBLISHED',
            published_by: 'user-1',
            published_at: '2026-09-14T09:00:00Z',
          }),
        ),
      ),
    )

    const v = await forms.publishDraft('quotation_form')
    expect(v.version).toBe(3)
    expect(v.status).toBe('PUBLISHED')
    expect(v.published_at).toBe('2026-09-14T09:00:00Z')
  })

  it('列表保留 published_version 與 has_draft', async () => {
    server.use(
      http.get(`${API}/forms`, () =>
        HttpResponse.json([
          {
            id: 'form-1',
            form_key: 'quotation_form',
            business_object: 'quotation',
            name: '報價單',
            created_at: '2026-09-14T08:00:00Z',
            updated_at: '2026-09-14T08:00:00Z',
            published_version: 2,
            has_draft: true,
          },
        ]),
      ),
    )

    const list = await forms.listForms()
    expect(list[0].published_version).toBe(2)
    expect(list[0].has_draft).toBe(true)
  })

  it('DELETE 回 204 時不嘗試解析 body', async () => {
    server.use(
      http.delete(`${API}/forms/:key/draft`, () => new HttpResponse(null, { status: 204 })),
    )

    await expect(forms.discardDraft('quotation_form')).resolves.toBeUndefined()
  })

  it('驗證失敗仍回 200，結果在 body', async () => {
    server.use(
      http.post(`${API}/forms/:key/draft/validate`, () =>
        HttpResponse.json({
          valid: false,
          errors: ['欄位 key 重複：amount', '表格欄位 lines 未定義 columns'],
        }),
      ),
    )

    const result = await forms.validateDraft('quotation_form')
    expect(result.valid).toBe(false)
    expect(result.errors).toHaveLength(2)
  })
})

// ── 錯誤處理 ────────────────────────────────────────────

describe('錯誤處理', () => {
  it('404 轉成 ApiError 並帶 NOT_FOUND', async () => {
    server.use(
      http.get(`${API}/forms/:key`, () =>
        HttpResponse.json(
          { code: 'NOT_FOUND', message: '找不到 form：nonexistent' },
          { status: 404 },
        ),
      ),
    )

    const err = await forms.getForm('nonexistent').catch((e) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err.code).toBe('NOT_FOUND')
    expect(err.status).toBe(404)
  })

  it('403 帶 FORBIDDEN', async () => {
    server.use(
      http.put(`${API}/forms/:key/draft`, () =>
        HttpResponse.json({ code: 'FORBIDDEN', message: '需要 designer 角色' }, { status: 403 }),
      ),
    )

    const err = await forms.saveDraft('quotation_form', sampleContent()).catch((e) => e)
    expect(err.code).toBe('FORBIDDEN')
  })

  it('422 驗證失敗附上逐條錯誤', async () => {
    server.use(
      http.post(`${API}/forms/:key/draft/publish`, () =>
        HttpResponse.json(
          {
            code: 'VALIDATION_FAILED',
            message: '欄位 amount 與 amount_copy 綁定同一資料路徑',
            details: { errors: ['欄位 amount 與 amount_copy 綁定同一資料路徑'] },
          },
          { status: 422 },
        ),
      ),
    )

    const err = await forms.publishDraft('quotation_form').catch((e) => e)
    expect(err.code).toBe('VALIDATION_FAILED')
    expect(err.validationErrors).toHaveLength(1)
    expect(err.validationErrors[0]).toContain('同一資料路徑')
  })

  it('409 衝突', async () => {
    server.use(
      http.post(`${API}/forms/:key/draft/publish`, () =>
        HttpResponse.json({ code: 'CONFLICT', message: '沒有可發布的草稿' }, { status: 409 }),
      ),
    )

    const err = await forms.publishDraft('quotation_form').catch((e) => e)
    expect(err.code).toBe('CONFLICT')
    expect(err.retryable, '衝突不該重試').toBe(false)
  })

  it('500 標記為可重試', async () => {
    server.use(
      http.get(`${API}/forms`, () =>
        HttpResponse.json({ code: 'INTERNAL_ERROR', message: '伺服器內部錯誤' }, { status: 500 }),
      ),
    )

    const err = await forms.listForms().catch((e) => e)
    expect(err.retryable).toBe(true)
  })

  it('非 JSON 錯誤回應也轉成 ApiError', async () => {
    // 反向代理可能回 HTML，前端不該因此崩潰
    server.use(
      http.get(`${API}/forms`, () =>
        new HttpResponse('<html><body>502 Bad Gateway</body></html>', {
          status: 502,
          headers: { 'Content-Type': 'text/html' },
        }),
      ),
    )

    const err = await forms.listForms().catch((e) => e)
    expect(err).toBeInstanceOf(ApiError)
    expect(err.code).toBe('INTERNAL_ERROR')
    expect(err.status).toBe(502)
  })
})

// ── 認證標頭 ────────────────────────────────────────────

describe('認證標頭', () => {
  it('一般請求帶 Bearer token', async () => {
    let auth: string | null = null
    server.use(
      http.get(`${API}/forms`, ({ request }) => {
        auth = request.headers.get('authorization')
        return HttpResponse.json([])
      }),
    )

    await forms.listForms()
    expect(auth).toBe('Bearer test-token')
  })

  it('沒有 token 時不帶 Authorization 標頭', async () => {
    setTokenGetter(() => null)
    let hasAuth = true
    server.use(
      http.get(`${API}/forms`, ({ request }) => {
        hasAuth = request.headers.has('authorization')
        return HttpResponse.json([])
      }),
    )

    await forms.listForms()
    expect(hasAuth, '無 token 時不該送空的 Authorization').toBe(false)
  })

  it('token 變更後續請求使用新值', async () => {
    const seen: string[] = []
    server.use(
      http.get(`${API}/forms`, ({ request }) => {
        seen.push(request.headers.get('authorization') ?? '')
        return HttpResponse.json([])
      }),
    )

    await forms.listForms()
    setTokenGetter(() => 'new-token')
    await forms.listForms()

    expect(seen).toEqual(['Bearer test-token', 'Bearer new-token'])
  })
})
