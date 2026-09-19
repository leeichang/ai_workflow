/**
 * 種子表單的 HIDDEN 權限路徑回歸
 *
 * 對應總結 04 的 N4：「種子表單沒有任何 readable_roles，
 * 所以權限過濾的 HIDDEN 路徑沒有實機驗證過」。
 *
 * ## 實際的問題比 N4 描述的更根本
 *
 * `seed.sh` 裡的 `internal_note` **一直都有** `readable_roles`。
 * 沒生效是因為 seed 只用 `POST /forms` 建立表單——表單已存在時
 * 撞唯一約束、印出 CONFLICT 就跳過，接著把**舊草稿**發布出去。
 *
 * 也就是說：**表單定義的任何修改都不會套用到既有環境**。
 * readable_roles 只是第一個被發現的受害者。
 *
 * seed 已改成「建立失敗就更新草稿」。這條測試守住結果——
 * 種子環境必須有一個 requester 看不到、approver 看得到的欄位，
 * 否則 HIDDEN 這條路徑又會退回只有單元測試涵蓋的狀態。
 *
 * 需先啟動全部六個服務並跑過 seed。
 */

import { expect, test } from '@playwright/test'

const API = 'http://localhost:3001'

/** 直接打 API 取權限矩陣。這條測試驗的是種子資料，不是畫面 */
async function permissionsOf(
  request: import('@playwright/test').APIRequestContext,
  role: string,
): Promise<Record<string, string>> {
  const login = await request.post(`${API}/auth/login`, {
    data: {
      tenant_code: 'demo',
      email: 'designer@demo.local',
      password: 'demo1234',
    },
  })
  const { access_token } = (await login.json()) as { access_token: string }

  const resp = await request.get(
    `${API}/forms/quotation_form/permissions?mode=by_role&role=${role}&node=start`,
    { headers: { Authorization: `Bearer ${access_token}` } },
  )
  expect(resp.status()).toBe(200)

  const body = (await resp.json()) as {
    rows: { key: string; cells: Record<string, { permission: string }> }[]
  }

  const result: Record<string, string> = {}
  for (const row of body.rows) {
    result[row.key] = row.cells.start?.permission ?? '?'
  }
  return result
}

test('種子表單有 requester 看不到的欄位', async ({ request }) => {
  const requester = await permissionsOf(request, 'requester')

  // 這是整個 readable_roles 機制唯一的實機證據。
  // 若種子退回沒有角色限制的版本，這裡會變成 EDITABLE 或 READONLY
  expect(requester['internal_note']).toBe('HIDDEN')
})

test('同一個欄位對 approver 是看得到的', async ({ request }) => {
  const approver = await permissionsOf(request, 'approver')

  // 只驗 HIDDEN 不夠——一個所有人都看不到的欄位也會通過上一條，
  // 但那代表的是設定錯誤而非權限過濾正常運作
  expect(approver['internal_note']).not.toBe('HIDDEN')
})

test('計算欄位對所有角色都是唯讀', async ({ request }) => {
  const requester = await permissionsOf(request, 'requester')
  const approver = await permissionsOf(request, 'approver')

  // margin_rate 有 computed，唯讀的理由與 readable_roles 無關。
  // 放在這裡是為了區分兩種機制——先前排查 N4 時，
  // 「唯讀」與「看不到」混在一起看不出是哪一種
  expect(requester['margin_rate']).toBe('READONLY')
  expect(approver['margin_rate']).toBe('READONLY')
})
