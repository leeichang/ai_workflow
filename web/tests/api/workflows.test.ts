/**
 * 流程定義 API client 測試
 *
 * 回應結構刻意與後端 server/crates/http-public/src/workflows.rs 及
 * crates/persistence/src/workflow.rs 對齊。後端改欄位時這裡會失敗。
 *
 * 與表單 API 最大的差異在驗證：流程驗證失敗仍回 200，
 * 逐條錯誤帶 node_id。這個約定若被破壞，畫布就標不出出錯節點。
 */

import { HttpResponse, http } from 'msw'
import { describe, expect, it } from 'vitest'
import { server } from '../msw/server'
import { setTokenGetter } from '@/api/client'
import * as workflows from '@/api/workflows'
import { ApiError } from '@/api/types'
import type { WorkflowContent } from '@/api/workflows'

const API = '*/api'

function sampleContent(key = 'quotation_approval'): WorkflowContent {
  return {
    workflow_key: key,
    version: 1,
    business_object: 'quotation',
    trigger: { type: 'form_submit' },
    nodes: [
      { id: 'start', type: 'trigger', label: '開始' },
      {
        id: 'mgr',
        type: 'human_approval',
        label: '主管簽核',
        participant: 'internal',
        resolver: { type: 'manager_of', of: 'initiator' },
      },
      { id: 'done', type: 'end', label: '結束', result: 'completed' },
    ],
    edges: [
      ['start', 'mgr'],
      ['mgr', 'done'],
    ],
  }
}

function sampleVersion(overrides: Record<string, unknown> = {}) {
  return {
    id: 'ver-1',
    workflow_id: 'wf-1',
    version: null,
    content: sampleContent(),
    status: 'DRAFT',
    created_by: 'user-1',
    created_at: '2026-09-15T08:00:00Z',
    updated_at: '2026-09-15T08:00:00Z',
    published_by: null,
    published_at: null,
    ...overrides,
  }
}

function sampleDefinition(overrides: Record<string, unknown> = {}) {
  return {
    id: 'wf-1',
    workflow_key: 'quotation_approval',
    business_object: 'quotation',
    name: '報價單簽核',
    description: null,
    created_at: '2026-09-15T08:00:00Z',
    updated_at: '2026-09-15T08:00:00Z',
    ...overrides,
  }
}

describe('listWorkflows', () => {
  it('解析 summary 的扁平化欄位', async () => {
    // 後端用 #[serde(flatten)] 把 definition 攤平進 summary，
    // 前端型別必須跟著是扁平的，不能是巢狀的 definition。
    server.use(
      http.get(`${API}/workflows`, () =>
        HttpResponse.json([
          { ...sampleDefinition(), published_version: 2, has_draft: true },
        ]),
      ),
    )

    const list = await workflows.listWorkflows()
    expect(list).toHaveLength(1)
    expect(list[0].workflow_key).toBe('quotation_approval')
    expect(list[0].published_version).toBe(2)
    expect(list[0].has_draft).toBe(true)
  })

  it('帶上認證標頭', async () => {
    setTokenGetter(() => 'abc123')
    let auth: string | null = null

    server.use(
      http.get(`${API}/workflows`, ({ request }) => {
        auth = request.headers.get('authorization')
        return HttpResponse.json([])
      }),
    )

    await workflows.listWorkflows()
    expect(auth).toBe('Bearer abc123')
  })
})

