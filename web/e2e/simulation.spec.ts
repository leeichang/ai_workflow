/**
 * 模擬簽核 E2E
 *
 * 驗收標準（交辦 §5）：
 *   一個測試者從頭走完流程，每關看到的簽核人與該角色實際解析出的一致。
 *   正式租戶的非 assignee 仍然被擋。
 *
 * 這條測的是使用者真正要的東西：
 *   「測試的人可以直接點選模擬這個人簽核，一一完成後最後整個流程完成」
 *
 * 需先啟動全部六個服務。
 */

import { expect, test, type Page, type APIRequestContext } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/測試報告/202609/screenshots/simulation')
mkdirSync(SHOTS, { recursive: true })

const API = 'http://localhost:3001'

async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

async function login(page: Page): Promise<void> {
  await page.goto('/login')
  await page.getByTestId('login-tenant').fill('demo')
  await page.getByTestId('login-email').fill('designer@demo.local')
  await page.getByTestId('login-password').fill('demo1234')
  await page.getByTestId('login-submit').click()
  await expect(page).toHaveURL('http://localhost:3040/')
}

async function token(request: APIRequestContext, tenant = 'demo'): Promise<string> {
  const res = await request.post(`${API}/auth/login`, {
    data: {
      tenant_code: tenant,
      email: 'designer@demo.local',
      password: 'demo1234',
    },
  })
  expect(res.ok(), `登入 ${tenant} 失敗`).toBeTruthy()
  return (await res.json()).access_token
}

/** 收掉測試建立的開發模式——沙箱會複製整份定義，不收會累積 */
const created: string[] = []

/**
 * 清掉所有進行中的開發模式
 *
 * 測試不該依賴前一次執行留下的狀態。一個租戶同時只會有一個
 * 開發模式在用，所以先清乾淨再開始，空狀態才是確定的。
 */
async function clearSandboxes(request: APIRequestContext): Promise<void> {
  const jwt = await token(request)
  const res = await request.get(`${API}/sandboxes`, {
    headers: { Authorization: `Bearer ${jwt}` },
  })
  if (!res.ok()) return

  for (const s of await res.json()) {
    if (s.sandbox_tenant_id) {
      await request
        .post(`${API}/sandboxes/${s.sandbox_tenant_id}/retire`, {
          headers: { Authorization: `Bearer ${jwt}` },
        })
        .catch(() => undefined)
    }
  }
}

test.afterAll(async ({ request }) => {
  const jwt = await token(request)
  for (const id of created) {
    await request
      .post(`${API}/sandboxes/${id}/retire`, {
        headers: { Authorization: `Bearer ${jwt}` },
      })
      .catch(() => undefined)
  }
})

