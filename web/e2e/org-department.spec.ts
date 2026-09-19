/**
 * 部門建立與編輯 E2E
 *
 * 補完健康檢查的閉環：報表裡的部門問題（「營運製造部沒有指定主管」）
 * 先前只能跳到組織管理，改不掉。
 *
 * 測試會建立部門並在結束時刪掉。空部門刪得掉，有成員的不行——
 * 兩個 FK 都是 `on delete set null`，放行的話那些人的
 * `department_id` 會被靜默清空。
 *
 * 需先啟動全部六個服務。
 */

import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/測試報告/202609/screenshots/org-department')
mkdirSync(SHOTS, { recursive: true })

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

async function openOrg(page: Page): Promise<void> {
  await page.goto('/designer/organization')
  await expect(page.getByTestId('org-employee-table')).toBeVisible()
}

test('建立部門後出現在樹上', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await openOrg(page)

  // 代碼帶時間戳：unique (tenant_id, code)，寫死的代碼第二次跑會衝突
  const code = `t${Date.now().toString().slice(-8)}`

  await page.getByTestId('dept-create').click()
  await expect(page.getByTestId('department-dialog')).toBeVisible()
  await shot(page, '01-建立部門')

  // 代碼與名稱都空白時不能按
  await expect(page.getByTestId('department-save')).toBeDisabled()

  await page.getByTestId('department-code').fill(code)
  await page.getByTestId('department-name').fill('測試部門')
  await expect(page.getByTestId('department-save')).toBeEnabled()
  await page.getByTestId('department-save').click()

  await expect(page.getByTestId(`dept-node-${code}`)).toBeVisible()

  // 收尾：刪掉它。空部門刪得掉，否則每跑一次測試就在樹上
  // 留一個垃圾，其他測試的截圖也會跟著變
  await page.getByTestId(`dept-node-${code}`).click()
  await page.getByTestId('dept-edit').click()
  await page.getByTestId('department-delete').click()
  await page.getByTestId('department-delete-confirm').click()
  await expect(page.getByTestId(`dept-node-${code}`)).toHaveCount(0)
})

test('刪除需要二次確認', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await openOrg(page)

  const code = `c${Date.now().toString().slice(-8)}`
  await page.getByTestId('dept-create').click()
  await page.getByTestId('department-code').fill(code)
  await page.getByTestId('department-name').fill('確認測試')
  await page.getByTestId('department-save').click()
  await expect(page.getByTestId(`dept-node-${code}`)).toBeVisible()

  await page.getByTestId(`dept-node-${code}`).click()
  await page.getByTestId('dept-edit').click()

  // 一按就刪會讓使用者誤刪——它不可逆
  await page.getByTestId('department-delete').click()
  await expect(page.getByTestId('department-delete-confirm')).toBeVisible()
  await shot(page, '04-刪除確認')

  // 取消後部門還在
  await page.getByRole('button', { name: '取消' }).first().click()
  await expect(page.getByTestId('department-delete-confirm')).toHaveCount(0)
  await expect(page.getByTestId('department-delete')).toBeVisible()

  // 收尾
  await page.getByTestId('department-delete').click()
  await page.getByTestId('department-delete-confirm').click()
  await expect(page.getByTestId(`dept-node-${code}`)).toHaveCount(0)
})

test('有成員的部門刪不掉，訊息說有幾個人', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await openOrg(page)

  await page.getByTestId('dept-node-sales').click()
  await page.getByTestId('dept-edit').click()
  await page.getByTestId('department-delete').click()
  await page.getByTestId('department-delete-confirm').click()

  // 放行的話那些人的 department_id 會被靜默清空，
  // 依部門解析的簽核人全部失效且沒有錯誤訊息
  await expect(page.getByTestId('department-dialog-error')).toContainText('成員')
  await shot(page, '05-有成員不可刪')

  await page.getByTestId('department-dialog-close').click()
  // 部門還在
  await expect(page.getByTestId('dept-node-sales')).toBeVisible()
})

test('代碼重複會顯示錯誤而不是靜默失敗', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await openOrg(page)

  await page.getByTestId('dept-create').click()
  // sales 是 seed 既有的部門代碼
  await page.getByTestId('department-code').fill('sales')
  await page.getByTestId('department-name').fill('重複的代碼')
  await page.getByTestId('department-save').click()

  await expect(page.getByTestId('department-dialog-error')).toBeVisible()
  await shot(page, '02-代碼重複')

  // 出錯時對話框要留著，讓使用者改代碼而不是重填一遍
  await expect(page.getByTestId('department-dialog')).toBeVisible()
  await page.getByTestId('department-dialog-close').click()
})

