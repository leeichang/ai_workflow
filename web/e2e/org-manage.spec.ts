/**
 * 組織管理 E2E
 *
 * 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §3、§12.2。
 *
 * 驗的三件事：
 *
 *   1. **欄位級 ownership 在畫面上看得見**（§3 規則 3）。
 *      編輯過的欄位要出現「平台維護」標記與「改回跟隨來源」。
 *      沒有這個標記，管理員不知道哪些欄位是自己補的、哪些還跟著 ERP。
 *
 *   2. **健康檢查的問題修得掉**。報表的「前往修正」要真的
 *      開到那個人的編輯——只能看不能修的報表沒有用。
 *
 *   3. **權限**。designer 進得來但不能改：改主管等於改簽核路徑。
 *
 * 測試會改動 demo 資料（職稱與電話），結束時改回去。
 *
 * 需先啟動全部六個服務。
 */

import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/測試報告/202609/screenshots/org-manage')
mkdirSync(SHOTS, { recursive: true })

/** 這個人是測試的對象。挑管理員本人以外的人，避免改到自己的權限 */
const TARGET = '黃志明'

async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

async function login(page: Page, email: string): Promise<void> {
  await page.goto('/login')
  await page.getByTestId('login-tenant').fill('demo')
  await page.getByTestId('login-email').fill(email)
  await page.getByTestId('login-password').fill('demo1234')
  await page.getByTestId('login-submit').click()
  await expect(page).toHaveURL(/\/$|\/home/)
}

/** 開啟某個員工的編輯對話框 */
async function openEditor(page: Page, name: string): Promise<void> {
  const row = page.locator('tr', { hasText: name }).first()
  await row.getByRole('button', { name: '編輯' }).click()
  await expect(page.getByTestId('employee-dialog')).toBeVisible()
}

test('部門樹與員工清單顯示完整的組織', async ({ page }) => {
  await login(page, 'admin@demo.local')

  await page.getByTestId('nav-designer-toggle').click()
  await page.getByRole('link', { name: '組織管理' }).click()
  await expect(page).toHaveURL(/\/designer\/organization/)

  await expect(page.getByTestId('org-employee-table')).toBeVisible()
  await shot(page, '01-組織管理')

  // demo 的部門樹：總經理室底下五個部門
  await expect(page.getByTestId('dept-node-exec')).toBeVisible()
  await expect(page.getByTestId('dept-node-sales')).toBeVisible()
  await expect(page.getByTestId('dept-node-qa')).toBeVisible()

  // 沒有主管的人要標成紅字——manager_of 會解析回空，流程卡住
  await expect(page.locator('[data-testid^="org-no-manager-"]').first()).toBeVisible()
})

test('選部門會連同下層部門的人一起顯示', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await page.goto('/designer/organization')
  await expect(page.getByTestId('org-employee-table')).toBeVisible()

  const rows = page.locator('[data-testid^="org-employee-row-"]')
  const total = await rows.count()

  // 總經理室是根，選它會看到整棵樹的人——但**不含沒有部門的人**。
  // demo 裡李一昌就沒有部門（健康檢查報成 EMPLOYEE_NO_DEPARTMENT），
  // 他不屬於任何子樹，所以會比「全部」少
  await page.getByTestId('dept-node-exec').click()
  const execCount = await rows.count()
  expect(execCount).toBeLessThanOrEqual(total)
  expect(execCount).toBeGreaterThan(1)

  // 業務一部是葉節點，只剩直屬成員
  await page.getByTestId('dept-node-sales').click()
  const salesCount = await rows.count()
  expect(salesCount).toBeLessThan(execCount)
  expect(salesCount).toBeGreaterThan(0)
})

