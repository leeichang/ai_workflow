/**
 * 操作手冊截圖：從零建立流程
 *
 * 產生操作手冊用的截圖，不是驗證行為的測試。
 * UI 改版時會跟著失敗——那正是要的：截圖過期會被抓到。
 *
 * 執行：npx playwright test manual-02-workflow
 * 截圖：docs/操作手冊/screenshots/02-流程/
 */

import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/操作手冊/screenshots/02-流程')
mkdirSync(SHOTS, { recursive: true })

async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

const WF_KEY = `leave_approval_${Date.now()}`

test.describe('操作手冊：從零建立流程', () => {
  test.setTimeout(180_000)

  test('登入到流程設計完成', async ({ page }) => {
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

    // ══ 進入流程清單 ════════════════════════════════
    await page.getByTestId('nav-designer-toggle').click()
    await expect(page.getByTestId('nav-designer-workflows')).toBeVisible()
    await shot(page, '03-展開設計器選單')

    await page.getByTestId('nav-designer-workflows').click()
    await expect(page).toHaveURL(/\/designer\/workflows$/)
    await expect(page.getByTestId('workflow-create')).toBeVisible()
    await shot(page, '04-流程清單')

    // ══ 建立新流程 ══════════════════════════════════
    await page.getByTestId('workflow-create').click()
    await expect(page.getByTestId('create-dialog')).toBeVisible()
    await shot(page, '05-建立流程對話框')

    await page.getByTestId('new-workflow-name').fill('請假簽核流程')
    await page.getByTestId('new-workflow-key').fill(WF_KEY)
    await page.getByTestId('new-business-object').fill('quotation')
    await shot(page, '06-填寫流程資訊')

    await page.getByTestId('create-confirm').click()

    const createErr = page.getByTestId('create-error')
    if (await createErr.isVisible().catch(() => false)) {
      throw new Error(`建立失敗：${await createErr.textContent()}`)
    }

    // ══ 起始畫布 ════════════════════════════════════
    // 新流程是最小骨架：起點直接連終點
    await expect(page).toHaveURL(
      new RegExp(`/designer/workflows/${WF_KEY}$`),
      { timeout: 15_000 },
    )
    await expect(page.getByTestId('workflow-canvas')).toBeVisible({
      timeout: 15_000,
    })
    await shot(page, '07-新流程的起始畫面')

    // ══ 插入第一個簽核節點 ══════════════════════════
    // 點連線上的「+」選擇要加入的節點類型
    await page.getByTestId('insert-start-end').click()
    await expect(page.getByTestId('insert-hint')).toBeVisible()
    await expect(page.getByTestId('node-palette')).toBeVisible()
    await shot(page, '08-選取插入點')

    await page.getByTestId('node-palette-human_approval').click()
    await expect(page.getByTestId('node-property-panel')).toBeVisible()
    await shot(page, '09-加入簽核節點')

    // 設定節點名稱與簽核人
    await page.getByTestId('node-prop-label').fill('部門主管簽核')
    await shot(page, '10-設定節點名稱')

    // resolver 決定誰來簽。這是流程設計最關鍵的一步——
    // 設錯的話流程會因為找不到參與者而 FAILED。
    await page.getByTestId('node-prop-resolver-type').selectOption('role')
    await expect(page.getByTestId('node-prop-resolver-value')).toBeVisible()
    await shot(page, '11-選擇簽核人類型')

    await page.getByTestId('node-prop-resolver-value').fill('approver')
    await shot(page, '12-指定簽核角色')

    // ══ 逾時設定 ════════════════════════════════════
    await page.getByTestId('node-prop-timeout-after').fill('P3D')
    await expect(page.getByTestId('node-prop-timeout-policy')).toBeVisible()
    await shot(page, '13-設定逾時期限')

    await page
      .getByTestId('node-prop-timeout-policy')
      .selectOption('AUTO_REJECT')
    await shot(page, '14-設定逾時處理方式')

    // 到期前提醒。可設多個時間點，逗號分隔。
    await page.getByTestId('node-prop-remind-at').fill('P2D, P1D, PT6H')
    await shot(page, '15-設定到期前提醒')

    // ══ 插入條件分支 ════════════════════════════════
    await page.getByTestId('insert-start-approval_1').click()
    await expect(page.getByTestId('node-palette')).toBeVisible()
    await shot(page, '16-在簽核前插入節點')

    await page.getByTestId('node-palette-condition').click()
    await expect(page.getByTestId('node-property-panel')).toBeVisible()
    await page.getByTestId('node-prop-label').fill('請假天數判斷')
    await shot(page, '17-加入條件節點')

    // 條件式引用的路徑必須存在於業務物件定義，否則發布時
    // WF-E012 會擋下——路徑打錯會讓流程靜默跳過簽核。
    await page
      .getByTestId('node-prop-expression')
      .fill('quotation.discount_rate > 0.15')
    await shot(page, '18-設定條件式')

    // ══ 驗證與存檔 ══════════════════════════════════
    await page.getByTestId('validate').click()
    await shot(page, '19-驗證流程')

    await page.getByTestId('save-draft').click()
    await expect(page.getByTestId('save-status')).toBeVisible()
    await shot(page, '20-儲存草稿')

    // ══ 發布 ════════════════════════════════════════
    await page.getByTestId('publish').click()
    await shot(page, '21-發布流程')

    await page.goto('/designer/workflows')
    await expect(page.getByTestId(`workflow-row-${WF_KEY}`)).toBeVisible({
      timeout: 15_000,
    })
    await shot(page, '22-清單中的新流程')
  })
})
