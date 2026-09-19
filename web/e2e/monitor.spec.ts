/**
 * 流程監控 E2E（S0）
 *
 * 驗收標準（交辦 §5 S0）：
 *   「選單有『流程監控』，看得到所有執行中流程的清單與詳情。
 *     權限正確（一般使用者看不到別人的單），欄位遮蔽正確。」
 *
 * 需先啟動全部六個服務。
 */

import { expect, test, type Page, type APIRequestContext } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/測試報告/202609/screenshots/monitor')
mkdirSync(SHOTS, { recursive: true })

const API = 'http://localhost:3001'

async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

async function login(page: Page, email = 'designer@demo.local'): Promise<void> {
  await page.goto('/login')
  await page.getByTestId('login-tenant').fill('demo')
  await page.getByTestId('login-email').fill(email)
  await page.getByTestId('login-password').fill('demo1234')
  await page.getByTestId('login-submit').click()
  await expect(page).toHaveURL('http://localhost:3040/')
}

async function token(
  request: APIRequestContext,
  email: string,
): Promise<string> {
  const res = await request.post(`${API}/auth/login`, {
    data: { tenant_code: 'demo', email, password: 'demo1234' },
  })
  expect(res.ok(), `登入 ${email} 失敗`).toBeTruthy()
  return (await res.json()).access_token
}

test.describe('流程監控', () => {
  test('選單進得去，看得到流程清單與詳情', async ({ page }) => {
    await login(page)

    // 從選單進入——交辦明寫「選單有『流程監控』」
    await page.getByRole('link', { name: '流程監控' }).click()
    await expect(page).toHaveURL(/\/monitor$/)

    await expect(page.getByTestId('monitor-table')).toBeVisible({
      timeout: 15_000,
    })
    await shot(page, '01-流程清單')

    await page.locator('[data-testid^="monitor-open-"]').first().click()
    await expect(page.getByTestId('monitor-detail')).toBeVisible()

    // 本地狀態與引擎回報分開顯示——兩者不一致本身就是訊號
    await expect(page.getByTestId('monitor-detail-status')).toBeVisible()
    await expect(page.getByTestId('monitor-detail-live')).toBeVisible()

    await shot(page, '02-流程詳情')
  })

  test('一般使用者看不到別人的單', async ({ page, request }) => {
    // **最重要的一條。** RLS 只隔離到租戶——同租戶的人
    // 預設看得見彼此的單。少了 visible_to，流程監控就變成
    // 全租戶的資料出口。
    const designerJwt = await token(request, 'designer@demo.local')
    const salesJwt = await token(request, 'sales@demo.local')

    const mine = await request.get(`${API}/instances?limit=200`, {
      headers: { Authorization: `Bearer ${designerJwt}` },
    })
    const theirs = await request.get(`${API}/instances?limit=200`, {
      headers: { Authorization: `Bearer ${salesJwt}` },
    })

    const mineKeys = new Set(
      (await mine.json()).map((i: { business_key: string }) => i.business_key),
    )
    const theirsKeys = (await theirs.json()).map(
      (i: { business_key: string }) => i.business_key,
    )

    // sales 不該看到 designer 的全部單
    expect(
      theirsKeys.length,
      'sales 看到的筆數不該等於 designer 的——那代表沒有過濾',
    ).toBeLessThan(mineKeys.size)

    await login(page, 'sales@demo.local')
    await page.goto('/monitor')
    await shot(page, '03-一般使用者的可見範圍')
  })

  test('知道 id 也讀不到看不見的單', async ({ request }) => {
    // 清單過濾了，單筆也要濾——否則知道 id 就能繞過，
    // 而 id 會出現在通知連結與稽核紀錄裡，不是秘密。
    const designerJwt = await token(request, 'designer@demo.local')
    const salesJwt = await token(request, 'sales@demo.local')

    const mine = await request.get(`${API}/instances?limit=200`, {
      headers: { Authorization: `Bearer ${designerJwt}` },
    })
    const theirs = await request.get(`${API}/instances?limit=200`, {
      headers: { Authorization: `Bearer ${salesJwt}` },
    })

    // 必須挑一筆 sales **確實看不到**的單。
    // 直接拿第一筆會隨其他測試建立的資料而變——那是測試自己的缺陷，
    // 不是產品問題（本檔先前就因此在全套執行時紅過一次）。
    const visibleToSales = new Set(
      (await theirs.json()).map((i: { id: string }) => i.id),
    )
    const hidden = (await mine.json()).find(
      (i: { id: string }) => !visibleToSales.has(i.id),
    )
    expect(
      hidden,
      '需要一筆 designer 看得到、sales 看不到的單才測得出來',
    ).toBeTruthy()

    const res = await request.get(`${API}/instances/${hidden.id}`, {
      headers: { Authorization: `Bearer ${salesJwt}` },
    })

    // 回 404 而非 403：403 等於告訴對方「這張單存在」
    expect(
      res.status(),
      `看不到的單應回 404，實際：${await res.text()}`,
    ).toBe(404)
  })
})
