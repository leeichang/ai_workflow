/**
 * 流程監控介入動作的截圖腳本（A19 測試報告用）
 *
 * 與 process-monitor-intervene.spec.ts 分開：那支是驗證行為的測試，
 * 這支只負責產圖。混在一起的話，為了拍到好看的畫面會開始在測試裡
 * 加等待與排版調整，測試就不再是測試了。
 *
 * 跑法：npx playwright test e2e/shots-process-monitor.spec.ts
 */
import { test, expect, type Page, type APIRequestContext } from '@playwright/test'

const API = process.env.API_URL ?? 'http://localhost:3001'
const TENANT = 'demo'
const PASSWORD = 'demo1234'
const TENANT_ID = '11111111-1111-1111-1111-111111111111'
const INTERNAL_TOKEN = process.env.INTERNAL_API_TOKEN ?? 'dev-internal-token'

const DIR = '../docs/測試報告/202609/screenshots/process-monitor'

async function token(request: APIRequestContext, email: string): Promise<string> {
  const res = await request.post(`${API}/auth/login`, {
    data: { tenant_code: TENANT, email, password: PASSWORD },
  })
  return (await res.json()).access_token
}

async function loginAs(page: Page, email: string): Promise<void> {
  await page.goto('/login')
  await page.getByPlaceholder('demo').fill(TENANT)
  await page.getByPlaceholder('you@company.com').fill(email)
  await page.locator('input[type="password"]').fill(PASSWORD)
  await page.getByRole('button', { name: '登入' }).click()
  await page.waitForURL((u) => !u.pathname.includes('/login'))
}

async function seedFlagged(
  request: APIRequestContext,
): Promise<{ id: string; key: string }> {
  const t = await token(request, 'sales@demo.local')
  const key = `QT-SHOT-${Date.now()}`
  const res = await request.post(`${API}/instances`, {
    headers: { Authorization: `Bearer ${t}` },
    data: {
      workflow_key: 'quotation_approval',
      business_key: key,
      input: { quotation: { amount: 500000, discount_rate: 0.2 } },
    },
  })
  const id = (await res.json()).id

  await request.post(`${API}/internal/instances/${id}/attention`, {
    headers: {
      'X-Internal-Token': INTERNAL_TOKEN,
      'X-Tenant-Id': TENANT_ID,
    },
    data: {
      code: 'ESCALATE_UNRESOLVED',
      detail:
        '節點「主管簽核」逾時加簽，但解析不到新的簽核人。流程仍在等原簽核人處理。',
    },
  })
  return { id, key }
}

test('產出流程監控截圖', async ({ page, request }) => {
  test.setTimeout(120000)
  const { key } = await seedFlagged(request)

  await loginAs(page, 'monitor@demo.local')
  await page.goto('/monitor')
  await expect(page.getByTestId('monitor-loading')).toBeHidden()
  await page.getByTestId('monitor-status').selectOption('RUNNING')
  await expect(page.getByTestId('monitor-loading')).toBeHidden()
  await expect(page.getByTestId(`monitor-attention-${key}`)).toBeVisible()

  await page.screenshot({ path: `${DIR}/01-清單異常標記.png`, fullPage: false })

  // 只看需要處理的
  await page.getByTestId('monitor-attention-only').check()
  await expect(page.getByTestId('monitor-loading')).toBeHidden()
  await page.screenshot({ path: `${DIR}/02-只看需要處理的.png` })
  await page.getByTestId('monitor-attention-only').uncheck()
  await expect(page.getByTestId('monitor-loading')).toBeHidden()

  // 詳情：異常說明 + 待辦 + 介入按鈕
  await page.getByTestId(`monitor-open-${key}`).click()
  await expect(page.getByTestId('monitor-attention-box')).toBeVisible()
  await page.screenshot({ path: `${DIR}/03-詳情異常說明.png` })

  // 改派面板
  const reassign = page.getByTestId('monitor-reassign-manager_approval')
  if (await reassign.isVisible()) {
    await reassign.click()
    await expect(page.getByTestId('monitor-reassign-to')).toBeVisible()
    await page.screenshot({ path: `${DIR}/04-改派對象.png` })
  }

  // 標記已處理後
  await page.getByTestId('monitor-resolve-attention').click()
  await expect(page.getByTestId('monitor-action-message')).toContainText(
    '已標記為處理完成',
    { timeout: 20000 },
  )
  await page.screenshot({ path: `${DIR}/05-標記已處理.png` })
})

test('一般使用者看不到介入按鈕', async ({ page, request }) => {
  test.setTimeout(60000)
  const t = await token(request, 'sales@demo.local')
  const key = `QT-SHOT-RO-${Date.now()}`
  await request.post(`${API}/instances`, {
    headers: { Authorization: `Bearer ${t}` },
    data: {
      workflow_key: 'quotation_approval',
      business_key: key,
      input: { quotation: { amount: 100000, discount_rate: 0.05 } },
    },
  })

  await loginAs(page, 'sales@demo.local')
  await page.goto('/monitor')
  await expect(page.getByTestId('monitor-loading')).toBeHidden()
  await page.getByTestId(`monitor-open-${key}`).click()
  await expect(page.getByTestId('monitor-detail')).toBeVisible()
  await page.screenshot({ path: `${DIR}/06-一般使用者無介入按鈕.png` })
})
