/**
 * 同步中心 E2E
 *
 * 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §7、§10.3。
 *
 * 驗的三件事：
 *
 *   1. **待停用要有門檻**（§7 規則 3）。未達次數不可停用——
 *      一次匯出疏漏就停用人，正是這條規則要避免的
 *   2. **停用前置檢查要在清單上就看得到**（§7 規則 4）。
 *      是別人主管的人停用後，他部屬送單會找不到簽核人
 *   3. **同步歷史要看得出計數**（§10.3）。同步不能只回「success」
 *
 * 測試會匯入員工並操作他們的狀態，結束時全部刪掉。
 *
 * 需先啟動全部六個服務。
 */

import { expect, test, type Page } from '@playwright/test'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { tmpdir } from 'node:os'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/測試報告/202609/screenshots/sync-center')
mkdirSync(SHOTS, { recursive: true })

const API = 'http://localhost:3001'

async function shot(page: Page, name: string): Promise<void> {
  await page.waitForLoadState('networkidle')
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

async function login(page: Page, email: string): Promise<void> {
  await page.goto('/login')
  await page.getByTestId('login-tenant').fill('demo')
  await page.getByTestId('login-email').fill(email)
  await page.getByTestId('login-password').fill('demo1234')
  await page.getByTestId('login-submit').click()
  await expect(page).toHaveURL(/\/$|\/home/)
}

async function tokenOf(page: Page): Promise<string> {
  return page.evaluate(() => {
    const raw = localStorage.getItem('workflow.session')
    return raw ? (JSON.parse(raw).access_token as string) : ''
  })
}

/**
 * 直接用 API 準備資料
 *
 * 走 UI 匯入三次才能湊出待停用清單，那會讓每條測試跑 30 秒以上，
 * 而且測的是匯入而非這一頁。匯入的 UI 流程由 org-import.spec.ts 涵蓋。
 */
class Fixture {
  constructor(
    private page: Page,
    private token: string,
    readonly prefix: string,
  ) {}

  static async create(page: Page): Promise<Fixture> {
    const token = await tokenOf(page)
    const prefix = `S${Date.now().toString().slice(-6)}`
    const fixture = new Fixture(page, token, prefix)
    await fixture.resetSources()
    return fixture
  }

  private headers() {
    return { Authorization: `Bearer ${this.token}` }
  }

  /** 每條測試都要清。來源記著完整性閘的比較基準，會互相污染 */
  async resetSources(): Promise<void> {
    const resp = await this.page.request.get(`${API}/integration/sources`, {
      headers: this.headers(),
    })
    for (const s of (await resp.json()) as { id: string }[]) {
      await this.page.request.delete(`${API}/integration/sources/${s.id}`, {
        headers: this.headers(),
      })
    }
  }

  async createSource(missThreshold = 3): Promise<string> {
    const conns = await this.page.request.get(
      `${API}/integration/connections`,
      { headers: this.headers() },
    )
    const list = (await conns.json()) as { id: string; kind: string }[]
    let connectionId = list.find((c) => c.kind === 'FILE')?.id

    if (!connectionId) {
      const created = await this.page.request.post(
        `${API}/integration/connections`,
        { headers: this.headers(), data: { name: '測試連線', kind: 'FILE' } },
      )
      connectionId = ((await created.json()) as { id: string }).id
    }

    const source = await this.page.request.post(`${API}/integration/sources`, {
      headers: this.headers(),
      data: {
        connection_id: connectionId,
        dataset: 'employee',
        name: '測試來源',
        mapping: {
          工號: 'employee_no',
          姓名: 'name',
          信箱: 'email',
          主管工號: 'manager_employee_no',
        },
      },
    })
    const sourceId = ((await source.json()) as { id: string }).id

    // 門檻放寬讓少數人消失也能通過完整性閘
    await this.page.request.patch(`${API}/integration/sources/${sourceId}`, {
      headers: this.headers(),
      data: {
        completeness_threshold: 0.1,
        min_drop_threshold: 1,
        deactivation_miss_threshold: missThreshold,
      },
    })

    return sourceId
  }

  async sync(sourceId: string, content: string): Promise<void> {
    await this.page.request.post(
      `${API}/integration/sources/${sourceId}/sync`,
      { headers: this.headers(), data: { content } },
    )
  }

  /** 刪掉這一輪建立的所有員工與來源 */
  async cleanup(): Promise<void> {
    const list = await this.page.request.get(
      `${API}/org/employees?q=${this.prefix}&include_inactive=true`,
      { headers: this.headers() },
    )
    const employees = (await list.json()) as { id: string }[]

    // 先解除主管關係，否則「是其他員工的直屬主管」會擋下刪除
    for (const e of employees) {
      await this.page.request.patch(`${API}/org/employees/${e.id}`, {
        headers: this.headers(),
        data: { fields: ['manager_id'], manager_id: null },
      })
    }
    for (const e of employees) {
      await this.page.request.delete(`${API}/org/employees/${e.id}`, {
        headers: this.headers(),
      })
    }

    await this.resetSources()
  }
}

test('待停用清單顯示次數與門檻', async ({ page }) => {
  await login(page, 'admin@demo.local')
  const f = await Fixture.create(page)
  const source = await f.createSource(3)

  const three =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}A,同步甲,${f.prefix.toLowerCase()}a@demo.local,\n` +
    `${f.prefix}B,同步乙,${f.prefix.toLowerCase()}b@demo.local,\n` +
    `${f.prefix}C,同步丙,${f.prefix.toLowerCase()}c@demo.local,\n`
  await f.sync(source, three)

  // C 消失一次
  const two =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}A,同步甲,${f.prefix.toLowerCase()}a@demo.local,\n` +
    `${f.prefix}B,同步乙,${f.prefix.toLowerCase()}b@demo.local,\n`
  await f.sync(source, two)

  await page.goto('/designer/sync-center')
  await expect(page.getByTestId('sync-pending-table')).toBeVisible()
  await shot(page, '01-待停用清單')

  const row = page.locator('tr', { hasText: `${f.prefix}C` }).first()
  await expect(row).toContainText('1 / 3')
  await expect(row).toContainText('還差 2 次')

  await f.cleanup()
})

