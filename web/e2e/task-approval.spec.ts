/**
 * 待辦核准 E2E
 *
 * 驗證簽核的完整往返：畫面按下核准 → Rust API 寫資料庫 →
 * 送 Signal 給 Temporal → Python Worker 推進流程 → 產生下一個節點的待辦。
 *
 * 這條路徑先前只有元件測試（用 MSW 假造 API 回應）覆蓋，
 * 沒有實機驗證過——Temporal 當時沒有執行。
 *
 * 需先啟動全部六個服務：
 *   PostgreSQL、Temporal (7233)、Rust API (3001)、Python Worker、前端 (3040)
 *
 * 測試會自己啟動一個流程，單號帶時間戳避免與既有資料衝突。
 * Temporal 以 workflow_id 去重，重複的單號會被拒絕而測試看不出原因。
 */

import { expect, test, type Page, type APIRequestContext } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { cancelTrackedInstances, trackInstance } from './support/instances'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(
  HERE,
  '../../docs/測試報告/202609/screenshots/task-approval',
)
mkdirSync(SHOTS, { recursive: true })

const API = 'http://localhost:3001'

async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
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
 * 啟動一張報價單
 *
 * 欄位必須包在 quotation 底下，流程定義是這樣讀的：
 *   discount_gate   的條件式是 `quotation.discount_rate > 0.15`
 *   customer_review 的 resolver path 是 `quotation.customer_contact_ids`
 *
 * 放在頂層的話，條件式取不到值會當成 false（跳過財務簽核），
 * 客戶簽核則會因為找不到參與者讓整個流程 FAILED。
 *
 * discount_rate 0.2 超過 15% 門檻，會走財務加簽的分支——
 * 這樣才驗得到「核准後流程真的前進到下一個節點」。
 */
async function startQuotation(
  request: APIRequestContext,
  businessKey: string,
): Promise<void> {
  const jwt = await token(request, 'designer@demo.local')
  const res = await request.post(`${API}/instances`, {
    headers: { Authorization: `Bearer ${jwt}` },
    data: {
      workflow_key: 'quotation_approval',
      business_key: businessKey,
      input: {
        quotation: {
          discount_rate: 0.2,
          total_amount: 738150,
          customer_name: '台灣松下精密機械股份有限公司',
          customer_contact_ids: ['cust@example.com'],
        },
      },
    },
  })
  expect(res.status(), '啟動流程失敗').toBe(201)

  // 這些流程沒有人會去簽完，不收掉的話會永遠留在 Temporal 輪詢。
  // 見 support/instances.ts 的說明。
  trackInstance((await res.json()).id)
}

/**
 * 等待指定單號的待辦出現
 *
 * 啟動流程只是送出 Temporal 的 StartWorkflow，待辦是 Python Worker
 * 走到第一個節點後才透過 internal API 建立的。中間隔著 Temporal 排程
 * 與一次 HTTP 往返，啟動後立刻查一定查不到。
 */
