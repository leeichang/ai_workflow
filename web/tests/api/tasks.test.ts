/**
 * 待辦 API client 測試
 */

import { HttpResponse, http } from 'msw'
import { describe, expect, it } from 'vitest'
import { server } from '../msw/server'
import * as tasks from '@/api/tasks'
import { ApiError } from '@/api/types'

const API = '*/api'

const TASK = {
  id: 'task-1',
  instance_id: 'inst-1',
  node_id: 'manager_approval',
  node_label: '主管簽核',
  assignee_user_id: 'u1',
  assignee_role: null,
  participant_kind: 'internal',
  form_key: 'quotation_form',
  status: 'PENDING',
  decision: null,
  comment: null,
  due_at: null,
  created_at: '2026-09-16T06:08:38Z',
  decided_at: null,
  business_key: 'QT-MANUAL-001',
  business_object: 'quotation',
}

describe('list', () => {
  it('預設查 PENDING', async () => {
    let url = ''
    server.use(
      http.get(`${API}/tasks`, ({ request }) => {
        url = request.url
        return HttpResponse.json([TASK])
      }),
    )

    await tasks.list()
    expect(url).toContain('status=PENDING')
  })

  it('可指定狀態', async () => {
    let url = ''
    server.use(
      http.get(`${API}/tasks`, ({ request }) => {
        url = request.url
        return HttpResponse.json([])
      }),
    )

    await tasks.list({ status: 'APPROVED' })
    expect(url).toContain('status=APPROVED')
  })

  it('回傳待辦連同單號', async () => {
    server.use(http.get(`${API}/tasks`, () => HttpResponse.json([TASK])))

    const result = await tasks.list()
    expect(result).toHaveLength(1)
    expect(result[0].business_key).toBe('QT-MANUAL-001')
    expect(result[0].node_label).toBe('主管簽核')
  })

  it('空收件匣回空陣列而非拋錯', async () => {
    server.use(http.get(`${API}/tasks`, () => HttpResponse.json([])))
    await expect(tasks.list()).resolves.toEqual([])
  })
})

describe('decide', () => {
  it('送出核准', async () => {
    let body: unknown
    server.use(
      http.post(`${API}/tasks/task-1/decision`, async ({ request }) => {
        body = await request.json()
        return HttpResponse.json({
          task_id: 'task-1',
          decision: 'APPROVE',
          signalled: true,
        })
      }),
    )

    const result = await tasks.decide('task-1', {
      decision: 'APPROVE',
      comment: '同意',
    })

    expect(body).toEqual({ decision: 'APPROVE', comment: '同意' })
    expect(result.signalled).toBe(true)
  })

  it('沒填意見時送空字串，後端 default 需要這個欄位存在', async () => {
    let body: unknown
    server.use(
      http.post(`${API}/tasks/task-1/decision`, async ({ request }) => {
        body = await request.json()
        return HttpResponse.json({
          task_id: 'task-1',
          decision: 'REJECT',
          signalled: true,
        })
      }),
    )

    await tasks.decide('task-1', { decision: 'REJECT' })
    expect(body).toEqual({ decision: 'REJECT', comment: '' })
  })

  it('別人的待辦回 403', async () => {
    server.use(
      http.post(`${API}/tasks/task-1/decision`, () =>
        HttpResponse.json(
          { code: 'FORBIDDEN', message: '此待辦不是指派給您的' },
          { status: 403 },
        ),
      ),
    )

    await expect(
      tasks.decide('task-1', { decision: 'APPROVE' }),
    ).rejects.toBeInstanceOf(ApiError)
  })

  it('已被處理回 409，訊息要能顯示給使用者', async () => {
    server.use(
      http.post(`${API}/tasks/task-1/decision`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '待辦已被處理' },
          { status: 409 },
        ),
      ),
    )

    const error = await tasks
      .decide('task-1', { decision: 'APPROVE' })
      .catch((e: unknown) => e)

    expect(error).toBeInstanceOf(ApiError)
    expect((error as ApiError).message).toBe('待辦已被處理')
  })

  it('流程引擎斷線回 503，屬於可重試', async () => {
    server.use(
      http.post(`${API}/tasks/task-1/decision`, () =>
        HttpResponse.json(
          { code: 'SERVICE_UNAVAILABLE', message: '流程引擎未連線' },
          { status: 503 },
        ),
      ),
    )

    const error = await tasks
      .decide('task-1', { decision: 'APPROVE' })
      .catch((e: unknown) => e)

    expect((error as ApiError).retryable).toBe(true)
  })
})
