/**
 * 組織 API client 測試
 *
 * 回應結構刻意與後端 server/crates/http-public/src/org.rs 一致。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it } from 'vitest'
import { server } from '../msw/server'
import { setTokenGetter } from '@/api/client'
import * as org from '@/api/org'
import { ApiError } from '@/api/types'

const API = '*/api'

/** 後端回傳的員工結構，測試裡多次用到 */
function employee(overrides: Partial<org.Employee> = {}): org.Employee {
  return {
    id: 'u1',
    employee_no: 'E001',
    name: '林小明',
    email: 'staff@demo.local',
    department_id: 'd1',
    department_name: '業務部',
    manager_id: 'u2',
    manager_name: '陳經理',
    job_title: '工程師',
    phone: null,
    extension: null,
    hired_at: '2024-01-01',
    left_at: null,
    status: 'ACTIVE',
    can_login: true,
    sync_status: 'PLATFORM_ONLY',
    source_system: 'MANUAL',
    platform_managed_fields: [],
    roles: ['requester'],
    ...overrides,
  }
}

describe('組織健康檢查', () => {
  beforeEach(() => {
    setTokenGetter(() => 'h.p.s')
  })

  it('取回報表與計數', async () => {
    server.use(
      http.get(`${API}/org/health`, () =>
        HttpResponse.json({
          high_count: 1,
          medium_count: 1,
          employee_count: 12,
          department_count: 4,
          issues: [
            {
              code: 'MANAGER_INACTIVE',
              severity: 'HIGH',
              subject_type: 'employee',
              subject_id: 'u1',
              subject_name: '林小明',
              message: '直屬主管已停用或已離職。任務會指派給收不到通知的人',
              detail: '主管：陳經理',
            },
            {
              code: 'EMPLOYEE_NO_EMAIL',
              severity: 'MEDIUM',
              subject_type: 'employee',
              subject_id: 'u2',
              subject_name: '王小美',
              message: '沒有 email。通知寄不出去，且不會有錯誤訊息',
            },
          ],
        }),
      ),
    )

    const report = await org.health()

    expect(report.high_count).toBe(1)
    expect(report.employee_count).toBe(12)
    expect(report.issues).toHaveLength(2)
    // detail 是選填——沒有時不該變成 undefined 以外的東西
    expect(report.issues[0].detail).toBe('主管：陳經理')
    expect(report.issues[1].detail).toBeUndefined()
  })

  it('組織健康時回空清單而非錯誤', async () => {
    server.use(
      http.get(`${API}/org/health`, () =>
        HttpResponse.json({
          high_count: 0,
          medium_count: 0,
          employee_count: 8,
          department_count: 3,
          issues: [],
        }),
      ),
    )

    const report = await org.health()
    expect(report.issues).toEqual([])
    expect(report.high_count).toBe(0)
  })

  it('未登入時拋出 ApiError', async () => {
    server.use(
      http.get(`${API}/org/health`, () =>
        HttpResponse.json(
          { code: 'UNAUTHORIZED', message: '未提供憑證' },
          { status: 401 },
        ),
      ),
    )

    await expect(org.health()).rejects.toThrow(ApiError)
  })
})

describe('部門', () => {
  beforeEach(() => {
    setTokenGetter(() => 'h.p.s')
  })

  it('取回部門與成員數', async () => {
    server.use(
      http.get(`${API}/org/departments`, () =>
        HttpResponse.json([
          {
            id: 'd1',
            code: 'SALES',
            name: '業務部',
            parent_id: null,
            manager_user_id: 'u2',
            manager_name: '陳經理',
            status: 'ACTIVE',
            sort_order: 0,
            source_system: 'MANUAL',
            platform_managed_fields: [],
            member_count: 5,
          },
        ]),
      ),
    )

    const rows = await org.listDepartments()
    expect(rows[0].member_count).toBe(5)
    // 前端要顯示「誰在管」，名字一起帶回來，不該為此再查一次
    expect(rows[0].manager_name).toBe('陳經理')
  })

  it('建立部門回傳新的 id', async () => {
    server.use(
      http.post(`${API}/org/departments`, () =>
        HttpResponse.json({ id: 'd9' }),
      ),
    )

    const created = await org.createDepartment({ code: 'QA', name: '品保部' })
    expect(created.id).toBe('d9')
  })

  it('代碼重複時拋出 ApiError', async () => {
    server.use(
      http.post(`${API}/org/departments`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '違反唯一約束：department_tenant_id_code_key' },
          { status: 409 },
        ),
      ),
    )

    await expect(
      org.createDepartment({ code: 'SALES', name: '重複' }),
    ).rejects.toThrow(ApiError)
  })

  it('刪除空部門', async () => {
    let method = ''
    server.use(
      http.delete(`${API}/org/departments/d1`, ({ request }) => {
        method = request.method
        return HttpResponse.json({ ok: true })
      }),
    )

    await org.deleteDepartment('d1')
    expect(method).toBe('DELETE')
  })

  it('有成員的部門刪不掉，錯誤訊息說有幾個人', async () => {
    server.use(
      http.delete(`${API}/org/departments/d1`, () =>
        HttpResponse.json(
          {
            code: 'CONFLICT',
            message: '這個部門還有 3 位成員。請先把他們移到其他部門，或將部門改為停用',
          },
          { status: 409 },
        ),
      ),
    )

    // 放行的話那些人的 department_id 會被靜默清空
    await expect(org.deleteDepartment('d1')).rejects.toThrow(ApiError)
  })

  it('編輯帶上 fields 清單', async () => {
    let captured: unknown = null
    server.use(
      http.patch(`${API}/org/departments/d1`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({ ok: true })
      }),
    )

    await org.updateDepartment('d1', {
      fields: ['manager_user_id'],
      manager_user_id: null,
    })

    // fields 同時是「要改什麼」與「要鎖哪些欄位」的依據。
    // 少了它，把主管清成 null 與不改主管無法區分
    expect(captured).toEqual({
      fields: ['manager_user_id'],
      manager_user_id: null,
    })
  })
})

