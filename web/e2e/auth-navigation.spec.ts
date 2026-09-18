/**
 * 登入登出與選單導覽 E2E
 *
 * 驗證兩個先前不存在的功能：
 *   1. 登入登出（原本由 dev-auth.ts 自動登入，沒有登入頁）
 *   2. 選單導覽（原本七個項目只有一個有對應路由，其餘被 catch-all 導走）
 *
 * 每個步驟都截圖，存到 docs/測試報告/202609/screenshots/auth-navigation/。
 *
 * 需先啟動：PostgreSQL、Rust API (:3001)、前端 (:3040)。
 */

import { expect, test, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

// 專案是 ESM，沒有 __dirname
const HERE = dirname(fileURLToPath(import.meta.url))

const SHOTS = resolve(
  HERE,
  '../../docs/測試報告/202609/screenshots/auth-navigation',
)

mkdirSync(SHOTS, { recursive: true })

/**
 * 截圖
 *
 * 先等網路靜止再拍。toHaveURL 這類斷言會重試到通過為止，
 * 但截圖是立即執行的——URL 已經變了而畫面還停在「登入中…」，
 * 拍出來的就是上一頁。
 */
async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png`, fullPage: false })
}

const CFO = { tenant: 'demo', email: 'cfo@demo.local', password: 'demo1234' }

async function login(page: Page, who = CFO): Promise<void> {
  await page.getByTestId('login-tenant').fill(who.tenant)
  await page.getByTestId('login-email').fill(who.email)
  await page.getByTestId('login-password').fill(who.password)
  await page.getByTestId('login-submit').click()
}

test.describe('登入', () => {
  test('未登入時任何頁面都導向登入頁', async ({ page }) => {
    await page.goto('/tasks')
    await expect(page).toHaveURL(/\/login/)
    await expect(page.getByTestId('login-form')).toBeVisible()
    await shot(page, '01-未登入導向登入頁')
  })

  test('原本要去的路徑記在 redirect', async ({ page }) => {
    await page.goto('/designer/permissions')
    await expect(page).toHaveURL(/redirect=%2Fdesigner%2Fpermissions|redirect=\/designer\/permissions/)
    await shot(page, '02-redirect參數')
  })

  test('密碼錯誤顯示訊息且只清密碼', async ({ page }) => {
    await page.goto('/login')
    await login(page, { ...CFO, password: 'wrongpassword' })

    await expect(page.getByTestId('login-error')).toContainText('帳號或密碼錯誤')
    // 租戶與帳號保留，讓使用者只需改密碼
    await expect(page.getByTestId('login-tenant')).toHaveValue('demo')
    await expect(page.getByTestId('login-email')).toHaveValue('cfo@demo.local')
    await expect(page.getByTestId('login-password')).toHaveValue('')
    await shot(page, '03-密碼錯誤')
  })

  test('登入成功進入首頁，顯示真實使用者', async ({ page }) => {
    await page.goto('/login')
    await login(page)

    await expect(page).toHaveURL('http://localhost:3040/')
    // 原本這裡寫死「林建志 協理」，現在必須是實際登入的張文華
    await expect(page.getByTestId('user-name')).toHaveText('張文華')
    await shot(page, '04-登入成功首頁')
  })

  test('登入後回到原本要去的頁面', async ({ page }) => {
    await page.goto('/designer/permissions')
    await login(page)
    await expect(page).toHaveURL(/\/designer\/permissions/)
    // 等畫面真的換過去，而不只是 URL 變了
    await expect(page.getByTestId('user-name')).toBeVisible()
    await shot(page, '05-登入後回原頁')
  })

  test('重新整理保持登入', async ({ page }) => {
    await page.goto('/login')
    await login(page)
    await expect(page).toHaveURL('http://localhost:3040/')

    await page.reload()
    await expect(page).toHaveURL('http://localhost:3040/')
    await expect(page.getByTestId('user-name')).toHaveText('張文華')
    await shot(page, '06-重新整理保持登入')
  })
})

test.describe('選單導覽', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/login')
    await login(page)
    await expect(page).toHaveURL('http://localhost:3040/')
  })

  test('首頁顯示工作概況', async ({ page }) => {
    await expect(page.getByTestId('home-greeting')).toContainText('張文華')
    await expect(page.getByTestId('home-card-pending')).toBeVisible()
    await shot(page, '07-首頁')
  })

  test('我的待辦', async ({ page }) => {
    await page.getByTestId('nav-tasks').click()
    await expect(page).toHaveURL(/\/tasks/)
    // 收件匣可能是空的，兩種狀態都算正確
    await expect(
      page.getByTestId('inbox-empty').or(page.locator('[data-testid^="task-row-"]').first()),
    ).toBeVisible()
    await shot(page, '08-我的待辦')
  })

  test('報價單列出流程實例', async ({ page }) => {
    await page.getByTestId('nav-quotations').click()
    await expect(page).toHaveURL(/\/quotations/)
    await expect(
      page.getByTestId('quotation-table').or(page.getByTestId('quotation-empty')),
    ).toBeVisible()
    await shot(page, '09-報價單')
  })

  test('採購申請顯示未實作說明', async ({ page }) => {
    await page.getByTestId('nav-purchase-requests').click()
    await expect(page).toHaveURL(/\/purchase-requests/)
    // 重點：不是被 catch-all 導回設計器，而是明確說明缺什麼
    await expect(page.getByTestId('not-built')).toBeVisible()
    await shot(page, '10-採購申請未實作')
  })

  test('設計器展開四個子項目', async ({ page }) => {
    await page.getByTestId('nav-designer-toggle').click()

    await expect(page.getByTestId('nav-designer-forms')).toBeVisible()
    await expect(page.getByTestId('nav-designer-workflows')).toBeVisible()
    await expect(page.getByTestId('nav-designer-templates')).toBeVisible()
    await expect(page.getByTestId('nav-designer-permissions')).toBeVisible()
    await shot(page, '11a-設計器子選單')
  })

  // 選單進的是清單頁（/designer/forms）而非直接開某一張
  // （/designer/forms/quotation_form）。正規式不能要求結尾的斜線。
  test('表單清單', async ({ page }) => {
    await page.getByTestId('nav-designer-toggle').click()
    await page.getByTestId('nav-designer-forms').click()
    await expect(page).toHaveURL(/\/designer\/forms$/)
    await expect(page.getByTestId('form-create')).toBeVisible()
    await shot(page, '11b-表單清單')
  })

  test('流程清單', async ({ page }) => {
    await page.getByTestId('nav-designer-toggle').click()
    await page.getByTestId('nav-designer-workflows').click()
    await expect(page).toHaveURL(/\/designer\/workflows$/)
    await expect(page.getByTestId('workflow-create')).toBeVisible()
    await shot(page, '11c-流程清單')
  })

  test('單據套版設計器', async ({ page }) => {
    await page.getByTestId('nav-designer-toggle').click()
    await page.getByTestId('nav-designer-templates').click()
    await expect(page).toHaveURL(/\/designer\/templates/)
    await shot(page, '11d-單據套版設計器')
  })

  test('權限矩陣', async ({ page }) => {
    await page.getByTestId('nav-designer-toggle').click()
    await page.getByTestId('nav-designer-permissions').click()
    await expect(page).toHaveURL(/\/designer\/permissions/)
    await shot(page, '11e-權限矩陣')
  })

  test('報表顯示未實作說明', async ({ page }) => {
    await page.getByTestId('nav-reports').click()
    await expect(page).toHaveURL(/\/reports/)
    await expect(page.getByTestId('not-built')).toBeVisible()
    await shot(page, '12-報表未實作')
  })

  test('設定顯示未實作說明', async ({ page }) => {
    await page.getByTestId('nav-settings').click()
    await expect(page).toHaveURL(/\/settings/)
    await expect(page.getByTestId('not-built')).toBeVisible()
    await shot(page, '13-設定未實作')
  })

  test('所有選單項目都可導覽，無一被彈回', async ({ page }) => {
    const paths = [
      ['nav-tasks', '/tasks'],
      ['nav-quotations', '/quotations'],
      ['nav-purchase-requests', '/purchase-requests'],
      ['nav-reports', '/reports'],
      ['nav-settings', '/settings'],
      ['nav-home', '/'],
    ] as const

    for (const [testid, expected] of paths) {
      await page.getByTestId(testid).click()
      await expect(page).toHaveURL(new RegExp(expected.replace(/\//g, '\\/')))
    }

    // 四個設計器
    await page.getByTestId('nav-designer-toggle').click()
    // 表單與流程進清單頁，網址結尾沒有斜線
    const designers = [
      ['nav-designer-forms', '/designer/forms'],
      ['nav-designer-workflows', '/designer/workflows'],
      ['nav-designer-templates', '/designer/templates'],
      ['nav-designer-permissions', '/designer/permissions'],
    ] as const

    for (const [testid, expected] of designers) {
      await page.getByTestId(testid).click()
      await expect(page).toHaveURL(new RegExp(expected.replace(/\//g, '\\/')))
    }
  })
})

test.describe('登出', () => {
  test('選單展開顯示帳號資訊', async ({ page }) => {
    await page.goto('/login')
    await login(page)
    await page.getByTestId('user-menu-toggle').click()

    await expect(page.getByTestId('user-menu-panel')).toContainText('cfo@demo.local')
    await shot(page, '14-使用者選單')
  })

  test('登出清除狀態並導向登入頁', async ({ page }) => {
    await page.goto('/login')
    await login(page)
    await page.getByTestId('user-menu-toggle').click()
    await page.getByTestId('logout-button').click()

    await expect(page).toHaveURL(/\/login/)
    const stored = await page.evaluate(() =>
      localStorage.getItem('workflow.session'),
    )
    expect(stored).toBeNull()
    await shot(page, '15-登出')
  })

  test('登出後原本可進的頁面需重新登入', async ({ page }) => {
    await page.goto('/login')
    await login(page)
    await page.getByTestId('user-menu-toggle').click()
    await page.getByTestId('logout-button').click()
    await expect(page).toHaveURL(/\/login/)

    await page.goto('/tasks')
    await expect(page).toHaveURL(/\/login/)
    await shot(page, '16-登出後需重新登入')
  })
})

test.describe('不同使用者', () => {
  test('切換帳號後顯示對應的人與角色', async ({ page }) => {
    await page.goto('/login')
    await login(page, {
      tenant: 'demo',
      email: 'designer@demo.local',
      password: 'demo1234',
    })

    await expect(page.getByTestId('user-name')).toHaveText('陳雅婷')
    await shot(page, '17-切換為設計者')
  })
})
