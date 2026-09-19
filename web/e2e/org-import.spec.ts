/**
 * 組織資料匯入 E2E
 *
 * 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §8、§12.2。
 *
 * 驗的三件事：
 *
 *   1. **四個步驟不能跳**。正式寫入的按鈕要等試跑完成才會出現——
 *      §12.1 說「沒有試跑，管理員第一次同步就是盲賭」
 *   2. **欄位對應建議要準**，而且人工可改
 *   3. **完整性閘擋得住**。筆數驟減時整批中止，寫入按鈕不可用
 *
 * 測試會寫入員工資料，結束時刪掉。用 T 開頭的工號與時間戳
 * 避免與 seed 衝突。
 *
 * 需先啟動全部六個服務。
 */

import { expect, test, type Page } from '@playwright/test'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { tmpdir } from 'node:os'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SHOTS = resolve(HERE, '../../docs/測試報告/202609/screenshots/org-import')
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

/** 取出目前登入者的 token，供直接打 API 用 */
async function tokenOf(page: Page): Promise<string> {
  return page.evaluate(() => {
    const raw = localStorage.getItem('workflow.session')
    return raw ? (JSON.parse(raw).access_token as string) : ''
  })
}

/**
 * 清掉既有的匯入來源
 *
 * **每條測試開頭都要做。** 來源記著 `last_success_count`，
 * 那是完整性閘的比較基準。上一條測試若同步了 4 筆，
 * 下一條測試匯入 2 筆就會被擋下——測試之間會互相污染。
 *
 * 同時也是收尾：跑完不留東西在 demo 租戶裡。
 */
async function resetSources(page: Page): Promise<void> {
  const token = await tokenOf(page)
  const resp = await page.request.get(`${API}/integration/sources`, {
    headers: { Authorization: `Bearer ${token}` },
  })
  for (const s of (await resp.json()) as { id: string }[]) {
    await page.request.delete(`${API}/integration/sources/${s.id}`, {
      headers: { Authorization: `Bearer ${token}` },
    })
  }
}

/** 把 CSV 寫成暫存檔供 input[type=file] 使用 */
function writeCsv(name: string, content: string): string {
  const path = resolve(tmpdir(), name)
  // 加 BOM——Excel 另存 UTF-8 CSV 會加，後端要去得掉
  writeFileSync(path, '﻿' + content, 'utf8')
  return path
}

/** 這一輪測試用的工號前綴，避免與 seed 及其他測試衝突 */
const PREFIX = `T${Date.now().toString().slice(-6)}`

function csvTwo(): string {
  return (
    '工號,姓名,信箱,部門,職稱,主管工號\n' +
    `${PREFIX}A,匯入測試甲,${PREFIX.toLowerCase()}a@demo.local,sales,工程師,\n` +
    `${PREFIX}B,匯入測試乙,${PREFIX.toLowerCase()}b@demo.local,qa,專員,${PREFIX}A\n`
  )
}

/** 收尾：刪掉匯入的人 */
async function cleanup(page: Page): Promise<void> {
  const token = await tokenOf(page)

  const list = await page.request.get(`${API}/org/employees?q=${PREFIX}`, {
    headers: { Authorization: `Bearer ${token}` },
  })
  const employees = (await list.json()) as { id: string; employee_no: string }[]

  // 先解除主管關係，再刪。匯入的資料裡「甲」是「乙」的主管，
  // 直接刪會因為「此人是其他員工的直屬主管」而擋下——
  // 而且擋下的順序取決於迴圈，結果不穩定
  for (const e of employees) {
    await page.request.patch(`${API}/org/employees/${e.id}`, {
      headers: { Authorization: `Bearer ${token}` },
      data: { fields: ['manager_id'], manager_id: null },
    })
  }

  for (const e of employees) {
    await page.request.delete(`${API}/org/employees/${e.id}`, {
      headers: { Authorization: `Bearer ${token}` },
    })
  }

  await resetSources(page)
}

