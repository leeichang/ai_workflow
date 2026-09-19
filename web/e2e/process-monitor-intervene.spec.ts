/**
 * 流程監控的可見範圍與介入動作（N1、N3）
 *
 * 對應 2026-09-19 使用者定案：
 *   N1 另立 process_monitor 角色，權限為讀 + 催辦 + 取消 + 改派
 *   N3 解析不到加簽對象時標記 NEEDS_ATTENTION，流程續跑
 *
 * **這支測試的重點是「誰不該看到」與「誰不該能做」。**
 * 只驗證監控者做得到的話，一個把所有角色都放行的實作也會通過，
 * 而那正是 N1 刻意要避免的方向。
 *
 * 用 monitor@demo.local 而非 admin：admin 本來就看得到全部，
 * 拿它測等於完全沒測到新角色。
 */
import { test, expect, type Page, type APIRequestContext } from '@playwright/test'

const API = process.env.API_URL ?? 'http://localhost:3001'
const TENANT = 'demo'
const PASSWORD = 'demo1234'

/** 後端的租戶 id。掛異常旗標的 internal API 要帶 */
const TENANT_ID = '11111111-1111-1111-1111-111111111111'
const INTERNAL_TOKEN = process.env.INTERNAL_API_TOKEN ?? 'dev-internal-token'

async function token(request: APIRequestContext, email: string): Promise<string> {
  const res = await request.post(`${API}/auth/login`, {
    data: { tenant_code: TENANT, email, password: PASSWORD },
  })
  expect(res.ok()).toBeTruthy()
  return (await res.json()).access_token
}

/**
 * 造一筆進行中的流程
 *
 * 每次用不同的 business_key：Temporal 以它去重，
 * 重複的話第二次會撞既有流程，測試之間互相污染。
 */
async function startInstance(
  request: APIRequestContext,
  suffix: string,
): Promise<{ id: string; key: string }> {
  const t = await token(request, 'sales@demo.local')
  const key = `QT-E2E-${suffix}-${Date.now()}`
  const res = await request.post(`${API}/instances`, {
    headers: { Authorization: `Bearer ${t}` },
    data: {
      workflow_key: 'quotation_approval',
      business_key: key,
      input: { quotation: { amount: 500000, discount_rate: 0.2 } },
    },
  })
  expect(res.ok()).toBeTruthy()
  return { id: (await res.json()).id, key }
}

/** 掛上異常旗標，模擬 Worker 解析不到加簽對象時的回報 */
async function raiseAttention(
  request: APIRequestContext,
  instanceId: string,
): Promise<void> {
  const res = await request.post(
    `${API}/internal/instances/${instanceId}/attention`,
    {
      headers: {
        'X-Internal-Token': INTERNAL_TOKEN,
        'X-Tenant-Id': TENANT_ID,
      },
      data: {
        code: 'ESCALATE_UNRESOLVED',
        detail: '節點「主管簽核」逾時加簽，但解析不到新的簽核人。',
      },
    },
  )
  expect(res.ok()).toBeTruthy()
}

/**
 * 開啟某張單的詳情
 *
 * 用搜尋而非直接在預設清單上找：清單預設只顯示 50 筆，排序是
 * 「異常優先、再依時間新到舊」。整套 E2E 跑完時 demo 租戶有
 * 三百多筆，且前面的測試會標記異常——那些異常單會排到最前面，
 * 把剛建立的單擠出第一頁。
 *
 * 這是實際踩到的坑：單獨跑這支檔案全過，跟著整套跑就找不到列。
 * 頁面因此改成一次取 200 筆（見 ProcessMonitor.vue 的 load）。
 *
 * 這裡再用「執行中」收斂一次：測試建立的單都是 RUNNING，
 * 而累積的三百多筆大多已取消。少了這層，等資料再長一倍就又會紅。
 */
async function gotoRunning(page: Page): Promise<void> {
  await page.goto('/monitor')
  await expect(page.getByTestId('monitor-loading')).toBeHidden()
  await page.getByTestId('monitor-status').selectOption('RUNNING')
  await expect(page.getByTestId('monitor-loading')).toBeHidden()
}