describe('getWorkflow', () => {
  it('同時解析草稿與已發布版本', async () => {
    server.use(
      http.get(`${API}/workflows/quotation_approval`, () =>
        HttpResponse.json({
          ...sampleDefinition(),
          draft: sampleVersion(),
          published: sampleVersion({
            id: 'ver-0',
            version: 1,
            status: 'PUBLISHED',
            published_at: '2026-09-15T09:00:00Z',
          }),
        }),
      ),
    )

    const detail = await workflows.getWorkflow('quotation_approval')
    expect(detail.draft?.version, '草稿的 version 為 null').toBeNull()
    expect(detail.published?.version).toBe(1)
    expect(detail.draft?.content.nodes).toHaveLength(3)
  })

  it('只有已發布版本時 draft 為 null', async () => {
    server.use(
      http.get(`${API}/workflows/quotation_approval`, () =>
        HttpResponse.json({
          ...sampleDefinition(),
          draft: null,
          published: sampleVersion({ version: 1, status: 'PUBLISHED' }),
        }),
      ),
    )

    const detail = await workflows.getWorkflow('quotation_approval')
    expect(detail.draft).toBeNull()
    expect(detail.published).not.toBeNull()
  })

  it('key 做 URL 編碼', async () => {
    let path = ''
    server.use(
      http.get(`${API}/workflows/:key`, ({ request }) => {
        path = new URL(request.url).pathname
        return HttpResponse.json({ ...sampleDefinition(), draft: null, published: null })
      }),
    )

    await workflows.getWorkflow('a/b')
    expect(path).toContain('a%2Fb')
  })

  it('找不到時拋出 ApiError', async () => {
    server.use(
      http.get(`${API}/workflows/missing`, () =>
        HttpResponse.json(
          { code: 'NOT_FOUND', message: '流程不存在' },
          { status: 404 },
        ),
      ),
    )

    await expect(workflows.getWorkflow('missing')).rejects.toThrow(ApiError)
    await expect(workflows.getWorkflow('missing')).rejects.toMatchObject({
      status: 404,
      code: 'NOT_FOUND',
    })
  })
})

describe('createWorkflow', () => {
  it('送出完整的建立請求', async () => {
    let body: Record<string, unknown> = {}
    server.use(
      http.post(`${API}/workflows`, async ({ request }) => {
        body = (await request.json()) as Record<string, unknown>
        return HttpResponse.json(
          { ...sampleDefinition(), draft: sampleVersion(), published: null },
          { status: 201 },
        )
      }),
    )

    await workflows.createWorkflow({
      workflow_key: 'quotation_approval',
      business_object: 'quotation',
      name: '報價單簽核',
      content: sampleContent(),
    })

    expect(body.workflow_key).toBe('quotation_approval')
    expect((body.content as WorkflowContent).nodes).toHaveLength(3)
  })

  it('權限不足時拋出 FORBIDDEN', async () => {
    server.use(
      http.post(`${API}/workflows`, () =>
        HttpResponse.json(
          { code: 'FORBIDDEN', message: '需要 designer 角色' },
          { status: 403 },
        ),
      ),
    )

    await expect(
      workflows.createWorkflow({
        workflow_key: 'x_flow',
        business_object: 'quotation',
        name: 'x',
        content: sampleContent('x_flow'),
      }),
    ).rejects.toMatchObject({ code: 'FORBIDDEN' })
  })
})

describe('saveWorkflowDraft', () => {
  it('以 PUT 送出並包在 content 鍵下', async () => {
    let method = ''
    let body: Record<string, unknown> = {}

    server.use(
      http.put(`${API}/workflows/quotation_approval/draft`, async ({ request }) => {
        method = request.method
        body = (await request.json()) as Record<string, unknown>
        return HttpResponse.json(sampleVersion())
      }),
    )

    await workflows.saveWorkflowDraft('quotation_approval', sampleContent())

    expect(method).toBe('PUT')
    expect(body).toHaveProperty('content')
    expect((body.content as WorkflowContent).workflow_key).toBe('quotation_approval')
  })

  it('儲存不做圖驗證，斷開的流程也能存', async () => {
    // 編輯中途流程常是斷的，存檔就報錯會妨礙設計。
    const broken = sampleContent()
    broken.edges = []

    server.use(
      http.put(`${API}/workflows/quotation_approval/draft`, () =>
        HttpResponse.json(sampleVersion({ content: broken })),
      ),
    )

    const v = await workflows.saveWorkflowDraft('quotation_approval', broken)
    expect(v.content.edges).toHaveLength(0)
  })
})

describe('discardWorkflowDraft', () => {
  it('以 DELETE 呼叫且容許空回應', async () => {
    let method = ''
    server.use(
      http.delete(`${API}/workflows/quotation_approval/draft`, ({ request }) => {
        method = request.method
        return new HttpResponse(null, { status: 204 })
      }),
    )

    await workflows.discardWorkflowDraft('quotation_approval')
    expect(method).toBe('DELETE')
  })
})