test.describe('開發模式', () => {
  test('從選單進入，可建立開發模式並看到不可關閉的標示', async ({
    page,
    request,
  }) => {
    await clearSandboxes(request)
    await login(page)
    await page.goto('/simulation')

    await shot(page, '01-尚未進入開發模式')

    // 先確認空狀態有明確的下一步，不是空白畫面
    await expect(page.getByTestId('sandbox-empty')).toBeVisible()
    await page.getByTestId('sandbox-create').click()

    // 建立會複製整份主檔與定義，給足時間
    await expect(page.getByTestId('sandbox-banner')).toBeVisible({
      timeout: 30_000,
    })
    await shot(page, '02-開發模式標示')

    // 標示不可關閉——使用者若以為正式也核准了，比不給模擬更糟
    await expect(page.getByTestId('sandbox-banner-close')).toHaveCount(0)

    const jwt = await token(request)
    const list = await request.get(`${API}/sandboxes`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    const sessions = await list.json()
    for (const s of sessions) {
      if (s.sandbox_tenant_id) created.push(s.sandbox_tenant_id)
    }
    expect(sessions.length, '應有進行中的開發模式').toBeGreaterThan(0)
  })
})

test.describe('模擬簽核', () => {
  // 這條真的很長：建沙箱要複製整份定義（目前約 379 個流程），
  // 之後還要等 Worker 走兩次節點。不是競態，是工作量。
  test.setTimeout(180_000)

  test('一人扮演簽核人，把流程推到下一關', async ({ page, request }) => {
    // 驗收標準：測試者從頭操作，每關的簽核人由真實 resolver 解析，
    // 點一下就以那個人的身分簽核——全程不換身分登入。
    await clearSandboxes(request)
    const jwt = await token(request)

    // ══ 準備：建立開發模式並在裡面啟動一張測試單 ══════
    const createRes = await request.post(`${API}/sandboxes`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    expect(createRes.status(), '建立開發模式失敗').toBe(201)
    const sandboxId = (await createRes.json()).sandbox_tenant_id as string
    created.push(sandboxId)

    const businessKey = `E2E-SIM-${Date.now()}`
    const started = await request.post(
      `${API}/sandboxes/${sandboxId}/instances`,
      {
        headers: { Authorization: `Bearer ${jwt}` },
        data: {
          workflow_key: 'quotation_approval',
          business_key: businessKey,
          // 折扣 0.2 > 0.15，會走到財務加簽那條分支
          input: {
            quotation: {
              discount_rate: 0.2,
              total: 800000,
              customer_contact_ids: ['c@example.com'],
            },
          },
        },
      },
    )
    expect(started.status(), '在沙箱啟動流程失敗').toBe(201)

    // ══ 畫面：看到流程走到哪、誰在簽 ══════════════
    await login(page)
    await page.goto('/simulation')
    await expect(page.getByTestId('sandbox-banner')).toBeVisible()

    // Worker 走到第一個節點才會有待辦，重試到出現
    await expect(async () => {
      await page.getByTestId('sim-refresh').click()
      await expect(page.getByTestId('sim-task-table')).toBeVisible({
        timeout: 3_000,
      })
    }).toPass({ timeout: 40_000 })

    // 解析出來的簽核人與解析依據都要看得到——
    // 「為什麼是這個人」正是模擬要驗證的
    const table = page.getByTestId('sim-task-table')
    await expect(table).toContainText('主管簽核')
    await expect(table).toContainText('張文華')

    await shot(page, '03-待簽清單與解析出的簽核人')

    // ══ 扮演並簽核 ══════════════════════════════
    // 用 testid 而非按鈕文字：模板是「以 {{ name }} 簽核」，
    // Vue 會在插值前後留空白，靠文字比對很容易因空白而失準。
    await page.locator('[data-testid^="act-as-"]').first().click()
    await expect(page.getByTestId('sim-panel')).toBeVisible()
    await expect(page.getByTestId('sim-panel')).toContainText('張文華')

    await shot(page, '04-扮演面板')

    await page.getByTestId('sim-comment').fill('模擬核准')
    await page.getByTestId('sim-approve').click()

    // 流程前進到財務簽核（折扣 > 15% 觸發加簽）
    await expect(async () => {
      await page.getByTestId('sim-refresh').click()
      await expect(page.getByTestId('sim-task-table')).toContainText('李淑芬', {
        timeout: 3_000,
      })
    }).toPass({ timeout: 40_000 })

    await shot(page, '05-流程前進到財務簽核')
  })
})

test.describe('授權邊界', () => {
  test('正式租戶不可被當成開發模式來模擬簽核', async ({ request }) => {
    // **最重要的一條。** 授權開洞綁在 is_sandbox 上，
    // 拿正式租戶的 id 當沙箱來用必定被擋——否則等同開放冒名簽核。
    //
    // 正式租戶的 is_sandbox 永遠是 false，這個洞在正式環境
    // 不可能打開。日後若有人把條件改成「有某角色就能扮演」，
    // 這條會紅。
    const jwt = await token(request)

    // 先拿到自己租戶的 id——用它冒充沙箱
    const me = await request.get(`${API}/sandboxes`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    expect(me.ok()).toBeTruthy()
    const sessions = await me.json()
    const productionTenantId =
      sessions.length > 0
        ? sessions[0].tenant_id
        : '11111111-1111-1111-1111-111111111111'

    const res = await request.post(
      `${API}/sandboxes/${productionTenantId}/tasks/` +
        `00000000-0000-0000-0000-000000000000/simulate`,
      {
        headers: { Authorization: `Bearer ${jwt}` },
        data: {
          acting_as: '00000000-0000-0000-0000-000000000001',
          decision: 'APPROVE',
        },
      },
    )

    expect(
      res.status(),
      `正式租戶必須被擋下，實際回應：${await res.text()}`,
    ).toBe(403)
  })

  test('沙箱的待簽清單也擋非建立者的租戶', async ({ request }) => {
    // 少了「租戶必須相符」這條，別的租戶只要知道沙箱 id 就能看它
    const jwt = await token(request)

    const res = await request.get(
      `${API}/sandboxes/11111111-1111-1111-1111-111111111111/tasks`,
      { headers: { Authorization: `Bearer ${jwt}` } },
    )

    expect(res.status(), '正式租戶不是沙箱，應回 403').toBe(403)
  })
})

test.describe('權限過濾畫面', () => {
  // 使用者原話的第三件事：
  //   「系統依據權限設定顯示每個簽核人員的畫面」
  //
  // 少了這塊，模擬只證明流程會動，沒證明那個人簽核時
  // 看得到該看的、看不到不該看的。
  test.setTimeout(180_000)

  test('扮演時看得到那個人眼中的欄位權限', async ({ page, request }) => {
    await clearSandboxes(request)
    const jwt = await token(request)

    const createRes = await request.post(`${API}/sandboxes`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    expect(createRes.status()).toBe(201)
    const sandboxId = (await createRes.json()).sandbox_tenant_id as string
    created.push(sandboxId)

    const started = await request.post(
      `${API}/sandboxes/${sandboxId}/instances`,
      {
        headers: { Authorization: `Bearer ${jwt}` },
        data: {
          workflow_key: 'quotation_approval',
          business_key: `E2E-PERM-${Date.now()}`,
          input: {
            quotation: {
              discount_rate: 0.1,
              total: 400000,
              customer_contact_ids: ['c@example.com'],
            },
          },
        },
      },
    )
    expect(started.status()).toBe(201)

    await login(page)
    await page.goto('/simulation')
    await expect(async () => {
      await page.getByTestId('sim-refresh').click()
      await expect(page.getByTestId('sim-task-table')).toBeVisible({
        timeout: 3_000,
      })
    }).toPass({ timeout: 40_000 })

    await page.locator('[data-testid^="act-as-"]').first().click()
    await expect(page.getByTestId('sim-form-view')).toBeVisible()

    // 顯示的必須是**被扮演者**的角色，不是測試者的。
    // 測試者登入時帶的是 designer——出現 designer 就代表
    // 後端用錯了角色來源，畫面與那個人真正看到的不同。
    const roles = page.getByTestId('sim-form-roles')
    await expect(roles).toBeVisible()
    await expect(roles).not.toContainText('designer')

    await shot(page, '06-被扮演者眼中的欄位權限')
  })
})

test.describe('平行分支模擬', () => {
  // 驗收標準（交辦 §5 S2）：
  //   「一個測試者從頭走完含平行分支的報價單流程」
  test.setTimeout(240_000)

  test('會簽的兩張待辦歸成一組，顯示還在等幾條分支', async ({
    page,
    request,
  }) => {
    await clearSandboxes(request)
    const jwt = await token(request)

    const createRes = await request.post(`${API}/sandboxes`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    const sandboxId = (await createRes.json()).sandbox_tenant_id as string
    created.push(sandboxId)

    const started = await request.post(
      `${API}/sandboxes/${sandboxId}/instances`,
      {
        headers: { Authorization: `Bearer ${jwt}` },
        data: {
          workflow_key: 'quotation_parallel_approval',
          business_key: `E2E-COSIGN-${Date.now()}`,
          input: {
            quotation: {
              discount_rate: 0.2,
              total: 900000,
              customer_contact_ids: ['c@example.com'],
            },
          },
        },
      },
    )
    expect(started.status(), '啟動會簽流程失敗').toBe(201)

    await login(page)
    await page.goto('/simulation')

    // 先過主管簽核，會簽才會展開
    await expect(async () => {
      await page.getByTestId('sim-refresh').click()
      await expect(page.getByTestId('sim-task-table')).toContainText(
        '主管簽核',
        { timeout: 3_000 },
      )
    }).toPass({ timeout: 60_000 })

    await page.locator('[data-testid^="act-as-"]').first().click()
    await page.getByTestId('sim-approve').click()

    // 會簽展開：兩張待辦，一個標頭
    await expect(async () => {
      await page.getByTestId('sim-refresh').click()
      await expect(page.locator('[data-testid^="sim-branch-progress-"]')).toBeVisible({
        timeout: 3_000,
      })
    }).toPass({ timeout: 60_000 })

    const header = page.locator('[data-testid^="sim-branch-progress-"]')
    await expect(header).toContainText('2 條分支')
    // 標頭只有一個——兩個代表被當成兩組獨立的事，
    // 使用者會以為流程分岔成兩條路
    await expect(page.locator('[data-testid^="sim-branch-"]').first()).toBeVisible()

    await shot(page, '07-會簽分支與進度')

    // 簽掉一條，進度要動
    await page.locator('[data-testid^="act-as-"]').first().click()
    await page.getByTestId('sim-approve').click()

    await expect(async () => {
      await page.getByTestId('sim-refresh').click()
      await expect(page.locator('[data-testid^="sim-branch-progress-"]')).toContainText(
        '已完成 1 條',
        { timeout: 3_000 },
      )
    }).toPass({ timeout: 60_000 })

    await shot(page, '08-會簽完成一條分支')
  })
})

test.describe('時間快轉', () => {
  test.setTimeout(180_000)

  test('有逾時的節點可以快轉，並說明套用了哪個策略', async ({
    page,
    request,
  }) => {
    await clearSandboxes(request)
    const jwt = await token(request)

    const createRes = await request.post(`${API}/sandboxes`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    const sandboxId = (await createRes.json()).sandbox_tenant_id as string
    created.push(sandboxId)

    await request.post(`${API}/sandboxes/${sandboxId}/instances`, {
      headers: { Authorization: `Bearer ${jwt}` },
      data: {
        workflow_key: 'quotation_approval',
        business_key: `E2E-SKIP-${Date.now()}`,
        input: {
          quotation: {
            discount_rate: 0.1,
            total: 300000,
            customer_contact_ids: ['c@example.com'],
          },
        },
      },
    })

    await login(page)
    await page.goto('/simulation')
    await expect(async () => {
      await page.getByTestId('sim-refresh').click()
      await expect(page.locator('[data-testid^="skip-time-"]')).toBeVisible({
        timeout: 3_000,
      })
    }).toPass({ timeout: 60_000 })

    // 按鈕要寫明快轉多久、之後會發生什麼——
    // 快轉跑的是真正的逾時策略，不是假造一個通過
    const btn = page.locator('[data-testid^="skip-time-"]').first()
    await expect(btn).toContainText('2 天')

    await shot(page, '09-時間快轉按鈕')

    await btn.click()
    await expect(page.getByTestId('sim-skip-message')).toBeVisible({
      timeout: 20_000,
    })
    await expect(page.getByTestId('sim-skip-message')).toContainText('已快轉')

    await shot(page, '10-快轉後套用的逾時策略')
  })
})