async function openInstance(page: Page, key: string): Promise<void> {
  await gotoRunning(page)

  await expect(page.getByTestId(`monitor-row-${key}`)).toBeVisible()
  await page.getByTestId(`monitor-open-${key}`).click()
  await expect(page.getByTestId('monitor-detail')).toBeVisible()
}

async function loginAs(page: Page, email: string): Promise<void> {
  await page.goto('/login')
  await page.getByPlaceholder('demo').fill(TENANT)
  await page.getByPlaceholder('you@company.com').fill(email)
  await page.locator('input[type="password"]').fill(PASSWORD)
  await page.getByRole('button', { name: '登入' }).click()
  await page.waitForURL((u) => !u.pathname.includes('/login'))
}

test.describe('N1：可見範圍', () => {
  test('process_monitor 看得到別人發起的單', async ({ page, request }) => {
    const { key } = await startInstance(request, 'VIS')

    await loginAs(page, 'monitor@demo.local')
    await gotoRunning(page)

    await expect(page.getByTestId(`monitor-row-${key}`)).toBeVisible()
  })

  test('approver 看不到別人的單', async ({ page, request }) => {
    // **最重要的一條。** N1 的核心決定就是不讓簽核角色兼差當監控者。
    // 這條紅掉代表 can_monitor_all 被放寬了，監控變成全租戶資料出口。
    const { key } = await startInstance(request, 'DENY')

    await loginAs(page, 'qa@demo.local')
    await page.goto('/monitor')
    await expect(page.getByTestId('monitor-loading')).toBeHidden()

    await expect(page.getByTestId(`monitor-row-${key}`)).toHaveCount(0)
  })

  test('approver 打監控端點會被擋', async ({ request }) => {
    // 前端不顯示按鈕只是 UX。真正的權限在後端，這條直接打 API。
    const { id } = await startInstance(request, 'API')
    const t = await token(request, 'qa@demo.local')

    const tasks = await request.get(`${API}/instances/${id}/tasks`, {
      headers: { Authorization: `Bearer ${t}` },
    })
    expect(tasks.status()).toBe(403)

    const remind = await request.post(`${API}/instances/${id}/remind`, {
      headers: { Authorization: `Bearer ${t}` },
    })
    expect(remind.status()).toBe(403)
  })

  test('非發起人不能取消別人的單', async ({ request }) => {
    // 這裡原本是個漏洞：cancel 完全沒有角色檢查，
    // 同租戶任何人知道 id 就能取消別人的單。
    // 回 404 而非 403——403 等於告訴對方這張單存在。
    const { id } = await startInstance(request, 'CANCEL')
    const t = await token(request, 'qa@demo.local')

    const res = await request.post(`${API}/instances/${id}/cancel`, {
      headers: { Authorization: `Bearer ${t}` },
      data: { reason: 'e2e' },
    })
    expect(res.status()).toBe(404)
  })
})

test.describe('N3：異常標記', () => {
  test('異常的單有紅色標記且排在最前面', async ({ page, request }) => {
    const { id, key } = await startInstance(request, 'ATT')
    await raiseAttention(request, id)

    await loginAs(page, 'monitor@demo.local')
    await gotoRunning(page)

    await expect(page.getByTestId(`monitor-attention-${key}`)).toBeVisible()

    // 監控頁的用途是找出要處理的單，異常沉在後面等於這功能沒做
    const firstRow = page.locator('[data-testid^="monitor-row-"]').first()
    await expect(firstRow).toHaveAttribute('data-testid', `monitor-row-${key}`)
  })

  test('只看需要處理的會濾掉正常的單', async ({ page, request }) => {
    const normal = await startInstance(request, 'NORMAL')
    const flagged = await startInstance(request, 'FLAG')
    await raiseAttention(request, flagged.id)

    await loginAs(page, 'monitor@demo.local')
    await gotoRunning(page)

    await page.getByTestId('monitor-attention-only').check()
    await expect(page.getByTestId('monitor-loading')).toBeHidden()

    await expect(page.getByTestId(`monitor-row-${flagged.key}`)).toBeVisible()
    await expect(page.getByTestId(`monitor-row-${normal.key}`)).toHaveCount(0)
  })

  test('詳情顯示異常說明與處理方式', async ({ page, request }) => {
    const { id, key } = await startInstance(request, 'BOX')
    await raiseAttention(request, id)

    await loginAs(page, 'monitor@demo.local')
    await openInstance(page, key)

    const box = page.getByTestId('monitor-attention-box')
    await expect(box).toBeVisible()
    // 只說「解析不到簽核人」使用者不知道要去哪裡修，
    // 那個提示就等於沒有用
    await expect(box).toContainText('組織健康檢查')
  })

  test('標記已處理後異常消失', async ({ page, request }) => {
    const { id, key } = await startInstance(request, 'RESOLVE')
    await raiseAttention(request, id)

    await loginAs(page, 'monitor@demo.local')
    await openInstance(page, key)

    await page.getByTestId('monitor-resolve-attention').click()

    await expect(page.getByTestId('monitor-action-message')).toContainText(
      '已標記為處理完成',
    )
    await expect(page.getByTestId('monitor-attention-box')).toHaveCount(0)
  })
})

