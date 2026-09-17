/**
 * 逾時前提醒設定 E2E
 *
 * 使用者可自己排程：到期前 2 天、1 天、12 小時、6 小時各發一次。
 */

import { expect, test, type APIRequestContext, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/測試報告/202609/screenshots/reminder')
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
  expect(res.ok()).toBeTruthy()
  return (await res.json()).access_token
}

async function createWorkflow(request: APIRequestContext, key: string): Promise<void> {
  const jwt = await token(request, 'designer@demo.local')
  const res = await request.post(`${API}/workflows`, {
    headers: { Authorization: `Bearer ${jwt}` },
    data: {
      workflow_key: key,
      business_object: 'quotation',
      name: `提醒測試 ${key}`,
      content: {
        workflow_key: key,
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
            timeout: { after: 'P3D', policy: 'AUTO_REJECT' },
          },
          { id: 'end', type: 'end', label: '結束', result: 'completed' },
        ],
        edges: [['start', 'approve'], ['approve', 'end']],
      },
    },
  })
  expect(res.status()).toBe(201)
}

async function login(page: Page): Promise<void> {
  await page.goto('/')
  await page.evaluate(() => localStorage.clear())
  await page.goto('/login')
  await page.getByTestId('login-tenant').fill('demo')
  await page.getByTestId('login-email').fill('designer@demo.local')
  await page.getByTestId('login-password').fill('demo1234')
  await page.getByTestId('login-submit').click()
  await expect(page).toHaveURL('http://localhost:3040/')
}

test.use({ viewport: { width: 1600, height: 1000 } })

test.describe('提醒設定', () => {
  let key: string

  test.beforeEach(async ({ page, request }) => {
    key = `rm_${Date.now()}`
    await createWorkflow(request, key)
    await login(page)
    await page.goto(`/designer/workflows/${key}`)
    await page.getByTestId('node-approve').click()
  })

  test('設了逾時就能設提醒', async ({ page }) => {
    await expect(page.getByTestId('node-prop-remind-at')).toBeVisible()
    await shot(page, '01-提醒欄位')
  })

  test('可設定多個時間點', async ({ page }) => {
    await page.getByTestId('node-prop-remind-at').fill('P2D, P1D, PT12H, PT6H')

    await page.getByTestId('save-draft').click()
    await expect(page.getByTestId('save-status')).toContainText('已儲存')
    await shot(page, '02-多個提醒時間點')
  })

  test('存檔後驗證通過', async ({ page }) => {
    await page.getByTestId('node-prop-remind-at').fill('P1D, PT6H')
    await page.getByTestId('save-draft').click()
    await expect(page.getByTestId('save-status')).toContainText('已儲存')

    await page.getByTestId('validate').click()
    await expect(page.getByTestId('validation-ok')).toBeVisible()
    await shot(page, '03-驗證通過')
  })

  test('存進去的是陣列而非字串', async ({ page, request }) => {
    await page.getByTestId('node-prop-remind-at').fill('P2D, P1D, PT6H')
    await page.getByTestId('save-draft').click()
    await expect(page.getByTestId('save-status')).toContainText('已儲存')

    const jwt = await token(request, 'designer@demo.local')
    const res = await request.get(`${API}/workflows/${key}`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    const detail = await res.json()
    const approve = detail.draft.content.nodes.find(
      (n: { id: string }) => n.id === 'approve',
    )

    expect(approve.timeout.remind_at).toEqual(['P2D', 'P1D', 'PT6H'])
  })

  test('清空後移除設定', async ({ page, request }) => {
    await page.getByTestId('node-prop-remind-at').fill('P1D')
    await page.getByTestId('node-prop-remind-at').fill('')
    await page.getByTestId('save-draft').click()
    await expect(page.getByTestId('save-status')).toContainText('已儲存')

    const jwt = await token(request, 'designer@demo.local')
    const res = await request.get(`${API}/workflows/${key}`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    const detail = await res.json()
    const approve = detail.draft.content.nodes.find(
      (n: { id: string }) => n.id === 'approve',
    )

    expect(approve.timeout.remind_at).toBeUndefined()
  })
})
