/**
 * 組織資料同步 API client 測試
 *
 * 回應結構刻意與後端 server/crates/http-public/src/integration.rs 一致。
 *
 * 重點在**該擋的時候前端讀不讀得懂**：完整性閘中止時，
 * 呼叫端要能分辨「成功但有問題」與「整批沒寫」。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'
import { server } from '../msw/server'
import { setTokenGetter } from '@/api/client'
import * as integration from '@/api/integration'
import { ApiError } from '@/api/types'

const API = '*/api'

function counts(
  overrides: Partial<integration.SyncCounts> = {},
): integration.SyncCounts {
  return {
    received_count: 0,
    parsed_count: 0,
    mapped_count: 0,
    validated_count: 0,
    inserted_count: 0,
    updated_count: 0,
    unchanged_count: 0,
    skipped_by_ownership_count: 0,
    rejected_count: 0,
    missing_in_source_count: 0,
    ...overrides,
  }
}

describe('分析檔案', () => {
  beforeEach(() => {
    setTokenGetter(() => 'h.p.s')
  })

  it('回表頭與欄位對應建議', async () => {
    server.use(
      http.post(`${API}/integration/analyze`, () =>
        HttpResponse.json({
          headers: ['工號', '姓名', '主管工號'],
          row_count: 120,
          sample_rows: [['E001', '陳大明', '']],
          suggestions: [
            {
              source_field: '工號',
              canonical_field: 'employee_no',
              label: '員工編號',
              confidence: 'exact',
            },
            {
              source_field: '姓名',
              canonical_field: 'name',
              label: '姓名',
              confidence: 'exact',
            },
            {
              source_field: '主管工號',
              canonical_field: 'manager_employee_no',
              label: '主管員工編號',
              confidence: 'exact',
            },
          ],
        }),
      ),
    )

    const result = await integration.analyze('工號\nE001\n', 'employee')

    expect(result.row_count).toBe(120)
    // 「主管工號」不可以被 employee_no 吃掉
    expect(result.suggestions[2].canonical_field).toBe('manager_employee_no')
  })

  it('猜不出來的欄位回 null 而非空字串', async () => {
    server.use(
      http.post(`${API}/integration/analyze`, () =>
        HttpResponse.json({
          headers: ['備註'],
          row_count: 1,
          sample_rows: [],
          suggestions: [
            {
              source_field: '備註',
              canonical_field: null,
              label: null,
              confidence: 'none',
            },
          ],
        }),
      ),
    )

    const result = await integration.analyze('備註\nx\n', 'employee')

    // null 與空字串要分得出來——後者會被誤認為「對應到一個無名欄位」
    expect(result.suggestions[0].canonical_field).toBeNull()
  })

  it('檔案格式錯誤時拋出 ApiError', async () => {
    server.use(
      http.post(`${API}/integration/analyze`, () =>
        HttpResponse.json(
          { code: 'VALIDATION_FAILED', message: '檔案沒有標題列' },
          { status: 422 },
        ),
      ),
    )

    await expect(integration.analyze('', 'employee')).rejects.toThrow(ApiError)
  })
})

describe('同步', () => {
  beforeEach(() => {
    setTokenGetter(() => 'h.p.s')
  })

  it('試跑回完整計數', async () => {
    server.use(
      http.post(`${API}/integration/sources/s1/preview`, () =>
        HttpResponse.json({
          status: 'SUCCESS',
          counts: counts({
            received_count: 350,
            validated_count: 350,
            inserted_count: 12,
            updated_count: 338,
            skipped_by_ownership_count: 8,
          }),
          issues: [],
          message: null,
        }),
      ),
    )

    const outcome = await integration.preview('s1', 'csv')

    // 管理員要能在寫入前看到會發生什麼
    expect(outcome.counts.inserted_count).toBe(12)
    expect(outcome.counts.updated_count).toBe(338)
    expect(outcome.counts.skipped_by_ownership_count).toBe(8)
  })

  it('完整性閘中止時要讀得出原因', async () => {
    server.use(
      http.post(`${API}/integration/sources/s1/sync`, () =>
        HttpResponse.json({
          status: 'ABORTED_INCOMPLETE',
          counts: counts({ received_count: 823, validated_count: 823 }),
          issues: [],
          message:
            '本次只有 823 筆通過驗證，低於上次成功同步的 1000 筆 × 90%。整批中止，未寫入任何變更',
        }),
      ),
    )

    const outcome = await integration.sync('s1', 'csv')

    // 中止是 200 而非錯誤——它是同步的一種正常結果，
    // 呼叫端要靠 status 而非 HTTP 狀態碼判斷
    expect(outcome.status).toBe('ABORTED_INCOMPLETE')
    expect(outcome.message).toContain('整批中止')
    expect(outcome.counts.inserted_count).toBe(0)
  })

  it('部分成功時帶回問題明細', async () => {
    server.use(
      http.post(`${API}/integration/sources/s1/sync`, () =>
        HttpResponse.json({
          status: 'PARTIAL_SUCCESS',
          counts: counts({ validated_count: 2, rejected_count: 1 }),
          issues: [
            {
              row_number: 3,
              kind: 'VALIDATION_FAILED',
              message: '缺少員工編號',
              field: 'employee_no',
            },
            {
              row_number: null,
              kind: 'MISSING_IN_SOURCE',
              message: '此人在本次來源資料中消失',
              field: null,
            },
          ],
          message: null,
        }),
      ),
    )

    const outcome = await integration.sync('s1', 'csv')

    expect(outcome.issues).toHaveLength(2)
    // row_number 讓管理員回到 Excel 找得到那一行
    expect(outcome.issues[0].row_number).toBe(3)
    // 來源消失不對應到特定的列
    expect(outcome.issues[1].row_number).toBeNull()
  })
})