describe('validateWorkflowDraft', () => {
  it('通過時 valid 為 true 且無 errors 欄位', async () => {
    // 後端用 skip_serializing_if 省略空陣列，前端必須容忍缺鍵
    server.use(
      http.post(`${API}/workflows/quotation_approval/draft/validate`, () =>
        HttpResponse.json({ valid: true }),
      ),
    )

    const r = await workflows.validateWorkflowDraft('quotation_approval')
    expect(r.valid).toBe(true)
    expect(r.errors).toBeUndefined()
  })

  it('失敗仍回 200，逐條錯誤帶 node_id', async () => {
    // 用 200 而非 4xx：畫布需要完整的錯誤清單來標記節點，
    // 用錯誤狀態碼表達會讓 client 走到例外分支，處理變複雜。
    server.use(
      http.post(`${API}/workflows/quotation_approval/draft/validate`, () =>
        HttpResponse.json({
          valid: false,
          errors: [
            { code: 'WF-E002', node_id: 'orphan', message: '節點無法從 trigger 到達' },
            { code: 'WF-E001', node_id: null, message: '缺少 trigger 節點' },
          ],
        }),
      ),
    )

    const r = await workflows.validateWorkflowDraft('quotation_approval')
    expect(r.valid).toBe(false)
    expect(r.errors).toHaveLength(2)
    expect(r.errors![0].node_id).toBe('orphan')
    expect(r.errors![1].node_id, '全域錯誤的 node_id 為 null').toBeNull()
  })
})

describe('publishWorkflowDraft', () => {
  it('回傳已發布版本號', async () => {
    server.use(
      http.post(`${API}/workflows/quotation_approval/draft/publish`, () =>
        HttpResponse.json(
          sampleVersion({
            version: 2,
            status: 'PUBLISHED',
            published_at: '2026-09-15T10:00:00Z',
          }),
        ),
      ),
    )

    const v = await workflows.publishWorkflowDraft('quotation_approval')
    expect(v.version).toBe(2)
    expect(v.status).toBe('PUBLISHED')
  })

  it('驗證失敗時把逐條錯誤放進 ApiError.validationErrors', async () => {
    server.use(
      http.post(`${API}/workflows/quotation_approval/draft/publish`, () =>
        HttpResponse.json(
          {
            code: 'VALIDATION_FAILED',
            message: '流程結構有誤',
            details: { errors: ['WF-E002 節點無法到達', 'WF-E004 退回目標不在上游'] },
          },
          { status: 422 },
        ),
      ),
    )

    try {
      await workflows.publishWorkflowDraft('quotation_approval')
      expect.unreachable('應該拋出 ApiError')
    } catch (e) {
      expect(e).toBeInstanceOf(ApiError)
      expect((e as ApiError).validationErrors).toHaveLength(2)
    }
  })
})

describe('listWorkflowVersions', () => {
  it('回傳版本清單', async () => {
    server.use(
      http.get(`${API}/workflows/quotation_approval/versions`, () =>
        HttpResponse.json([
          sampleVersion({ version: 2, status: 'PUBLISHED' }),
          sampleVersion({ id: 'ver-0', version: 1, status: 'ARCHIVED' }),
        ]),
      ),
    )

    const list = await workflows.listWorkflowVersions('quotation_approval')
    expect(list.map((v) => v.version)).toEqual([2, 1])
  })
})

describe('錯誤處理', () => {
  it('伺服器錯誤標記為可重試', async () => {
    server.use(
      http.get(`${API}/workflows`, () =>
        HttpResponse.json({ code: 'INTERNAL', message: '資料庫連線失敗' }, { status: 500 }),
      ),
    )

    try {
      await workflows.listWorkflows()
      expect.unreachable('應該拋出 ApiError')
    } catch (e) {
      expect((e as ApiError).retryable).toBe(true)
    }
  })

  it('未認證時拋出 401', async () => {
    setTokenGetter(() => null)
    server.use(
      http.get(`${API}/workflows`, () =>
        HttpResponse.json({ code: 'UNAUTHORIZED', message: '請先登入' }, { status: 401 }),
      ),
    )

    await expect(workflows.listWorkflows()).rejects.toMatchObject({ status: 401 })
  })
})