test('未達門檻時停用鈕不可用', async ({ page }) => {
  await login(page, 'admin@demo.local')
  const f = await Fixture.create(page)
  const source = await f.createSource(3)

  const two =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}A,同步甲,${f.prefix.toLowerCase()}a@demo.local,\n` +
    `${f.prefix}B,同步乙,${f.prefix.toLowerCase()}b@demo.local,\n`
  await f.sync(source, two)

  const one =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}A,同步甲,${f.prefix.toLowerCase()}a@demo.local,\n`
  await f.sync(source, one)

  await page.goto('/designer/sync-center')
  await expect(page.getByTestId('sync-pending-table')).toBeVisible()

  // 一次匯出疏漏就停用人，正是 §7 規則 3 要避免的
  const row = page.locator('tr', { hasText: `${f.prefix}B` }).first()
  await expect(
    row.locator('[data-testid^="pending-confirm-"]'),
  ).toBeDisabled()

  await f.cleanup()
})

test('達到門檻後可以停用', async ({ page }) => {
  await login(page, 'admin@demo.local')
  const f = await Fixture.create(page)
  // 門檻設成 1，一次消失就可處理
  const source = await f.createSource(1)

  const two =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}A,同步甲,${f.prefix.toLowerCase()}a@demo.local,\n` +
    `${f.prefix}B,同步乙,${f.prefix.toLowerCase()}b@demo.local,\n`
  await f.sync(source, two)

  const one =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}A,同步甲,${f.prefix.toLowerCase()}a@demo.local,\n`
  await f.sync(source, one)

  await page.goto('/designer/sync-center')
  await expect(page.getByTestId('sync-pending-table')).toBeVisible()

  // 用工號前綴定位。姓名在各條測試間重複，而完整套跑時清單上
  // 可能有別條測試留下的紀錄——斷言「整個清單空」會因為執行順序
  // 而時好時壞
  const row = page.locator('tr', { hasText: `${f.prefix}B` }).first()
  const button = row.locator('[data-testid^="pending-confirm-"]')
  await expect(button).toBeEnabled()
  await button.click()

  // 處理完就從清單消失
  await expect(page.locator('tr', { hasText: `${f.prefix}B` })).toHaveCount(0)
  await shot(page, '02-停用後清單清空')

  await f.cleanup()
})