describe('來源設定', () => {
  beforeEach(() => {
    setTokenGetter(() => 'h.p.s')
  })

  it('建立來源送出欄位對應', async () => {
    let captured: unknown = null
    server.use(
      http.post(`${API}/integration/sources`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({ id: 's9' })
      }),
    )

    await integration.createSource({
      connection_id: 'c1',
      dataset: 'employee',
      name: '員工主檔',
      mapping: { 工號: 'employee_no' },
    })

    expect(captured).toMatchObject({
      dataset: 'employee',
      mapping: { 工號: 'employee_no' },
    })
  })

  it('缺少員工編號對應時後端擋下', async () => {
    server.use(
      http.post(`${API}/integration/sources`, () =>
        HttpResponse.json(
          {
            code: 'VALIDATION_FAILED',
            message: '必須對應員工編號。沒有它無法判斷是新人還是既有員工',
          },
          { status: 422 },
        ),
      ),
    )

    await expect(
      integration.createSource({
        connection_id: 'c1',
        dataset: 'employee',
        name: 'x',
        mapping: { 姓名: 'name' },
      }),
    ).rejects.toThrow(ApiError)
  })

  it('從未同步過時 last_success_count 為 null', async () => {
    server.use(
      http.get(`${API}/integration/sources/s1`, () =>
        HttpResponse.json({
          id: 's1',
          connection_id: 'c1',
          dataset: 'employee',
          name: '員工主檔',
          mapping_definition: { 工號: 'employee_no' },
          completeness_threshold: 0.9,
          last_success_count: null,
          last_synced_at: null,
        }),
      ),
    )

    const source = await integration.getSource('s1')

    // null 代表完整性閘不生效，與 0 是不同的意思
    expect(source.last_success_count).toBeNull()
  })

  it('門檻設為 0 時後端擋下', async () => {
    server.use(
      http.patch(`${API}/integration/sources/s1`, () =>
        HttpResponse.json(
          {
            code: 'VALIDATION_FAILED',
            message: '完整性門檻必須大於 0 且不超過 1。設為 0 等於關掉保護',
          },
          { status: 422 },
        ),
      ),
    )

    await expect(
      integration.updateSource('s1', { completeness_threshold: 0 }),
    ).rejects.toThrow(ApiError)
  })
})

describe('同步歷史', () => {
  beforeEach(() => {
    setTokenGetter(() => 'h.p.s')
  })

  it('列出同步紀錄，試跑也在其中', async () => {
    server.use(
      http.get(`${API}/integration/sync-runs`, () =>
        HttpResponse.json([
          {
            id: 'r1',
            source_id: 's1',
            status: 'SUCCESS',
            dry_run: true,
            received_count: 350,
            validated_count: 350,
            inserted_count: 12,
            updated_count: 338,
            unchanged_count: 0,
            skipped_by_ownership_count: 0,
            rejected_count: 0,
            missing_in_source_count: 0,
            message: null,
            started_at: '2026-09-19T00:00:00Z',
            finished_at: '2026-09-19T00:00:01Z',
          },
        ]),
      ),
    )

    const runs = await integration.listRuns()

    // 試跑也留紀錄——管理員回頭要看得到那天試跑的結果
    expect(runs[0].dry_run).toBe(true)
  })

  it('取得某次同步的問題明細', async () => {
    server.use(
      http.get(`${API}/integration/sync-runs/r1/issues`, () =>
        HttpResponse.json([
          {
            id: 'i1',
            row_number: 5,
            kind: 'SKIPPED_BY_OWNERSHIP',
            message: '欄位「job_title」由平台維護，本次同步未覆蓋',
            field: 'job_title',
          },
        ]),
      ),
    )

    const issues = await integration.listIssues('r1')

    expect(issues[0].kind).toBe('SKIPPED_BY_OWNERSHIP')
    expect(issues[0].field).toBe('job_title')
  })
})
