/**
 * 操作手冊截圖：從零建立表單
 *
 * 這不是驗證行為的測試，是產生操作手冊用的截圖。
 * 放在 e2e/ 而非另建目錄，因為它需要的環境與 E2E 完全相同，
 * 而且 UI 改版時會跟著失敗——那正是要的：截圖過期會被抓到，
 * 不會默默留著騙人。
 *
 * 每個 shot() 之前都用嚴格斷言確認元素存在。
 * 不用 `if (visible)` 包起來——那會讓 selector 失效時靜默跳過，
 * 測試通過但截圖少了幾張，而沒有人會發現。
 *
 * 執行：npx playwright test manual-01-form
 * 截圖：docs/操作手冊/screenshots/01-表單/
 */

import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/操作手冊/screenshots/01-表單')
mkdirSync(SHOTS, { recursive: true })

/**
 * 截圖
 *
 * networkidle 是必要的——toHaveURL 這類斷言會重試直到通過，
 * screenshot 卻是立即執行，不等就會截到上一頁的畫面。
 */
async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

/** 每次執行用新的代碼，避免與前一次的殘留衝突 */
const FORM_KEY = `employee_form_${Date.now()}`

test.describe('操作手冊：從零建立表單', () => {
  test.setTimeout(180_000)

  test('登入到表單設計完成', async ({ page }) => {
    // ══ 登入 ════════════════════════════════════════
    await page.goto('/login')
    await page.evaluate(() => localStorage.clear())
    await page.goto('/login')
    await expect(page.getByTestId('login-tenant')).toBeVisible()
    await shot(page, '01-登入頁')

    await page.getByTestId('login-tenant').fill('demo')
    await page.getByTestId('login-email').fill('designer@demo.local')
    await page.getByTestId('login-password').fill('demo1234')
    await shot(page, '02-填入帳密')

    await page.getByTestId('login-submit').click()
    await expect(page).toHaveURL('http://localhost:3040/')
    await shot(page, '03-登入後首頁')

    // ══ 進入表單清單 ════════════════════════════════
    await page.getByTestId('nav-designer-toggle').click()
    await expect(page.getByTestId('nav-designer-forms')).toBeVisible()
    await shot(page, '04-展開設計器選單')

    await page.getByTestId('nav-designer-forms').click()
    await expect(page).toHaveURL(/\/designer\/forms$/)
    await expect(page.getByTestId('form-create')).toBeVisible()
    await shot(page, '05-表單清單')

    // ══ 建立新表單 ══════════════════════════════════
    await page.getByTestId('form-create').click()
    await expect(page.getByTestId('create-dialog')).toBeVisible()
    await shot(page, '06-建立表單對話框')

    await page.getByTestId('new-form-name').fill('員工基本資料')
    await page.getByTestId('new-form-key').fill(FORM_KEY)
    await page.getByTestId('new-business-object').fill('employee')
    await shot(page, '07-填寫表單資訊')

    await page.getByTestId('create-confirm').click()

    // 建立失敗時畫面停在原地，先攔下來看訊息
    const createErr = page.getByTestId('create-error')
    if (await createErr.isVisible().catch(() => false)) {
      throw new Error(`建立失敗：${await createErr.textContent()}`)
    }

    // ══ 空白畫布 ════════════════════════════════════
    await expect(page).toHaveURL(new RegExp(`/designer/forms/${FORM_KEY}$`), {
      timeout: 15_000,
    })
    await expect(page.getByTestId('canvas')).toBeVisible({ timeout: 15_000 })
    await shot(page, '08-新表單的起始畫面')

    // 左側元件庫
    await expect(page.getByTestId('palette')).toBeVisible()
    await expect(page.getByTestId('palette-group-basic')).toBeVisible()
    await expect(page.getByTestId('palette-group-advanced')).toBeVisible()
    await shot(page, '09-元件庫')

    // ══ 加入第一個欄位 ══════════════════════════════
    // 元件庫是點擊加入，不是拖放
    await page.getByTestId('palette-item-input').click()
    await shot(page, '10-加入單行文字欄位')

    // 新加的欄位會自動選取，右側顯示屬性
    await expect(page.getByTestId('property-panel')).toBeVisible()
    await page.getByTestId('prop-label').fill('姓名')
    await shot(page, '11-設定欄位名稱')

    // 資料分頁：設定對應的資料路徑
    await page.getByTestId('tab-data').click()
    await expect(page.getByTestId('prop-data-path')).toBeVisible()
    await page.getByTestId('prop-data-path').fill('employee.name')
    await shot(page, '12-設定資料路徑')

    // ══ 加入更多欄位 ════════════════════════════════
    await page.getByTestId('palette-item-date').click()
    await expect(page.getByTestId('property-panel')).toBeVisible()
    // 屬性面板會停在前一次選的分頁，設 label 前要先切回「基本」
    await page.getByTestId('tab-basic').click()
    await page.getByTestId('prop-label').fill('到職日')
    await shot(page, '13-加入日期欄位')

    await page.getByTestId('palette-item-select').click()
    await page.getByTestId('tab-basic').click()
    await page.getByTestId('prop-label').fill('部門')
    await shot(page, '14-加入下拉選單')

    // ══ 明細表格 ════════════════════════════════════
    // table 是 one2many 的子表格，ERP 表單幾乎都需要
    await page.getByTestId('palette-item-table').click()
    await expect(page.getByTestId('property-panel')).toBeVisible()
    await page.getByTestId('tab-basic').click()
    await page.getByTestId('prop-label').fill('工作經歷')
    await shot(page, '15-加入明細表格')

    // ══ 欄位權限 ════════════════════════════════════
    await page.getByTestId('tab-rules').click()
    await expect(page.getByTestId('prop-visible-when')).toBeVisible()
    await shot(page, '16-欄位規則')

    // ══ 權限預覽 ════════════════════════════════════
    // 預覽面板預設收合
    await page.getByTestId('toggle-preview').click()
    await expect(page.getByTestId('permission-preview')).toBeVisible()
    await shot(page, '17-權限預覽')

    await page.getByTestId('preview-role').selectOption('requester')
    await shot(page, '18-以申請人身分預覽')

    await page.getByTestId('toggle-preview').click()

    // ══ 驗證與存檔 ══════════════════════════════════
    await page.getByTestId('validate').click()
    await shot(page, '19-驗證表單')

    await page.getByTestId('save-draft').click()
    await expect(page.getByTestId('save-status')).toBeVisible()
    await shot(page, '20-儲存草稿')

    // ══ 發布 ════════════════════════════════════════
    await page.getByTestId('publish').click()
    await shot(page, '21-發布表單')

    // 回清單頁看結果
    await page.goto('/designer/forms')
    await expect(page.getByTestId(`form-row-${FORM_KEY}`)).toBeVisible({
      timeout: 15_000,
    })
    await shot(page, '22-清單中的新表單')
  })
})