test.describe('N1：介入動作', () => {
  test('催辦寄出提醒', async ({ page, request }) => {
    const { id, key } = await startInstance(request, 'REMIND')
    // 等待辦建立。沒有待辦時催辦會回 409，那是另一條路徑
    await expect
      .poll(
        async () => {
          const t = await token(request, 'monitor@demo.local')
          const res = await request.get(`${API}/instances/${id}/tasks`, {
            headers: { Authorization: `Bearer ${t}` },
          })
          return (await res.json()).length
        },
        { timeout: 15000 },
      )
      .toBeGreaterThan(0)

    await loginAs(page, 'monitor@demo.local')
    await openInstance(page, key)

    await page.getByTestId('monitor-remind').click()

    // 催辦會真的走 SMTP 寄信，實測 4～6 秒。
    // Playwright 預設的 5 秒剛好卡在中間，會時紅時綠——
    // 那種不穩定的測試比沒有測試更糟，因為紅了會被當成雜訊忽略。
    await expect(page.getByTestId('monitor-action-message')).toContainText(
      '已寄出',
      { timeout: 20000 },
    )
  })

  test('改派把待辦換人並清掉角色', async ({ page, request }) => {
    const { id, key } = await startInstance(request, 'REASSIGN')
    await expect
      .poll(
        async () => {
          const t = await token(request, 'monitor@demo.local')
          const res = await request.get(`${API}/instances/${id}/tasks`, {
            headers: { Authorization: `Bearer ${t}` },
          })
          return (await res.json()).length
        },
        { timeout: 15000 },
      )
      .toBeGreaterThan(0)

    await loginAs(page, 'monitor@demo.local')
    await openInstance(page, key)

    await page.getByTestId('monitor-reassign-manager_approval').click()

    const select = page.getByTestId('monitor-reassign-to')
    await expect(select).toBeVisible()
    // 選第二個（第一個是「選擇改派對象…」的空值）
    const target = await select.locator('option').nth(1).getAttribute('value')
    await select.selectOption(target!)

    await page.getByTestId('monitor-reassign-confirm').click()
    await expect(page.getByTestId('monitor-action-message')).toContainText(
      '已改派',
    )

    // 後端真的改了，不只是畫面顯示成功
    const t = await token(request, 'monitor@demo.local')
    const res = await request.get(`${API}/instances/${id}/tasks`, {
      headers: { Authorization: `Bearer ${t}` },
    })
    const tasks = await res.json()
    expect(tasks[0].assignee_user_id).toBe(target)
    // 不清角色的話原角色的人還看得到這張待辦，等於沒有改派
    expect(tasks[0].assignee_role).toBeNull()
  })

  test('一般使用者看不到介入按鈕', async ({ page, request }) => {
    // 發起人看得到自己的單，但不該有催辦、取消、改派
    const { key } = await startInstance(request, 'NOBTN')

    await loginAs(page, 'sales@demo.local')
    await openInstance(page, key)

    await expect(page.getByTestId('monitor-detail')).toBeVisible()
    await expect(page.getByTestId('monitor-remind')).toHaveCount(0)
    await expect(page.getByTestId('monitor-cancel')).toHaveCount(0)
  })
})
