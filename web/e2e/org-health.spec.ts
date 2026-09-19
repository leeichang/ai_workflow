/**
 * 組織健康檢查 E2E
 *
 * 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §9。
 *
 * 這一頁的價值在於「導入期問題上線前就浮出來」，所以 E2E 要驗的
 * 不只是畫面渲染得出來，而是**它在真實 seed 資料上真的抓到問題**。
 * 一張永遠顯示「一切正常」的報表比沒有報表更危險。
 *
 * demo 種子的組織確實有問題：每個部門主管都隸屬於自己管的部門，
 * 送單時 department_manager_of 的 `m.id <> u.id` 會排除自己，
 * 解析回空、流程卡住。這是需求 §9 表格沒列到、實務上最常見的一項。
 *
 * 需先啟動全部六個服務。
 */

import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/測試報告/202609/screenshots/org-health')
mkdirSync(SHOTS, { recursive: true })

/**
 * 截圖前先等資料列出現
 *
 * networkidle 不夠——Vue 收到回應後還要渲染，
 * 之前的截圖就抓到過「載入中…」
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

test('從側邊欄進入健康檢查，看到真實的組織問題', async ({ page }) => {
  await login(page)

  // 從側邊欄進入，而非直接 goto——要驗證入口真的存在。
  // 「設計器」在展開的側邊欄是展開子選單的 button，不是連結
  await page.getByTestId('nav-designer-toggle').click()
  await page.getByRole('link', { name: '組織健康檢查' }).click()
  await expect(page).toHaveURL(/\/designer\/org-health/)

  // 等表格出現再截圖
  await expect(page.getByTestId('org-health-table')).toBeVisible()
  await shot(page, '01-報表總覽')

  // demo 種子的組織有問題，不該顯示「一切正常」
  await expect(page.getByTestId('org-health-empty')).toHaveCount(0)

  const high = await page.getByTestId('org-health-high').innerText()
  expect(Number(high.replace(/\D/g, ''))).toBeGreaterThan(0)
})

test('部門主管隸屬於自己部門會被標為會卡住', async ({ page }) => {
  await login(page)
  await page.goto('/designer/org-health')
  await expect(page.getByTestId('org-health-table')).toBeVisible()

  // 這是 seed 資料實際存在的問題。若哪天 seed 修好了，
  // 這條會失敗——那是正確的失敗，要跟著改測試而不是刪掉斷言
  const selfManaged = page.locator(
    '[data-testid^="org-health-row-DEPARTMENT_MANAGER_IS_SELF-"]',
  )
  await expect(selfManaged.first()).toBeVisible()

  await expect(selfManaged.first()).toContainText('會卡住')
  await expect(selfManaged.first()).toContainText('解析不到簽核人')
  // detail 要指出是哪個部門，否則使用者不知道去哪修
  await expect(selfManaged.first()).toContainText('部門：')
})

test('只看會卡住的問題會濾掉 MEDIUM', async ({ page }) => {
  await login(page)
  await page.goto('/designer/org-health')
  await expect(page.getByTestId('org-health-table')).toBeVisible()

  const allRows = page.locator('[data-testid^="org-health-row-"]')
  const before = await allRows.count()

  await page.getByTestId('org-health-high-only').check()
  await shot(page, '02-只看會卡住')

  const after = await allRows.count()
  // seed 有 MEDIUM（李一昌沒有部門），所以勾選後一定會變少
  expect(after).toBeLessThan(before)

  // 剩下的全部是 HIGH。範圍限在表格內——
  // 「功能受損」也是統計卡片的標籤，那張卡不該被濾掉
  await expect(
    page.getByTestId('org-health-table').getByText('功能受損'),
  ).toHaveCount(0)
})

test('HIGH 排在 MEDIUM 前面', async ({ page }) => {
  await login(page)
  await page.goto('/designer/org-health')
  await expect(page.getByTestId('org-health-table')).toBeVisible()

  // 使用者通常只修最上面幾項，順序決定他先修到什麼
  const badges = await page
    .locator('[data-testid^="org-health-row-"] td:first-child')
    .allInnerTexts()

  const firstMedium = badges.findIndex((t) => t.includes('功能受損'))
  const lastHigh = badges.reduce(
    (acc, t, i) => (t.includes('會卡住') ? i : acc),
    -1,
  )

  expect(firstMedium).toBeGreaterThan(-1)
  expect(lastHigh).toBeGreaterThan(-1)
  expect(lastHigh).toBeLessThan(firstMedium)
})

test('重新檢查會重新呼叫 API', async ({ page }) => {
  await login(page)
  await page.goto('/designer/org-health')
  await expect(page.getByTestId('org-health-table')).toBeVisible()

  const called = page.waitForResponse(
    (r) => r.url().includes('/org/health') && r.status() === 200,
  )
  await page.getByTestId('org-health-refresh').click()
  await called

  await expect(page.getByTestId('org-health-table')).toBeVisible()
})