describe('員工', () => {
  beforeEach(() => {
    setTokenGetter(() => 'h.p.s')
  })

  it('取回員工與部門、主管名稱', async () => {
    server.use(
      http.get(`${API}/org/employees`, () => HttpResponse.json([employee()])),
    )

    const rows = await org.listEmployees()
    expect(rows[0].department_name).toBe('業務部')
    expect(rows[0].manager_name).toBe('陳經理')
  })

  it('關鍵字與部門進查詢字串', async () => {
    let captured = ''
    server.use(
      http.get(`${API}/org/employees`, ({ request }) => {
        captured = new URL(request.url).search
        return HttpResponse.json([])
      }),
    )

    await org.listEmployees({ q: '小明', department_id: 'd1' })
    expect(captured).toContain('q=')
    expect(captured).toContain('department_id=d1')
  })

  it('未帶參數時不送空的查詢字串', async () => {
    let captured = ''
    server.use(
      http.get(`${API}/org/employees`, ({ request }) => {
        captured = new URL(request.url).search
        return HttpResponse.json([])
      }),
    )

    await org.listEmployees()
    // 送 ?q= 會讓後端把空字串當成有效的過濾條件
    expect(captured).toBe('')
  })

  it('include_inactive 為 false 時不送', async () => {
    let captured = ''
    server.use(
      http.get(`${API}/org/employees`, ({ request }) => {
        captured = new URL(request.url).search
        return HttpResponse.json([])
      }),
    )

    await org.listEmployees({ include_inactive: false })
    expect(captured).toBe('')
  })

  it('不可編輯的欄位由後端擋下', async () => {
    server.use(
      http.patch(`${API}/org/employees/u1`, () =>
        HttpResponse.json(
          {
            code: 'VALIDATION_FAILED',
            message: '不可編輯的欄位「email」。可編輯的欄位：name、job_title',
          },
          { status: 422 },
        ),
      ),
    )

    // email 是登入帳號，改它等於換人
    await expect(
      org.updateEmployee('u1', { fields: ['email'] }),
    ).rejects.toThrow(ApiError)
  })

  it('主管成環由後端擋下', async () => {
    server.use(
      http.patch(`${API}/org/employees/u1`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '不能指定自己的下屬為主管，會讓簽核循環' },
          { status: 409 },
        ),
      ),
    )

    await expect(
      org.updateEmployee('u1', { fields: ['manager_id'], manager_id: 'u3' }),
    ).rejects.toThrow(ApiError)
  })
})

describe('欄位鎖定', () => {
  beforeEach(() => {
    setTokenGetter(() => 'h.p.s')
  })

  it('解除鎖定送出欄位清單', async () => {
    let captured: unknown = null
    let path = ''
    server.use(
      http.post(`${API}/org/employees/u1/unlock-fields`, async ({ request }) => {
        path = new URL(request.url).pathname
        captured = await request.json()
        return HttpResponse.json({ ok: true })
      }),
    )

    await org.unlockFields('employees', 'u1', ['job_title'])

    expect(path).toContain('/org/employees/u1/unlock-fields')
    expect(captured).toEqual({ fields: ['job_title'] })
  })

  it('部門與員工用同一支函式，路徑不同', async () => {
    let path = ''
    server.use(
      http.post(`${API}/org/departments/d1/unlock-fields`, ({ request }) => {
        path = new URL(request.url).pathname
        return HttpResponse.json({ ok: true })
      }),
    )

    await org.unlockFields('departments', 'd1', ['name'])
    expect(path).toContain('/org/departments/d1/unlock-fields')
  })
})
