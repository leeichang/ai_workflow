/**
 * join 逾時設定 E2E
 *
 * 並簽時某人長期不簽會讓整個流程卡住，逾時是唯一的出口。
 *
 * join 不支援 ESCALATE——分支各有自己的簽核人，加簽給誰沒有明確語意。
 * 選項清單要排除它，否則使用者設了才在驗證時被 WF-E009 擋下。
 */

import { expect, test, type APIRequestContext, type Page } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/測試報告/202609/screenshots/join-timeout')
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
      name: `逾時測試 ${key}`,
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

test.describe('join 逾時', () => {
  test.beforeEach(async ({ page, request }) => {
    const key = `jt_${Date.now()}`
    await createWorkflow(request, key)
    await login(page)
    await page.goto(`/designer/workflows/${key}`)
    await page.getByTestId('insert-start-approve').click()
    await page.getByTestId('node-palette-parallel').click()
    await page.getByTestId('node-join').click()
  })

  test('join 可設定逾時', async ({ page }) => {
    await expect(page.getByTestId('node-prop-timeout-after')).toBeVisible()
    await shot(page, '01-join可設逾時')
  })

  test('策略選單不含 ESCALATE', async ({ page }) => {
    await page.getByTestId('node-prop-timeout-after').fill('P2D')

    const options = await page
      .getByTestId('node-prop-timeout-policy')
      .locator('option')
      .allTextContents()

    // 分支各有自己的簽核人，加簽給誰沒有明確語意
    expect(options.join()).not.toContain('加簽')
    expect(options.join()).toContain('自動退回')
    await shot(page, '02-無加簽選項')
  })

  test('設定逾時後驗證通過', async ({ page }) => {
    await page.getByTestId('node-prop-timeout-after').fill('P2D')
    await page.getByTestId('node-prop-timeout-policy').selectOption('AUTO_REJECT')

    await page.getByTestId('save-draft').click()
    await expect(page.getByTestId('save-status')).toContainText('已儲存')
    await page.getByTestId('validate').click()
    await expect(page.getByTestId('validation-ok')).toBeVisible()
    await shot(page, '03-設定逾時後驗證通過')
  })
})

test.describe('簽核節點的加簽', () => {
  test('簽核節點保留 ESCALATE 選項', async ({ page, request }) => {
    const key = `jt_esc_${Date.now()}`
    await createWorkflow(request, key)
    await login(page)
    await page.goto(`/designer/workflows/${key}`)
    await page.getByTestId('node-approve').click()
    await page.getByTestId('node-prop-timeout-after').fill('P2D')

    const options = await page
      .getByTestId('node-prop-timeout-policy')
      .locator('option')
      .allTextContents()

    expect(options.join()).toContain('加簽')
    await shot(page, '04-簽核節點可加簽')
  })
})
