/**
 * 權限預覽測試
 *
 * 預覽會隨使用者切換角色連續發出請求，競態是這裡最容易出錯的地方：
 * 先發的慢請求若覆蓋後發的快請求，畫面會顯示上一個角色的結果，
 * 而且不會有任何錯誤。
 */

import { HttpResponse, http, delay } from 'msw'
import { describe, expect, it } from 'vitest'
import { server } from '../msw/server'
import { usePermissionPreview } from '@/designer/usePermissionPreview'

const API = '*/api'

function mockPreview(handler: (roles: string) => unknown) {
  server.use(
    http.get(`${API}/forms/:key/permissions/preview`, ({ request }) => {
      const roles = new URL(request.url).searchParams.get('roles') ?? ''
      return HttpResponse.json(handler(roles))
    }),
  )
}

describe('查詢', () => {
  it('把情境轉成查詢參數', async () => {
    let url = ''
    server.use(
      http.get(`${API}/forms/:key/permissions/preview`, ({ request }) => {
        url = request.url
        return HttpResponse.json([])
      }),
    )

    const p = usePermissionPreview('quotation_form')
    await p.run({ nodeId: 'mgr', roles: ['approver'], participantKind: 'internal' })

    expect(url).toContain('node_id=mgr')
    expect(url).toContain('roles=approver')
    expect(url).toContain('participant_kind=internal')
  })

  it('多個角色以逗號串接', async () => {
    let url = ''
    server.use(
      http.get(`${API}/forms/:key/permissions/preview`, ({ request }) => {
        url = request.url
        return HttpResponse.json([])
      }),
    )

    const p = usePermissionPreview('quotation_form')
    await p.run({ roles: ['approver', 'viewer'], participantKind: 'internal' })

    expect(decodeURIComponent(url)).toContain('roles=approver,viewer')
  })

  it('解析後端回傳的判定結果與理由', async () => {
    mockPreview(() => [
      { key: 'amount', permission: 'EDITABLE', required: true, reason: '預設可編輯' },
      { key: 'cost', permission: 'HIDDEN', required: false, reason: '外部參與者不可見' },
    ])

    const p = usePermissionPreview('quotation_form')
    await p.run({ roles: ['requester'], participantKind: 'external' })

    expect(p.result.value).toHaveLength(2)
    expect(p.result.value[1].reason).toBe('外部參與者不可見')
  })
})

describe('競態', () => {
  it('慢的舊請求不覆蓋快的新請求', async () => {
    // 使用者連點兩個角色，第一次的回應比第二次晚到。
    // 沒有序號保護時畫面會停在第一個角色的結果。
    server.use(
      http.get(`${API}/forms/:key/permissions/preview`, async ({ request }) => {
        const roles = new URL(request.url).searchParams.get('roles')
        if (roles === 'slow') {
          await delay(50)
          return HttpResponse.json([
            { key: 'a', permission: 'HIDDEN', required: false, reason: '舊的' },
          ])
        }
        return HttpResponse.json([
          { key: 'a', permission: 'EDITABLE', required: false, reason: '新的' },
        ])
      }),
    )

    const p = usePermissionPreview('quotation_form')
    const first = p.run({ roles: ['slow'], participantKind: 'internal' })
    const second = p.run({ roles: ['fast'], participantKind: 'internal' })

    await Promise.all([first, second])

    expect(p.result.value[0].reason, '應保留後發請求的結果').toBe('新的')
    expect(p.loading.value).toBe(false)
  })
})

describe('過時提示', () => {
  it('有結果時標記為過時', async () => {
    mockPreview(() => [{ key: 'a', permission: 'EDITABLE', required: false, reason: 'x' }])

    const p = usePermissionPreview('quotation_form')
    await p.run({ roles: ['admin'], participantKind: 'internal' })
    expect(p.stale.value).toBe(false)

    p.markStale()
    expect(p.stale.value).toBe(true)
  })

  it('尚無結果時不標記過時', () => {
    // 還沒預覽過就顯示「結果已過時」會讓人困惑
    const p = usePermissionPreview('quotation_form')
    p.markStale()
    expect(p.stale.value).toBe(false)
  })

  it('重新預覽後清除過時標記', async () => {
    mockPreview(() => [{ key: 'a', permission: 'EDITABLE', required: false, reason: 'x' }])

    const p = usePermissionPreview('quotation_form')
    await p.run({ roles: ['admin'], participantKind: 'internal' })
    p.markStale()

    await p.run({ roles: ['admin'], participantKind: 'internal' })
    expect(p.stale.value).toBe(false)
  })
})

describe('錯誤處理', () => {
  it('表單無版本時顯示後端訊息', async () => {
    server.use(
      http.get(`${API}/forms/:key/permissions/preview`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '表單尚無任何版本' },
          { status: 409 },
        ),
      ),
    )

    const p = usePermissionPreview('quotation_form')
    await p.run({ roles: ['admin'], participantKind: 'internal' })

    expect(p.error.value).toBe('表單尚無任何版本')
    expect(p.result.value).toEqual([])
    expect(p.loading.value).toBe(false)
  })

  it('清除會重置所有狀態', async () => {
    mockPreview(() => [{ key: 'a', permission: 'EDITABLE', required: false, reason: 'x' }])

    const p = usePermissionPreview('quotation_form')
    await p.run({ roles: ['admin'], participantKind: 'internal' })
    p.clear()

    expect(p.result.value).toEqual([])
    expect(p.stale.value).toBe(false)
    expect(p.error.value).toBeNull()
  })
})