test('是別人主管的人不可停用，理由在清單上', async ({ page }) => {
  await login(page, 'admin@demo.local')
  const f = await Fixture.create(page)
  const source = await f.createSource(1)

  // 甲是乙的主管
  const two =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}A,同步甲,${f.prefix.toLowerCase()}a@demo.local,\n` +
    `${f.prefix}B,同步乙,${f.prefix.toLowerCase()}b@demo.local,${f.prefix}A\n`
  await f.sync(source, two)

  // 甲消失
  const onlyB =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}B,同步乙,${f.prefix.toLowerCase()}b@demo.local,\n`
  await f.sync(source, onlyB)

  await page.goto('/designer/sync-center')
  await expect(page.getByTestId('sync-pending-table')).toBeVisible()

  const row = page.locator('tr', { hasText: `${f.prefix}A` }).first()
  // 理由要在清單上就看得到，不是按了才知道
  await expect(row).toContainText('直屬主管')
  await expect(
    row.locator('[data-testid^="pending-confirm-"]'),
  ).toBeDisabled()
  await shot(page, '03-主管不可停用')

  await f.cleanup()
})

test('標記為仍在職後從清單移除', async ({ page }) => {
  await login(page, 'admin@demo.local')
  const f = await Fixture.create(page)
  const source = await f.createSource(3)

  const two =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}A,同步甲,${f.prefix.toLowerCase()}a@demo.local,\n` +
    `${f.prefix}B,同步乙,${f.prefix.toLowerCase()}b@demo.local,\n`
  await f.sync(source, two)

  const one =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}A,同步甲,${f.prefix.toLowerCase()}a@demo.local,\n`
  await f.sync(source, one)

  await page.goto('/designer/sync-center')
  await expect(page.getByTestId('sync-pending-table')).toBeVisible()

  const row = page.locator('tr', { hasText: `${f.prefix}B` }).first()
  await row.locator('[data-testid^="pending-dismiss-"]').click()
  await expect(page.locator('tr', { hasText: `${f.prefix}B` })).toHaveCount(0)

  await f.cleanup()
})

test('同步歷史顯示計數，可展開問題明細', async ({ page }) => {
  await login(page, 'admin@demo.local')
  const f = await Fixture.create(page)
  const source = await f.createSource(3)

  // 其中一列缺工號，會產生問題明細
  const withBadRow =
    '工號,姓名,信箱,主管工號\n' +
    `${f.prefix}A,同步甲,${f.prefix.toLowerCase()}a@demo.local,\n` +
    `,沒有工號,${f.prefix.toLowerCase()}x@demo.local,\n`
  await f.sync(source, withBadRow)

  await page.goto('/designer/sync-center')
  await page.getByTestId('sync-tab-history').click()
  await expect(page.getByTestId('sync-history-table')).toBeVisible()

  // 最新的一筆就是這條測試剛跑的。歷史依時間倒序
  const row = page.locator('[data-testid^="run-row-"]').first()
  // 同步不能只回 success，管理員要看得出兩邊差異
  await expect(row).toContainText('新增')
  await expect(row).toContainText('擋下 1')
  await shot(page, '04-同步歷史')

  // 點開看明細
  await row.click()
  const issues = page.locator('[data-testid^="run-issues-"]').first()
  await expect(issues).toBeVisible()
  await expect(issues).toContainText('缺少員工編號')
  await shot(page, '05-問題明細')

  await f.cleanup()
})

test('試跑的紀錄要標示出來', async ({ page }) => {
  await login(page, 'admin@demo.local')
  const f = await Fixture.create(page)
  const source = await f.createSource(3)

  const token = await tokenOf(page)
  await page.request.post(`${API}/integration/sources/${source}/preview`, {
    headers: { Authorization: `Bearer ${token}` },
    data: {
      content:
        '工號,姓名,信箱,主管工號\n' +
        `${f.prefix}A,試跑甲,${f.prefix.toLowerCase()}a@demo.local,\n`,
    },
  })

  await page.goto('/designer/sync-center')
  await page.getByTestId('sync-tab-history').click()
  await expect(page.getByTestId('sync-history-table')).toBeVisible()

  // 不標示的話會被誤認為真的同步過
  await expect(page.locator('[data-testid^="run-row-"]').first()).toContainText(
    '試跑',
  )

  await f.cleanup()
})

test('designer 不能使用同步中心', async ({ page }) => {
  await login(page, 'designer@demo.local')
  await page.goto('/designer/sync-center')

  await expect(page.getByTestId('sync-readonly-hint')).toBeVisible()
  // 後端擋下，畫面顯示錯誤而非空白
  await expect(page.getByTestId('sync-error')).toBeVisible()
  await shot(page, '06-designer唯讀')
})
