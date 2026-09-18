/**
 * 操作手冊截圖：端到端實作
 *
 * 從畫面建立報價單 → 送簽 → 逐關簽核 → 完成。
 * 與其他三支不同：這支**用畫面操作建單**，不走 API 捷徑，
 * 因為操作手冊要教的就是使用者實際會做的事。
 *
 * 簽核人由 resolver 依組織結構解析，不是寫死的：
 *   manager_of  發起人的主管。sales 的主管是 designer（陳雅婷）
 *   role        指定角色。財務簽核走 finance_manager
 *
 * 用 API 建單時發起人是 designer，從畫面建單時發起人是 sales——
 * 兩者的主管不同，待辦會落在不同人身上。這支走畫面，所以第一關是
 * designer 而非 cfo。
 *
 * 執行：npx playwright test manual-04-endtoend
 * 截圖：docs/操作手冊/screenshots/04-端到端/
 */

import {
  expect,
  test,
  type Page,
  type APIRequestContext,
} from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { cancelByBusinessKey } from './support/instances'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/操作手冊/screenshots/04-端到端')
mkdirSync(SHOTS, { recursive: true })

const API = 'http://localhost:3001'

async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

async function login(page: Page, email: string): Promise<void> {
  await page.goto('/login')
  await page.evaluate(() => localStorage.clear())
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

/**
 * 等待指定單號的待辦出現
 *
 * 待辦是 Python Worker 走到節點後才透過 internal API 建立的，
 * 送簽後立刻查一定查不到。
 */
async function waitForTask(
  request: APIRequestContext,
  email: string,
  businessKey: string,
  timeoutMs = 40_000,
): Promise<{ id: string }> {
  const jwt = await token(request, email)
  const deadline = Date.now() + timeoutMs

  while (Date.now() < deadline) {
    const res = await request.get(`${API}/tasks?status=PENDING&limit=50`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    const tasks = await res.json()
    const found = tasks.find(
      (t: { business_key: string }) => t.business_key === businessKey,
    )
    if (found !== undefined) return found
    await new Promise((r) => setTimeout(r, 500))
  }
  throw new Error(`等待 ${timeoutMs}ms 仍未出現 ${businessKey} 的待辦`)
}

/** 單號帶時間戳。Temporal 以 workflow_id 去重，重複的單號會被拒絕 */
const KEY = `QT-${Date.now()}`

// 這張單從畫面建立，走不到最後一關（客戶簽核沒有登入管道），
// 不收掉的話會永遠留在 Temporal 輪詢。見 support/instances.ts。
test.afterAll(async ({ request }) => {
  const jwt = await token(request, 'designer@demo.local')
  await cancelByBusinessKey(request, jwt, KEY)
})

test.describe('操作手冊：端到端', () => {
  test.setTimeout(240_000)

  test('建立報價單、送簽、逐關簽核', async ({ page, request }) => {
    // ══ 第一段：業務建立報價單 ══════════════════════
    await page.goto('/login')
    await page.evaluate(() => localStorage.clear())
    await page.goto('/login')
    await expect(page.getByTestId('login-tenant')).toBeVisible()
    await shot(page, '01-登入頁')

    await page.getByTestId('login-tenant').fill('demo')
    await page.getByTestId('login-email').fill('sales@demo.local')
    await page.getByTestId('login-password').fill('demo1234')
    await shot(page, '02-業務帳號登入')

    await page.getByTestId('login-submit').click()
    await expect(page).toHaveURL('http://localhost:3040/')
    await shot(page, '03-業務首頁')

    await page.getByTestId('nav-quotations').click()
    await expect(page.getByTestId('quotation-create')).toBeVisible()
    await shot(page, '04-報價單列表')

    await page.getByTestId('quotation-create').click()
    await expect(page.getByTestId('field-business_key')).toBeVisible()
    await shot(page, '05-新增報價單')

    // ── 填寫表頭 ────────────────────────────────────
    await page.getByTestId('field-business_key').fill(KEY)
    await page
      .getByTestId('field-customer_name')
      .fill('台灣松下精密機械股份有限公司')
    await shot(page, '06-填寫客戶資料')

    // 折扣率填小數不是百分比：0.2 代表 20%。
    // 超過 15%（0.15）門檻會多走一關財務簽核——這正是要示範的分支。
    await page.getByTestId('field-discount_rate').fill('0.2')
    await shot(page, '07-填寫折扣率')

    // ── 填寫明細 ────────────────────────────────────
    await expect(page.getByTestId('lines-table')).toBeVisible()
    await page.getByTestId('lines-add').click()
    await shot(page, '08-新增明細列')

    await page.getByTestId('line-0-part_no').fill('PA-3021 主軸座')
    await page.getByTestId('line-0-drawing_no').fill('DWG-3021-C')
    await page.getByTestId('line-0-material').fill('SCM440 調質')
    await page.getByTestId('line-0-qty').fill('50')
    await page.getByTestId('line-0-unit').fill('PCS')
    await page.getByTestId('line-0-material_cost').fill('1850')
    await page.getByTestId('line-0-process_cost').fill('2400')
    // 單位售價是金額計算的依據，材料費與加工費只用來算毛利率
    await page.getByTestId('line-0-target_price').fill('5200')
    await shot(page, '09-填寫明細內容')

    await page.getByTestId('lines-add').click()
    await page.getByTestId('line-1-part_no').fill('PA-3022 軸承蓋')
    await page.getByTestId('line-1-drawing_no').fill('DWG-3022-A')
    await page.getByTestId('line-1-material').fill('S45C')
    await page.getByTestId('line-1-qty').fill('50')
    await page.getByTestId('line-1-unit').fill('PCS')
    await page.getByTestId('line-1-material_cost').fill('620')
    await page.getByTestId('line-1-process_cost').fill('880')
    await page.getByTestId('line-1-target_price').fill('1900')
    await shot(page, '10-兩列明細')

    await expect(page.getByTestId('computed-subtotal')).toBeVisible()
    await expect(page.getByTestId('computed-tax')).toBeVisible()
    await shot(page, '11-金額自動計算')

    await page.getByTestId('field-customer_contacts').fill('cust@example.com')
    await shot(page, '12-填寫客戶聯絡人')

    // ── 送簽 ────────────────────────────────────────
    const submit = page.getByTestId('create-submit')
    await submit.scrollIntoViewIfNeeded()
    await shot(page, '13-送出前確認')
    await submit.click()

    // 驗證失敗時畫面停在原地並顯示錯誤，先攔下來看
    const err = page.getByTestId('create-error')
    if (await err.isVisible().catch(() => false)) {
      await shot(page, '13b-送出失敗')
      throw new Error(`送出被擋：${await err.textContent()}`)
    }

    await expect(page).toHaveURL(/\/quotations$/, { timeout: 20_000 })
    await expect(page.getByTestId(`quotation-row-${KEY}`)).toBeVisible({
      timeout: 20_000,
    })
    await shot(page, '14-送簽完成')

    // ══ 第二段：主管簽核 ════════════════════════════
    // resolver 是 manager_of（發起人的主管）。
    // 從畫面建單的發起人是 sales，其主管是 designer（陳雅婷）。
    await waitForTask(request, 'designer@demo.local', KEY)

    await login(page, 'designer@demo.local')
    await shot(page, '15-主管登入')

    await page.getByTestId('nav-tasks').click()
    const mgrRow = page
      .locator('[data-testid^="task-row-"]')
      .filter({ hasText: KEY })
    await expect(mgrRow).toBeVisible({ timeout: 20_000 })
    await shot(page, '16-主管收到待辦')

    const mgrId = (await mgrRow.getAttribute('data-testid'))!.replace(
      'task-row-',
      '',
    )
    await page
      .getByTestId(`comment-${mgrId}`)
      .fill('客戶為長期合作對象，折扣合理，同意。')
    await shot(page, '17-填寫簽核意見')

    await page.getByTestId(`approve-${mgrId}`).click()
    await expect(mgrRow).toBeHidden({ timeout: 20_000 })
    await shot(page, '18-主管核准完成')

    // ══ 第三段：財務簽核 ════════════════════════════
    // 折扣 20% > 15%，條件節點把流程導向財務加簽這一關。
    // resolver 是 role = finance_manager。
    await waitForTask(request, 'finance@demo.local', KEY)

    await login(page, 'finance@demo.local')
    await page.getByTestId('nav-tasks').click()

    const finRow = page
      .locator('[data-testid^="task-row-"]')
      .filter({ hasText: KEY })
    await expect(finRow).toBeVisible({ timeout: 20_000 })
    await shot(page, '19-財務收到待辦')

    const finId = (await finRow.getAttribute('data-testid'))!.replace(
      'task-row-',
      '',
    )
    await page.getByTestId(`comment-${finId}`).fill('毛利率符合公司標準，核准。')
    await page.getByTestId(`approve-${finId}`).click()
    await expect(finRow).toBeHidden({ timeout: 20_000 })
    await shot(page, '20-財務核准完成')

    // ══ 第四段：流程狀態 ════════════════════════════
    await login(page, 'sales@demo.local')
    await page.getByTestId('nav-quotations').click()
    await expect(page.getByTestId(`quotation-row-${KEY}`)).toBeVisible({
      timeout: 20_000,
    })
    await shot(page, '21-報價單簽核狀態')
  })
})
