/**
 * 並行分支設計器 E2E
 *
 * 驗證使用者能在畫面上建立、編輯、刪除並行分支，
 * 且產生的流程定義能通過後端驗證。
 *
 * 並行是企業流程的核心能力（多人會簽、跨部門協作），
 * 先前設計器完全沒有入口——節點庫有「並行」與「匯合」兩項，
 * 但點了只會插入單一節點，立刻違反 WF-E009／WF-E010。
 *
 * 需先啟動：PostgreSQL、Temporal、Rust API、前端。
 * 測試會自己建立流程定義，key 帶時間戳避免衝突。
 */

import { expect, test, type APIRequestContext, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(
  HERE,
  '../../docs/測試報告/202609/screenshots/parallel-designer',
)
mkdirSync(SHOTS, { recursive: true })

const API = 'http://localhost:3001'

async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

async function token(request: APIRequestContext, email: string): Promise<string> {
  const res = await request.post(`${API}/auth/login`, {
    data: { tenant_code: 'demo', email, password: 'demo1234' },
  })
  expect(res.ok(), `登入 ${email} 失敗`).toBeTruthy()
  return (await res.json()).access_token
}

/** 最小的線性流程，供測試插入並行 */
function seedContent() {
  return {
    workflow_key: 'placeholder',
    version: 1,
    business_object: 'quotation',
    trigger: { type: 'form_submit' },
    nodes: [
      { id: 'start', type: 'trigger', label: '送出申請' },
      {
        id: 'approve',
        type: 'human_approval',
        label: '主管簽核',
        participant: 'internal',
        resolver: { type: 'role', value: 'approver' },
      },
      { id: 'end', type: 'end', label: '結束', result: 'completed' },
    ],
    edges: [
      ['start', 'approve'],
      ['approve', 'end'],
    ],
  }
}

async function createWorkflow(
  request: APIRequestContext,
  key: string,
): Promise<void> {
  const jwt = await token(request, 'designer@demo.local')
  const content = { ...seedContent(), workflow_key: key }
  const res = await request.post(`${API}/workflows`, {
    headers: { Authorization: `Bearer ${jwt}` },
    data: {
      workflow_key: key,
      business_object: 'quotation',
      name: `並行測試 ${key}`,
      content,
    },
  })
  expect(res.status(), '建立流程定義失敗').toBe(201)
}

/**
 * 以 designer 登入
 *
 * 先清 session：已登入時 guard 會把 /login 導回首頁，
 * 登入表單不會出現。
 */
async function loginAsDesigner(page: Page): Promise<void> {
  await page.goto('/')
  await page.evaluate(() => localStorage.clear())
  await page.goto('/login')
  await page.getByTestId('login-tenant').fill('demo')
  await page.getByTestId('login-email').fill('designer@demo.local')
  await page.getByTestId('login-password').fill('demo1234')
  await page.getByTestId('login-submit').click()
  await expect(page).toHaveURL('http://localhost:3040/')
}

function uniqueKey(): string {
  return `par_e2e_${Date.now()}`
}

/** 畫布較寬，用大一點的視窗才看得到並排的分支 */
test.use({ viewport: { width: 1600, height: 1000 } })

test.describe('建立並行', () => {
  test('一次點擊建立完整的一組並行', async ({ page, request }) => {
    const key = uniqueKey()
    await createWorkflow(request, key)
    await loginAsDesigner(page)
    await page.goto(`/designer/workflows/${key}`)

    await expect(page.getByTestId('node-start')).toBeVisible()
    await shot(page, '01-原始線性流程')

    // 點「送出申請 → 主管簽核」之間的插入點
    await page.getByTestId('insert-start-approve').click()
    await expect(page.getByTestId('insert-hint')).toBeVisible()
    await shot(page, '02-選取插入點')

    await page.getByTestId('node-palette-parallel').click()

    // 一次生成：parallel + 2 個分支 + join
    await expect(page.getByTestId('node-parallel')).toBeVisible()
    await expect(page.getByTestId('node-join')).toBeVisible()
    await expect(page.getByTestId('node-branch')).toBeVisible()
    await expect(page.getByTestId('node-branch_2')).toBeVisible()
    await shot(page, '03-一次建立整組並行')
  })

  test('節點庫的「匯合」單獨不可用', async ({ page, request }) => {
    const key = uniqueKey()
    await createWorkflow(request, key)
    await loginAsDesigner(page)
    await page.goto(`/designer/workflows/${key}`)

    await page.getByTestId('insert-start-approve').click()
    await page.getByTestId('node-palette-join').click()

    // join 不能單獨存在，導引使用者改用「並行」
    await expect(page.getByTestId('error-banner')).toContainText('並行')
    await shot(page, '04-匯合不可單獨插入')
  })
})

test.describe('編輯 join 策略', () => {
  test.beforeEach(async ({ page, request }) => {
    const key = uniqueKey()
    await createWorkflow(request, key)
    await loginAsDesigner(page)
    await page.goto(`/designer/workflows/${key}`)
    await page.getByTestId('insert-start-approve').click()
    await page.getByTestId('node-palette-parallel').click()
    await page.getByTestId('node-join').click()
  })

  test('完成條件與判定方式可分別設定', async ({ page }) => {
    // 兩者是不同的事：完成條件管「等幾個」，判定方式管「怎麼算」
    await expect(page.getByTestId('node-prop-join-completion')).toHaveValue('ALL')
    await expect(page.getByTestId('node-prop-join-result')).toHaveValue('ALL_SUCCESS')
    await shot(page, '05-join預設策略')
  })

  test('切到 N_OF_M 自動給 n，避免存檔後才被驗證擋下', async ({ page }) => {
    await page.getByTestId('node-prop-join-completion').selectOption('N_OF_M')

    // n 欄位出現且有預設值——沒有的話卡片會顯示「? 人完成即繼續」
    await expect(page.getByTestId('node-prop-join-n')).toBeVisible()
    await expect(page.getByTestId('node-prop-join-n')).toHaveValue('1')
    await expect(page.getByTestId('node-summary-join')).toContainText('1 人完成')
    await shot(page, '06-N_OF_M自動帶入n')
  })

  test('切回 ALL 時移除 n', async ({ page }) => {
    await page.getByTestId('node-prop-join-completion').selectOption('N_OF_M')
    await page.getByTestId('node-prop-join-completion').selectOption('ALL')

    await expect(page.getByTestId('node-prop-join-n')).toHaveCount(0)
    await expect(page.getByTestId('node-summary-join')).toContainText('全部完成')
  })

  test('判定方式切換時說明跟著變', async ({ page }) => {
    await page.getByTestId('node-prop-join-result').selectOption('ANY_REJECT')
    await expect(page.getByTestId('node-prop-join-result').locator('..')).toContainText(
      '任一人退回',
    )
    await shot(page, '07-判定方式說明')
  })

  test('卡片摘要反映策略', async ({ page }) => {
    await expect(page.getByTestId('node-summary-join')).toContainText('全部完成')

    await page.getByTestId('node-prop-join-completion').selectOption('ANY')
    await expect(page.getByTestId('node-summary-join')).toContainText('任一完成')
  })
})

test.describe('刪除並行', () => {
  async function setup(page: Page, request: APIRequestContext) {
    const key = uniqueKey()
    await createWorkflow(request, key)
    await loginAsDesigner(page)
    await page.goto(`/designer/workflows/${key}`)
    await page.getByTestId('insert-start-approve').click()
    await page.getByTestId('node-palette-parallel').click()
  }

  test('刪除 parallel 會一併移除分支與 join', async ({ page, request }) => {
    await setup(page, request)
    await page.getByTestId('node-remove-parallel').click()

    // 整組一起消失，留下任一個都會立刻變成非法狀態
    await expect(page.getByTestId('node-parallel')).toHaveCount(0)
    await expect(page.getByTestId('node-join')).toHaveCount(0)
    await expect(page.getByTestId('node-branch')).toHaveCount(0)
    // 前後重新接起來
    await expect(page.getByTestId('node-start')).toBeVisible()
    await expect(page.getByTestId('node-approve')).toBeVisible()
    await shot(page, '08-刪除後回到線性')
  })

  test('刪除 join 等同刪除整組', async ({ page, request }) => {
    await setup(page, request)
    await page.getByTestId('node-remove-join').click()

    await expect(page.getByTestId('node-parallel')).toHaveCount(0)
    await expect(page.getByTestId('node-join')).toHaveCount(0)
  })
})

test.describe('儲存與驗證', () => {
  test('並行流程通過後端驗證', async ({ page, request }) => {
    const key = uniqueKey()
    await createWorkflow(request, key)
    await loginAsDesigner(page)
    await page.goto(`/designer/workflows/${key}`)

    await page.getByTestId('insert-start-approve').click()
    await page.getByTestId('node-palette-parallel').click()

    await page.getByTestId('save-draft').click()
    await expect(page.getByTestId('save-status')).toContainText('已儲存')

    await page.getByTestId('validate').click()
    await expect(page.getByTestId('validation-ok')).toBeVisible()
    await shot(page, '09-驗證通過')
  })

  test('存檔後的定義含 parallel 與 join', async ({ page, request }) => {
    const key = uniqueKey()
    await createWorkflow(request, key)
    await loginAsDesigner(page)
    await page.goto(`/designer/workflows/${key}`)

    await page.getByTestId('insert-start-approve').click()
    await page.getByTestId('node-palette-parallel').click()
    await page.getByTestId('save-draft').click()
    await expect(page.getByTestId('save-status')).toContainText('已儲存')

    // 從 API 確認寫進去的內容，而非只看畫面
    const jwt = await token(request, 'designer@demo.local')
    const res = await request.get(`${API}/workflows/${key}`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    const detail = await res.json()
    const nodes = detail.draft.content.nodes as Array<{ type: string }>

    expect(nodes.filter((n) => n.type === 'parallel')).toHaveLength(1)
    expect(nodes.filter((n) => n.type === 'join')).toHaveLength(1)
    // 兩個分支 + 原本的主管簽核
    expect(nodes.filter((n) => n.type === 'human_approval')).toHaveLength(3)
  })

  test('巢狀並行也能通過驗證', async ({ page, request }) => {
    const key = uniqueKey()
    await createWorkflow(request, key)
    await loginAsDesigner(page)
    await page.goto(`/designer/workflows/${key}`)

    // 第一組
    await page.getByTestId('insert-start-approve').click()
    await page.getByTestId('node-palette-parallel').click()
    await expect(page.getByTestId('node-parallel')).toBeVisible()

    // 第二組接在第一組之後
    await page.getByTestId('insert-join-approve').click()
    await page.getByTestId('node-palette-parallel').click()
    await expect(page.getByTestId('node-parallel_2')).toBeVisible()

    await page.getByTestId('save-draft').click()
    await page.getByTestId('validate').click()
    await expect(page.getByTestId('validation-ok')).toBeVisible()
    await shot(page, '10-兩組並行串接')
  })
})

test.describe('授權', () => {
  test('沒有 designer 角色不能存檔', async ({ page, request }) => {
    const key = uniqueKey()
    await createWorkflow(request, key)

    // 以 requester 登入
    await page.goto('/')
    await page.evaluate(() => localStorage.clear())
    await page.goto('/login')
    await page.getByTestId('login-tenant').fill('demo')
    await page.getByTestId('login-email').fill('sales@demo.local')
    await page.getByTestId('login-password').fill('demo1234')
    await page.getByTestId('login-submit').click()
    await expect(page).toHaveURL('http://localhost:3040/')

    await page.goto(`/designer/workflows/${key}`)
    await page.getByTestId('insert-start-approve').click()
    await page.getByTestId('node-palette-parallel').click()
    await page.getByTestId('save-draft').click()

    await expect(page.getByTestId('error-banner')).toContainText('designer')
    await shot(page, '11-無權限存檔被擋')
  })
})
