/**
 * 基本資料查詢 API client 測試
 *
 * 這支 API 讓表單的下拉選單能綁定組織資料——選部門、選人員、選角色。
 * 先前 `reference.source` 是空字串，沒有東西可查；`select` 只能寫死
 * options，組織異動就得改表單定義。
 *
 * 回應結構刻意與後端 server/crates/http-public/src/lookup.rs 一致。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'
import { server } from '../msw/server'
import { setTokenGetter } from '@/api/client'
import * as lookup from '@/api/lookup'
import { ApiError } from '@/api/types'

const API = '*/api'

describe('基本資料查詢', () => {
  beforeEach(() => {
    setTokenGetter(() => 'h.p.s')
  })

  it('列出可用的資料來源', async () => {
    server.use(
      http.get(`${API}/lookup/sources`, () =>
        HttpResponse.json([
          {
            name: 'employee',
            label: '員工',
            extra_fields: ['employee_no', 'job_title'],
          },
          { name: 'department', label: '部門', extra_fields: ['code'] },
        ]),
      ),
    )

    const sources = await lookup.listSources()
    expect(sources).toHaveLength(2)
    expect(sources[0].name).toBe('employee')
    expect(sources[0].extra_fields).toContain('job_title')
  })

  it('查詢員工帶回 extra 供自動填入使用', async () => {
    server.use(
      http.get(`${API}/lookup/employee`, () =>
        HttpResponse.json([
          {
            value: 'u1',
            label: 'E001 陳雅婷',
            extra: {
              employee_no: 'E001',
              job_title: '產品經理',
              department_id: 'd1',
              email: 'designer@demo.local',
            },
          },
        ]),
      ),
    )

    const items = await lookup.query('employee')
    expect(items[0].label).toBe('E001 陳雅婷')
    // extra 供 option_source.fill 用——選了人自動帶出部門與職稱
    expect(items[0].extra?.job_title).toBe('產品經理')
  })

  it('關鍵字與筆數上限帶進查詢字串', async () => {
    let captured = ''
    server.use(
      http.get(`${API}/lookup/employee`, ({ request }) => {
        captured = new URL(request.url).search
        return HttpResponse.json([])
      }),
    )

    await lookup.query('employee', { q: '陳', limit: 10 })
    expect(captured).toContain('q=%E9%99%B3')
    expect(captured).toContain('limit=10')
  })

  it('未帶參數時不送空的查詢字串', async () => {
    let captured = ''
    server.use(
      http.get(`${API}/lookup/department`, ({ request }) => {
        captured = new URL(request.url).search
        return HttpResponse.json([])
      }),
    )

    await lookup.query('department')
    // 送 ?q=&limit= 會讓後端把空字串當成有效的過濾條件
    expect(captured).toBe('')
  })

  it('未知的來源回報後端的錯誤訊息', async () => {
    server.use(
      http.get(`${API}/lookup/app_user`, () =>
        HttpResponse.json(
          {
            code: 'BAD_REQUEST',
            message: '未知的資料來源「app_user」。可用的來源：employee、department、role',
          },
          { status: 400 },
        ),
      ),
    )

    // 白名單擋住直接查任意資料表——訊息要告訴設計者有哪些可用
    await expect(lookup.query('app_user')).rejects.toThrow(ApiError)
  })
})