test('編輯部門時代碼不可改', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await openOrg(page)

  await page.getByTestId('dept-node-sales').click()
  await page.getByTestId('dept-edit').click()

  // code 是 unique 約束的一部分，也是同步匹配的備援鍵
  await expect(page.getByTestId('department-code')).toBeDisabled()
  await expect(page.getByTestId('department-name')).toBeEnabled()
})

test('上層部門的選項不含自己與下層，避免成環', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await openOrg(page)

  // 總經理室是根，底下有五個部門
  await page.getByTestId('dept-node-exec').click()
  await page.getByTestId('dept-edit').click()

  const options = await page
    .getByTestId('department-parent')
    .locator('option')
    .allInnerTexts()

  // 自己不在選項裡
  expect(options).not.toContain('總經理室')
  // 下層也不在——把根移到自己的下層會成環
  expect(options).not.toContain('業務一部')
})

test('健康檢查的部門問題可直接開啟該部門的編輯', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await page.goto('/designer/org-health')
  await expect(page.getByTestId('org-health-table')).toBeVisible()

  const deptIssue = page
    .locator('[data-testid^="org-health-row-DEPARTMENT_NO_MANAGER-"]')
    .first()
  await expect(deptIssue).toBeVisible()
  const deptName = await deptIssue
    .locator('td')
    .nth(1)
    .locator('p')
    .first()
    .innerText()

  await deptIssue.getByRole('link', { name: '前往修正' }).click()

  // 先前部門的問題只跳到組織管理，改不掉
  await expect(page).toHaveURL(/\/designer\/organization\?department=/)
  await expect(page.getByTestId('department-dialog')).toBeVisible()
  await expect(page.getByTestId('department-name')).toHaveValue(deptName.trim())
  await shot(page, '03-從健康檢查修部門')
})

test('指派部門主管後健康檢查的問題消失', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await page.goto('/designer/org-health')
  await expect(page.getByTestId('org-health-table')).toBeVisible()

  const before = await page
    .locator('[data-testid^="org-health-row-DEPARTMENT_NO_MANAGER-"]')
    .count()
  expect(before).toBeGreaterThan(0)

  // 修它
  await page
    .locator('[data-testid^="org-health-row-DEPARTMENT_NO_MANAGER-"]')
    .first()
    .getByRole('link', { name: '前往修正' })
    .click()
  await expect(page.getByTestId('department-dialog')).toBeVisible()

  // 挑一個不屬於這個部門的人當主管，避免觸發
  // DEPARTMENT_MANAGER_IS_SELF（換一個問題不算修好）
  await page.getByTestId('department-manager').selectOption({ label: '張文華' })
  await page.getByTestId('department-save').click()
  await expect(page.getByTestId('department-dialog')).toBeVisible()
  await page.getByTestId('department-dialog-close').click()

  // 報表要少一筆
  await page.goto('/designer/org-health')
  await expect(page.getByTestId('org-health-table')).toBeVisible()
  const after = await page
    .locator('[data-testid^="org-health-row-DEPARTMENT_NO_MANAGER-"]')
    .count()
  expect(after).toBe(before - 1)

  // 收尾：把主管清回未指定，還原 seed 的狀態
  await page.goto('/designer/organization')
  await expect(page.getByTestId('org-employee-table')).toBeVisible()
  await page.getByTestId('dept-node-mfg').click()
  await page.getByTestId('dept-edit').click()
  await page.getByTestId('department-manager').selectOption('')
  await page.getByTestId('department-save').click()
  await expect(page.getByTestId('unlock-field')).toHaveCount(1)
  // 編輯過的欄位會被鎖定，一併解開才算還原
  await page.getByTestId('unlock-field').click()
  await expect(page.getByTestId('unlock-field')).toHaveCount(0)
})

test('designer 看不到部門的建立與編輯', async ({ page }) => {
  await login(page, 'designer@demo.local')
  await openOrg(page)

  // 改部門主管等於改簽核路徑
  await expect(page.getByTestId('dept-create')).toHaveCount(0)
  await page.getByTestId('dept-node-sales').click()
  await expect(page.getByTestId('dept-edit')).toHaveCount(0)
})