test('編輯後欄位標記為平台維護，可改回跟隨來源', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await page.goto('/designer/organization')
  await expect(page.getByTestId('org-employee-table')).toBeVisible()

  await openEditor(page, TARGET)

  // 帶時間戳：儲存鈕只在值真的改變時才啟用，寫死的字串會在
  // 第二次跑測試時等於現值，按鈕就一直是 disabled
  const title = `品保經理-${Date.now()}`

  // 改職稱。只有改過的欄位會進 fields，也只有它會被鎖定
  await page.getByTestId('employee-job-title').fill(title)
  await page.getByTestId('employee-save').click()

  // 儲存後重新載入，標記要出現在職稱旁。
  // 只有職稱被鎖——整包送出會讓同步跳過使用者沒碰過的欄位
  await expect(page.getByTestId('unlock-field')).toHaveCount(1)
  // 對話框底部的說明文字也含「平台維護」，要指名標記本身
  await expect(
    page.getByTitle('此欄位由平台維護，同步時不會被來源系統覆蓋'),
  ).toBeVisible()
  await shot(page, '02-欄位鎖定標記')

  // 改回跟隨來源
  await page.getByTestId('unlock-field').click()
  await expect(page.getByTestId('unlock-field')).toHaveCount(0)

  // 收尾：清空職稱回到 seed 的狀態，再解一次鎖。
  // 不還原的話下一次跑測試會從髒資料開始——先前就是這樣
  // 讓「儲存鈕停用」的斷言在第二次跑時誤判
  await page.getByTestId('employee-job-title').fill('')
  await page.getByTestId('employee-save').click()
  await expect(page.getByTestId('unlock-field')).toHaveCount(1)
  await page.getByTestId('unlock-field').click()
  await expect(page.getByTestId('unlock-field')).toHaveCount(0)

  await page.getByTestId('employee-dialog-close').click()
})

test('沒有改動時儲存鈕停用', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await page.goto('/designer/organization')
  await expect(page.getByTestId('org-employee-table')).toBeVisible()

  await openEditor(page, TARGET)

  // 一打開就能按會讓使用者誤以為自己改了什麼
  await expect(page.getByTestId('employee-save')).toBeDisabled()

  // 帶時間戳，避免與資料庫現值相同而測不出啟用。
  // 這條測試不按儲存，值不會寫回去
  await page.getByTestId('employee-job-title').fill(`改了-${Date.now()}`)
  await expect(page.getByTestId('employee-save')).toBeEnabled()
})

test('主管下拉選單不含自己', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await page.goto('/designer/organization')
  await expect(page.getByTestId('org-employee-table')).toBeVisible()

  await openEditor(page, TARGET)

  // 後端也擋，但讓它連選都選不到比較好
  const options = await page
    .getByTestId('employee-manager')
    .locator('option')
    .allInnerTexts()
  expect(options).not.toContain(TARGET)
})

test('健康檢查的前往修正會開到該員工的編輯', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await page.goto('/designer/org-health')
  await expect(page.getByTestId('org-health-table')).toBeVisible()

  // 找一筆員工的問題。部門的問題沒有 employee 參數
  const employeeIssue = page
    .locator('[data-testid^="org-health-row-EMPLOYEE_NO_MANAGER-"]')
    .first()
  await expect(employeeIssue).toBeVisible()
  const subjectName = await employeeIssue
    .locator('td')
    .nth(1)
    .locator('p')
    .first()
    .innerText()

  await employeeIssue.getByRole('link', { name: '前往修正' }).click()

  // 報表要修得掉才有用——不該讓使用者自己再找一次那個人
  await expect(page).toHaveURL(/\/designer\/organization\?employee=/)
  await expect(page.getByTestId('employee-dialog')).toBeVisible()
  await expect(page.getByTestId('employee-name')).toHaveValue(subjectName.trim())
  await shot(page, '03-從健康檢查前往修正')
})

test('關掉對話框會清掉網址上的 employee 參數', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await page.goto('/designer/org-health')
  await expect(page.getByTestId('org-health-table')).toBeVisible()

  await page
    .locator('[data-testid^="org-health-row-EMPLOYEE_NO_MANAGER-"]')
    .first()
    .getByRole('link', { name: '前往修正' })
    .click()
  await expect(page.getByTestId('employee-dialog')).toBeVisible()

  await page.getByTestId('employee-dialog-close').click()

  // 留著參數的話重新整理又會跳出對話框
  await expect(page).toHaveURL(/\/designer\/organization$/)
  await expect(page.getByTestId('employee-dialog')).toHaveCount(0)
})

test('搜尋會過濾員工', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await page.goto('/designer/organization')
  await expect(page.getByTestId('org-employee-table')).toBeVisible()

  await page.getByTestId('org-search').fill(TARGET)

  const rows = page.locator('[data-testid^="org-employee-row-"]')
  await expect(rows).toHaveCount(1)
  await expect(rows.first()).toContainText(TARGET)
})

test('designer 看得到組織但不能編輯', async ({ page }) => {
  await login(page, 'designer@demo.local')
  await page.goto('/designer/organization')

  await expect(page.getByTestId('org-employee-table')).toBeVisible()
  // 改主管等於改簽核路徑，designer 管的是表單與流程定義，不是人事
  await expect(page.getByTestId('org-readonly-hint')).toBeVisible()
  await expect(page.getByRole('button', { name: '編輯' })).toHaveCount(0)
  await shot(page, '04-designer唯讀')
})