test('四個步驟：上傳、對應、試跑、寫入', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await resetSources(page)

  await page.getByTestId('nav-designer-toggle').click()
  await page.getByRole('link', { name: '組織資料匯入' }).click()
  await expect(page).toHaveURL(/\/designer\/org-import/)
  await shot(page, '01-匯入起始畫面')

  // 步驟 1：上傳
  await page
    .getByTestId('import-file')
    .setInputFiles(writeCsv(`${PREFIX}-two.csv`, csvTwo()))

  // 步驟 2：欄位對應。建議要自動填好
  await expect(page.getByTestId('import-mapping-section')).toBeVisible()
  await expect(page.getByTestId('mapping-select-0')).toHaveValue('employee_no')
  await expect(page.getByTestId('mapping-select-1')).toHaveValue('name')
  // 「主管工號」不可以被 employee_no 吃掉
  await expect(page.getByTestId('mapping-select-5')).toHaveValue(
    'manager_employee_no',
  )
  await shot(page, '02-欄位對應建議')

  // 步驟 3：試跑。寫入按鈕在試跑前不存在
  await expect(page.getByTestId('import-commit')).toHaveCount(0)
  await page.getByTestId('import-preview').click()

  await expect(page.getByTestId('import-preview-result')).toBeVisible()
  await expect(page.getByTestId('import-preview-status')).toContainText('成功')
  await shot(page, '03-試跑結果')

  // 步驟 4：寫入
  await page.getByTestId('import-commit').click()
  await expect(page.getByTestId('import-sync-result')).toBeVisible()
  await expect(page.getByTestId('import-sync-status')).toContainText('成功')
  await shot(page, '04-寫入完成')

  // 真的寫進去了，而且主管關係回填正確
  await page.goto('/designer/organization')
  await page.getByTestId('org-search').fill(`${PREFIX}B`)
  const row = page.locator('[data-testid^="org-employee-row-"]').first()
  await expect(row).toContainText('匯入測試乙')
  await expect(row).toContainText('匯入測試甲')

  await cleanup(page)
})

test('試跑不寫入任何資料', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await resetSources(page)
  await page.goto('/designer/org-import')

  const prefix = `P${Date.now().toString().slice(-6)}`
  const csv =
    '工號,姓名,信箱,部門,職稱,主管工號\n' +
    `${prefix}A,只試跑不寫入,${prefix.toLowerCase()}@demo.local,sales,工程師,\n`

  await page
    .getByTestId('import-file')
    .setInputFiles(writeCsv(`${prefix}.csv`, csv))
  await page.getByTestId('import-preview').click()
  await expect(page.getByTestId('import-preview-result')).toBeVisible()

  // 試跑說會新增 1 筆
  await expect(page.getByTestId('import-preview-result')).toContainText('新增')

  // 但組織裡找不到那個人
  await page.goto('/designer/organization')
  await page.getByTestId('org-search').fill(prefix)
  await expect(page.getByTestId('org-employees-empty')).toBeVisible()
})

test('沒有對應員工編號時不能試跑', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await resetSources(page)
  await page.goto('/designer/org-import')

  const csv = '姓名,職稱\n某某,工程師\n'
  await page
    .getByTestId('import-file')
    .setInputFiles(writeCsv('no-emp-no.csv', csv))

  await expect(page.getByTestId('import-mapping-section')).toBeVisible()
  // 沒有它，同步兩次會產生兩份資料
  await expect(page.getByTestId('import-no-employee-no')).toBeVisible()
  await expect(page.getByTestId('import-preview')).toBeDisabled()
  await shot(page, '05-缺少員工編號')
})

test('手動改對應後警告消失', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await resetSources(page)
  await page.goto('/designer/org-import')

  const csv = '代號,姓名\nX001,某某\n'
  await page.getByTestId('import-file').setInputFiles(writeCsv('manual.csv', csv))
  await expect(page.getByTestId('import-mapping-section')).toBeVisible()

  // 「代號」猜不出來——建議只是省打字，人工確認才是關鍵
  await expect(page.getByTestId('import-no-employee-no')).toBeVisible()

  await page.getByTestId('mapping-select-0').selectOption('employee_no')
  await expect(page.getByTestId('import-no-employee-no')).toHaveCount(0)
  await expect(page.getByTestId('import-preview')).toBeEnabled()
})