async function waitForTask(
  request: APIRequestContext,
  email: string,
  businessKey: string,
  timeoutMs = 20_000,
): Promise<{ id: string; business_key: string }> {
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

/**
 * 登入
 *
 * 先清 session 再進登入頁。已登入時 guard 會把 /login 導回首頁，
 * 登入表單根本不會出現——換帳號時直接 goto('/login') 會卡住。
 */
async function login(page: Page, email: string): Promise<void> {
  await page.goto('/')
  await page.evaluate(() => localStorage.clear())
  await page.goto('/login')
  await page.getByTestId('login-tenant').fill('demo')
  await page.getByTestId('login-email').fill(email)
  await page.getByTestId('login-password').fill('demo1234')
  await page.getByTestId('login-submit').click()
  await expect(page).toHaveURL('http://localhost:3040/')
}

/** 每個測試用自己的單號，避免互相干擾 */
function uniqueKey(prefix: string): string {
  return `${prefix}-${Date.now()}`
}

/**
 * 收掉本檔啟動的所有流程
 *
 * 這個檔案是實例的主要來源——每跑一次會啟動六到七個
 * 停在人工節點、沒有人會簽完的流程。不收掉的話它們會永遠
 * 留在 Temporal 輪詢，讓後續每次執行都更慢。
 */
test.afterAll(async ({ request }) => {
  const jwt = await token(request, 'designer@demo.local')
  await cancelTrackedInstances(request, jwt)
})

test.describe('核准', () => {
  // 這條走完整往返：啟動、等 Worker 建待辦、登入、核准、
  // 等 Worker 推進流程、換帳號再看一次。預設 30 秒不夠。
  test.setTimeout(90_000)

  test('核准後待辦消失，流程前進到財務簽核', async ({ page, request }) => {
    const key = uniqueKey('QT-APPROVE')
    await startQuotation(request, key)
    // 待辦由 Worker 非同步建立，先確認它存在再開畫面——
    // 收件匣沒有自動輪詢，進得太早就是一片空白
    await waitForTask(request, 'cfo@demo.local', key)

    await login(page, 'cfo@demo.local')
    await page.getByTestId('nav-tasks').click()

    // 剛啟動的報價單應該出現在 cfo 的收件匣
    const row = page.locator('[data-testid^="task-row-"]').filter({ hasText: key })
    await expect(row).toBeVisible()
    await expect(row).toContainText('主管簽核')
    await shot(page, '01-收到待辦')

    // 填意見再核准
    const taskId = await row.getAttribute('data-testid')
    const id = taskId!.replace('task-row-', '')
    await page.getByTestId(`comment-${id}`).fill('金額與折扣確認無誤')
    await shot(page, '02-填寫簽核意見')

    await page.getByTestId(`approve-${id}`).click()

    // 核准後這一筆應該從收件匣消失
    await expect(row).toBeHidden()
    await shot(page, '03-核准後待辦消失')

    // 流程前進到財務簽核，出現在 finance 的收件匣。
    // 這是關鍵驗證：Signal 真的送到 Temporal，Worker 真的推進了流程。
    // 同樣要先等 Worker 建好待辦，再開畫面。
    await waitForTask(request, 'finance@demo.local', key)

    await login(page, 'finance@demo.local')
    await page.getByTestId('nav-tasks').click()

    const financeRow = page
      .locator('[data-testid^="task-row-"]')
      .filter({ hasText: key })
    await expect(financeRow).toBeVisible({ timeout: 15_000 })
    await expect(financeRow).toContainText('財務簽核')
    await shot(page, '04-流程前進到財務簽核')
  })

  test('核准寫入資料庫，狀態與意見都留下', async ({ page, request }) => {
    const key = uniqueKey('QT-RECORD')
    await startQuotation(request, key)
    await waitForTask(request, 'cfo@demo.local', key)

    await login(page, 'cfo@demo.local')
    await page.getByTestId('nav-tasks').click()

    const row = page.locator('[data-testid^="task-row-"]').filter({ hasText: key })
    await expect(row).toBeVisible()
    const id = (await row.getAttribute('data-testid'))!.replace('task-row-', '')

    await page.getByTestId(`comment-${id}`).fill('已核對報價明細')
    await page.getByTestId(`approve-${id}`).click()
    await expect(row).toBeHidden()

    // 從 API 確認決策已寫入，而非只是畫面上消失
    const jwt = await token(request, 'cfo@demo.local')
    const res = await request.get(`${API}/tasks?status=APPROVED&limit=50`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    const tasks = await res.json()
    const recorded = tasks.find(
      (t: { business_key: string }) => t.business_key === key,
    )

    expect(recorded, '核准的待辦應該查得到').toBeDefined()
    expect(recorded.decision).toBe('APPROVE')
    expect(recorded.comment).toBe('已核對報價明細')
    expect(recorded.decided_at).not.toBeNull()
  })
})

test.describe('退回', () => {
  test('退回送出 REJECT 並記錄意見', async ({ page, request }) => {
    const key = uniqueKey('QT-REJECT')
    await startQuotation(request, key)
    await waitForTask(request, 'cfo@demo.local', key)

    await login(page, 'cfo@demo.local')
    await page.getByTestId('nav-tasks').click()

    const row = page.locator('[data-testid^="task-row-"]').filter({ hasText: key })
    await expect(row).toBeVisible()
    const id = (await row.getAttribute('data-testid'))!.replace('task-row-', '')

    await page.getByTestId(`comment-${id}`).fill('折扣率過高，請重新評估')
    await shot(page, '05-填寫退回原因')
    await page.getByTestId(`reject-${id}`).click()
    await expect(row).toBeHidden()
    await shot(page, '06-退回後')

    const jwt = await token(request, 'cfo@demo.local')
    const res = await request.get(`${API}/tasks?status=REJECTED&limit=50`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    const tasks = await res.json()
    const recorded = tasks.find(
      (t: { business_key: string }) => t.business_key === key,
    )

    expect(recorded, '退回的待辦應該查得到').toBeDefined()
    expect(recorded.decision).toBe('REJECT')
    expect(recorded.comment).toBe('折扣率過高，請重新評估')
  })
})

test.describe('授權', () => {
  test('看不到別人的待辦', async ({ page, request }) => {
    const key = uniqueKey('QT-OTHER')
    await startQuotation(request, key)
    // 先確認待辦確實存在（在 cfo 那裡），否則 procure 看不到可能只是
    // 因為它還沒被建立，測試會假性通過
    await waitForTask(request, 'cfo@demo.local', key)

    // 這張待辦指派給 cfo，procure 不該看到
    await login(page, 'procure@demo.local')
    await page.getByTestId('nav-tasks').click()
    await page.waitForLoadState('networkidle')

    const row = page.locator('[data-testid^="task-row-"]').filter({ hasText: key })
    await expect(row).toHaveCount(0)
    await shot(page, '07-他人待辦不可見')
  })

  test('直接打 API 核准別人的待辦回 403', async ({ request }) => {
    const key = uniqueKey('QT-FORBID')
    await startQuotation(request, key)

    // 以 cfo 的身分找出待辦 id
    const target = await waitForTask(request, 'cfo@demo.local', key)

    // 換成 procure 嘗試核准。前端看不到不等於後端擋得住，
    // 這裡繞過畫面直接打 API，驗證的是 tasks.rs 的授權檢查。
    const procureJwt = await token(request, 'procure@demo.local')
    const res = await request.post(`${API}/tasks/${target.id}/decision`, {
      headers: { Authorization: `Bearer ${procureJwt}` },
      data: { decision: 'APPROVE', comment: '' },
    })

    expect(res.status()).toBe(403)
  })

  test('未登入無法取得待辦', async ({ request }) => {
    const res = await request.get(`${API}/tasks`)
    expect(res.status()).toBe(401)
  })
})

test.describe('重複決策', () => {
  test('同一張待辦決策兩次，第二次回 409', async ({ request }) => {
    const key = uniqueKey('QT-TWICE')
    await startQuotation(request, key)

    const target = await waitForTask(request, 'cfo@demo.local', key)
    const jwt = await token(request, 'cfo@demo.local')

    const first = await request.post(`${API}/tasks/${target.id}/decision`, {
      headers: { Authorization: `Bearer ${jwt}` },
      data: { decision: 'APPROVE', comment: '第一次' },
    })
    expect(first.status()).toBe(200)

    // 兩人同時點同一張待辦時，第二個人會走到這裡
    const second = await request.post(`${API}/tasks/${target.id}/decision`, {
      headers: { Authorization: `Bearer ${jwt}` },
      data: { decision: 'APPROVE', comment: '第二次' },
    })
    expect(second.status()).toBe(409)
  })
})
