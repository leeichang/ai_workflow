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

/**
 * 送出登入表單
 *
 * 刻意不等導頁——密碼錯誤的測試要停在原頁看錯誤訊息。
 * 成功路徑請用 loginAndWait()。
 */
async function login(page: Page, who = CFO): Promise<void> {
  await page.getByTestId('login-tenant').fill(who.tenant)
  await page.getByTestId('login-email').fill(who.email)
  await page.getByTestId('login-password').fill(who.password)
  await page.getByTestId('login-submit').click()
}

/**
 * 登入並等到真的進了首頁
 *
 * 為什麼要自己指定 timeout：登入的密碼雜湊是 argon2，**刻意很慢**。
 * 實測 /auth/login 穩定落在 2.0～2.5 秒，而 Playwright 的 expect
 * 預設只等 5 秒——扣掉往返與重繪幾乎沒有餘裕。
 *
 * 全量執行時最常紅的就是登入後的第一個斷言，而且每次紅的是
 * 不同測試。那是餘裕不足的特徵，不是競態。
 *
 * 只放寬登入這一段而不調高全域 expect timeout：後者會讓每個
 * 真正失敗的斷言都慢 3 倍才回報。
 */
const LOGIN_TIMEOUT = 15_000

/**
 * @param landsOn 登入後應該到的位址。帶 redirect 參數進來時不是首頁。
 */
async function loginAndWait(
  page: Page,
  who = CFO,
  landsOn: string | RegExp = 'http://localhost:3040/',
): Promise<void> {
  await login(page, who)
  await expect(page).toHaveURL(landsOn, { timeout: LOGIN_TIMEOUT })
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
    await loginAndWait(page)

    await expect(page).toHaveURL('http://localhost:3040/')
    // 原本這裡寫死「林建志 協理」，現在必須是實際登入的張文華
    await expect(page.getByTestId('user-name')).toHaveText('張文華')
    await shot(page, '04-登入成功首頁')
  })

  test('登入後回到原本要去的頁面', async ({ page }) => {
    await page.goto('/designer/permissions')
    // 這條的重點就是「不會落在首頁」，所以要明確告訴 helper 預期的落點
    await loginAndWait(page, CFO, /\/designer\/permissions/)
    // 等畫面真的換過去，而不只是 URL 變了
    await expect(page.getByTestId('user-name')).toBeVisible()
    await shot(page, '05-登入後回原頁')
  })

  test('重新整理保持登入', async ({ page }) => {
    await page.goto('/login')
    await loginAndWait(page)
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
    await loginAndWait(page)
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

  // 表單與流程的選單改為指向清單頁（0→1 入口），不再直接開某一張定義。
  // 舊的斷言帶結尾斜線，是在釘「直接開 quotation_form」那個行為——
  // 固定指向某一張會讓使用者以為系統只能編輯那一張。
  test('表單設計器', async ({ page }) => {
    await page.getByTestId('nav-designer-toggle').click()
    await page.getByTestId('nav-designer-forms').click()
    await expect(page).toHaveURL(/\/designer\/forms$/)
    await expect(page.getByTestId('form-create')).toBeVisible()
    await shot(page, '11b-表單設計器')
  })

  test('流程設計器', async ({ page }) => {
    await page.getByTestId('nav-designer-toggle').click()
    await page.getByTestId('nav-designer-workflows').click()
    await expect(page).toHaveURL(/\/designer\/workflows$/)
    await expect(page.getByTestId('workflow-create')).toBeVisible()
    await shot(page, '11c-流程設計器')
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
    await loginAndWait(page)
    await page.getByTestId('user-menu-toggle').click()

    await expect(page.getByTestId('user-menu-panel')).toContainText('cfo@demo.local')
    await shot(page, '14-使用者選單')
  })

  test('登出清除狀態並導向登入頁', async ({ page }) => {
    await page.goto('/login')
    await loginAndWait(page)
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
    await loginAndWait(page)
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
    await loginAndWait(page, {
      tenant: 'demo',
      email: 'designer@demo.local',
      password: 'demo1234',
    })

    // 先等導頁完成再查 shell 的元素。
    //
    // login() 只按下送出，不等導頁。直接查 user-name 等於同時賭
    // 「登入成功」與「畫面已重繪」兩件事都在 expect 的 5 秒內完成，
    // 而這支測試先前就是全量執行時最常紅的一個。
    //
    // 分成兩段不是為了多等，是為了失敗時看得出是哪一段出問題——
    // 停在 /login 代表登入本身失敗，到了首頁但沒有 user-name
    // 才是渲染問題。其餘的 describe 在 beforeEach 都是這樣做的。
    await expect(page).toHaveURL('http://localhost:3040/')
    await expect(page.getByTestId('user-name')).toHaveText('陳雅婷')
    await shot(page, '17-切換為設計者')
  })
})