test('同一個欄位被對應兩次時警告', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await resetSources(page)
  await page.goto('/designer/org-import')

  const csv = '工號,員工代號\nX001,X001\n'
  await page.getByTestId('import-file').setInputFiles(writeCsv('dup.csv', csv))
  await expect(page.getByTestId('import-mapping-section')).toBeVisible()

  // 手動把第二欄也指到 employee_no
  await page.getByTestId('mapping-select-1').selectOption('employee_no')

  // 兩欄對到同一個目標時，後面的會覆蓋前面的，而同步照樣成功
  await expect(page.getByTestId('mapping-duplicate-0')).toBeVisible()
  await expect(page.getByTestId('mapping-duplicate-1')).toBeVisible()
  await shot(page, '06-重複對應警告')
})

test('完整性閘擋下筆數驟減的檔案', async ({ page }) => {
  await login(page, 'admin@demo.local')
  await resetSources(page)
  await page.goto('/designer/org-import')

  const prefix = `G${Date.now().toString().slice(-6)}`

  // 15 人。少到 1 人時減少 14 人，超過預設的絕對值門檻 10——
  // Q-02 改成雙條件後，只少 3 人不再觸發（那是小公司的正常異動）
  const many =
    '工號,姓名,信箱,部門,職稱,主管工號\n' +
    Array.from({ length: 15 }, (_, index) => index + 1)
      .map(
        (i) =>
          `${prefix}${i},閘測試${i},${prefix.toLowerCase()}${i}@demo.local,sales,工程師,\n`,
      )
      .join('')

  // 第一次：15 人寫入成功
  await page
    .getByTestId('import-file')
    .setInputFiles(writeCsv(`${prefix}-many.csv`, many))
  await page.getByTestId('import-preview').click()
  await expect(page.getByTestId('import-preview-result')).toBeVisible()
  await page.getByTestId('import-commit').click()
  await expect(page.getByTestId('import-sync-status')).toContainText('成功')

  // 第二次：同一個來源，只剩 1 人
  const one =
    '工號,姓名,信箱,部門,職稱,主管工號\n' +
    `${prefix}1,閘測試1,${prefix.toLowerCase()}1@demo.local,sales,工程師,\n`

  await page.reload()
  await page
    .getByTestId('import-file')
    .setInputFiles(writeCsv(`${prefix}-one.csv`, one))
  await page.getByTestId('import-preview').click()
  await expect(page.getByTestId('import-preview-result')).toBeVisible()

  // 這是整個同步最重要的保護
  await expect(page.getByTestId('import-preview-status')).toContainText('整批中止')
  await expect(page.getByTestId('import-preview-message')).toContainText(
    '未寫入任何變更',
  )
  await expect(page.getByTestId('import-commit')).toBeDisabled()
  await shot(page, '07-完整性閘擋下')

  // 收尾
  const token = await tokenOf(page)
  const list = await page.request.get(`${API}/org/employees?q=${prefix}`, {
    headers: { Authorization: `Bearer ${token}` },
  })
  for (const e of (await list.json()) as { id: string }[]) {
    await page.request.delete(`${API}/org/employees/${e.id}`, {
      headers: { Authorization: `Bearer ${token}` },
    })
  }
  await resetSources(page)
})

test('designer 看得到頁面但不能匯入', async ({ page }) => {
  await login(page, 'designer@demo.local')
  await page.goto('/designer/org-import')

  // 匯入會改動整份組織資料，而組織決定簽核路徑
  await expect(page.getByTestId('import-readonly-hint')).toBeVisible()
  await expect(page.getByTestId('import-file')).toBeDisabled()
  await shot(page, '08-designer唯讀')
})
