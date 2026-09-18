/**
 * 流程清單與 0→1 建立 E2E
 *
 * 補的缺口與表單那半相同：後端有 POST /workflows、前端的
 * createWorkflow() 也寫好了，但先前沒有任何畫面呼叫它，
 * 使用者無法從零建立一個流程。
 *
 * 為什麼單元測試不夠：MSW 不驗 schema，也不跑圖結構驗證。
 * 表單那半就是在實機才發現 fields 有 minItems:1、
 * 欄位結構是 { key, section, ui, data } 而非扁平的。
 *
 * 這裡最關鍵的一條是「建立後可以發布」——流程與表單不同，
 * 圖結構要通過 WF-E001～E012。產生一個建了卻發布不了的流程
 * 比不給建還糟，使用者會以為是自己設計錯了。
 *
 * 需先啟動全部六個服務。流程 key 帶時間戳避免與既有資料衝突。
 */

import { expect, test, type Page, type APIRequestContext } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/測試報告/202609/screenshots/workflow-list')
mkdirSync(SHOTS, { recursive: true })

const API = 'http://localhost:3001'

/**
 * 截圖前先等網路靜止
 *
 * toHaveURL 會重試到通過，screenshot() 卻是立即執行——
 * URL 已經變了而畫面還停在上一頁，截出來就是錯的。
 */
async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

async function login(page: Page): Promise<void> {
  await page.goto('/login')
  await page.getByTestId('login-tenant').fill('demo')
  await page.getByTestId('login-email').fill('designer@demo.local')
  await page.getByTestId('login-password').fill('demo1234')
  await page.getByTestId('login-submit').click()
  await expect(page).toHaveURL(/\/$|\/home/)
}

async function token(request: APIRequestContext): Promise<string> {
  const res = await request.post(`${API}/auth/login`, {
    data: {
      tenant_code: 'demo',
      email: 'designer@demo.local',
      password: 'demo1234',
    },
  })
  expect(res.ok()).toBeTruthy()
  return (await res.json()).access_token
}

test.describe('流程清單', () => {
  test('選單進得去，且列出既有流程', async ({ page }) => {
    await login(page)

    // 選單先前寫死指向 quotation_approval，使用者以為
    // 系統只能編輯那一個流程
    await page.goto('/designer/workflows')
    await expect(page.getByTestId('workflow-table')).toBeVisible()
    await expect(
      page.getByTestId('workflow-row-quotation_approval'),
    ).toBeVisible()

    await shot(page, '01-流程清單')
  })

  test('點列可進入該流程的設計器', async ({ page }) => {
    await login(page)
    await page.goto('/designer/workflows')

    await page.getByTestId('workflow-row-quotation_approval').click()
    await expect(page).toHaveURL(/\/designer\/workflows\/quotation_approval/)

    await shot(page, '02-進入設計器')
  })
})

test.describe('從零建立流程', () => {
  test('建立後進設計器，且該流程可以發布', async ({ page, request }) => {
    const key = `e2e_leave_${Date.now()}`

    await login(page)
    await page.goto('/designer/workflows')

    await page.getByTestId('workflow-create').click()
    await expect(page.getByTestId('create-dialog')).toBeVisible()
    await shot(page, '03-建立對話框')

    await page.getByTestId('new-workflow-name').fill('請假簽核流程')
    await page.getByTestId('new-workflow-key').fill(key)
    await page.getByTestId('new-business-object').fill('leave')
    await shot(page, '04-填寫完成')

    await page.getByTestId('create-confirm').click()

    // 建完直接進設計器，不要讓使用者自己再點一次
    await expect(page).toHaveURL(new RegExp(`/designer/workflows/${key}`))
    await shot(page, '05-建立後進入設計器')

    // ── 最關鍵的一條 ──────────────────────────────
    // 起始骨架必須通過 WF-E001～E012 才能發布。
    // 這條驗不過的話，使用者拿到的是一個建了卻用不了的流程。
    const jwt = await token(request)
    const publish = await request.post(
      `${API}/workflows/${key}/draft/publish`,
      { headers: { Authorization: `Bearer ${jwt}` } },
    )

    expect(
      publish.status(),
      `起始骨架應可直接發布，回應：${await publish.text()}`,
    ).toBe(200)
    expect((await publish.json()).version).toBe(1)
  })

  test('代碼格式不合時當場擋下，不送出', async ({ page }) => {
    await login(page)
    await page.goto('/designer/workflows')

    await page.getByTestId('workflow-create').click()
    await page.getByTestId('new-workflow-name').fill('格式測試')
    // workflow_key 會進 API 路徑與資料表關聯，事後不能改，
    // 所以要在送出前就擋下
    await page.getByTestId('new-workflow-key').fill('Leave-Approval')
    await page.getByTestId('create-confirm').click()

    await expect(page.getByTestId('create-error')).toBeVisible()
    // 沒有離開清單頁 = 沒有送出
    await expect(page).toHaveURL(/\/designer\/workflows$/)

    await shot(page, '06-代碼格式錯誤')
  })

  test('代碼重複時顯示是重複，而非泛用錯誤', async ({ page }) => {
    await login(page)
    await page.goto('/designer/workflows')

    await page.getByTestId('workflow-create').click()
    await page.getByTestId('new-workflow-name').fill('重複的流程')
    await page.getByTestId('new-workflow-key').fill('quotation_approval')
    await page.getByTestId('create-confirm').click()

    // 使用者需要知道是「這個代碼已經有人用了」，
    // 而不是「建立失敗，請稍後再試」——後者會讓他一直重試同一個代碼
    await expect(page.getByTestId('create-error')).toBeVisible()
    await expect(page).toHaveURL(/\/designer\/workflows$/)

    await shot(page, '07-代碼重複')
  })
})
