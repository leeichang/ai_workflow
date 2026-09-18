/**
 * 操作手冊截圖：權限設定與報表
 *
 * 產生操作手冊用的截圖，不是驗證行為的測試。
 *
 * 執行：npx playwright test manual-03-permission
 * 截圖：docs/操作手冊/screenshots/03-權限/
 */

import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/操作手冊/screenshots/03-權限')
mkdirSync(SHOTS, { recursive: true })

async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

test.describe('操作手冊：權限與報表', () => {
  test.setTimeout(180_000)

  test('登入到權限設定完成', async ({ page }) => {
    // ══ 登入 ════════════════════════════════════════
    await page.goto('/login')
    await page.evaluate(() => localStorage.clear())
    await page.goto('/login')
    await expect(page.getByTestId('login-tenant')).toBeVisible()
    await shot(page, '01-登入頁')

    await page.getByTestId('login-tenant').fill('demo')
    await page.getByTestId('login-email').fill('designer@demo.local')
    await page.getByTestId('login-password').fill('demo1234')
    await page.getByTestId('login-submit').click()
    await expect(page).toHaveURL('http://localhost:3040/')
    await shot(page, '02-登入後首頁')

    // ══ 進入權限矩陣 ════════════════════════════════
    await page.getByTestId('nav-designer-toggle').click()
    await expect(page.getByTestId('nav-designer-permissions')).toBeVisible()
    await shot(page, '03-展開設計器選單')

    await page.getByTestId('nav-designer-permissions').click()
    await expect(page.getByTestId('permission-matrix')).toBeVisible({
      timeout: 15_000,
    })
    await shot(page, '04-權限矩陣')

    // ══ 選擇表單 ════════════════════════════════════
    await expect(page.getByTestId('form-picker')).toBeVisible()
    await page.getByTestId('form-picker').selectOption('quotation_form')
    await expect(page.locator('[data-testid^="row-"]').first()).toBeVisible({
      timeout: 15_000,
    })
    await shot(page, '05-選擇表單')

    // ══ 兩種檢視模式 ════════════════════════════════
    // 依角色：一次看一個角色在所有節點的權限
    await page.getByTestId('mode-by-role').click()
    await expect(page.getByTestId('role-picker')).toBeVisible()
    await expect(page.locator('[data-testid^="row-"]').first()).toBeVisible({
      timeout: 15_000,
    })
    await shot(page, '06-依角色檢視')

    await page.getByTestId('role-picker').selectOption('approver')
    await expect(page.locator('[data-testid^="row-"]').first()).toBeVisible({
      timeout: 15_000,
    })
    await shot(page, '07-切換角色')

    // ── 外部可見性標示 ──
    // 「客戶看不到」只出現在依角色模式的欄位清單，
    // 依節點模式是矩陣（欄位當欄、角色當列）沒有這個標記。
    // 報價單確實有這類欄位（內部備註、毛利率），用嚴格斷言——
    // 標示消失代表權限設定出了問題，不該靜默略過。
    const externalBadge = page.getByTestId('external-hidden-badge').first()
    await expect(externalBadge).toBeVisible()
    await externalBadge.scrollIntoViewIfNeeded()
    await shot(page, '08-外部不可見標示')

    // 依節點：一次看一個節點上所有角色的權限
    //
    // 切換模式會重新查詢。networkidle 不夠——它只保證網路靜止，
    // 不保證 Vue 已經把資料渲染出來，會截到「載入中」。
    // 等一個實際的資料列出現才是可靠的就緒訊號。
    await page.getByTestId('mode-by-node').click()
    await expect(page.locator('[data-testid^="row-"]').first()).toBeVisible({
      timeout: 15_000,
    })
    await shot(page, '09-依節點檢視')

    // ══ 欄位篩選 ════════════════════════════════════
    await page.getByTestId('field-filter').fill('客戶')
    await shot(page, '10-篩選欄位')
    await page.getByTestId('field-filter').fill('')

    // ══ 報表 ════════════════════════════════════════
    await page.getByTestId('nav-reports').click()
    await expect(page).toHaveURL(/\/reports/)
    await shot(page, '11-報表')
  })
})
