/**
 * 前置改動 E2E：WF-E012 路徑存在性驗證 + 權限預覽帶真實單據
 *
 * 這兩項是「編輯開發模式（沙箱）」與「AI 產生表單與流程」的前置條件，
 * 規劃見 docs/實踐規劃/規劃/202609/01_前置改動實踐規劃.md。
 *
 * 為什麼要有 E2E 而不只是單元測試：
 *   WF-E012 的價值在於「使用者真的無法發布一個會靜默跳過簽核的流程」。
 *   單元測試只驗到 validate_graph 回傳錯誤碼，驗不到 API 是否真的擋下發布。
 *   本專案已有前例——SMTP 的單元測試驗到「有呼叫 send_notification」就算過，
 *   但實際欄位漏傳，信寄出去標題是錯的。
 *
 * 需先啟動全部六個服務。流程 key 帶時間戳避免與既有資料衝突。
 */

import { expect, test, type APIRequestContext } from '@playwright/test'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { cancelTrackedInstances, trackInstance } from './support/instances'

const HERE = dirname(fileURLToPath(import.meta.url))

// 這批改動全在後端（validator 與預覽 API），沒有畫面異動，
// 所以本檔是 API 層的 E2E 而非操作流程的 E2E，不附截圖。
// 截圖要等 S0 流程監控有畫面之後才有意義。
const API = 'http://localhost:3001'

const FIXTURE = resolve(
  HERE,
  '../../schemas/fixtures/quotation_approval_v1.json',
)

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

function fixtureDsl(): Record<string, unknown> {
  return JSON.parse(readFileSync(FIXTURE, 'utf-8'))
}

/** 建立流程定義，回傳 workflow_key */
async function createWorkflow(
  request: APIRequestContext,
  jwt: string,
  key: string,
  content: Record<string, unknown>,
): Promise<void> {
  const res = await request.post(`${API}/workflows`, {
    headers: { Authorization: `Bearer ${jwt}` },
    data: {
      workflow_key: key,
      business_object: 'quotation',
      name: `E012 測試 ${key}`,
      content: { ...content, workflow_key: key },
    },
  })
  expect(res.status(), '建立流程定義失敗').toBe(201)
}

// 本檔只啟動一張單（權限預覽那條），但同樣要收掉——
// 累積是逐次發生的，一次一張也會累積。
test.afterAll(async ({ request }) => {
  const jwt = await token(request, 'designer@demo.local')
  await cancelTrackedInstances(request, jwt)
})

test.describe('WF-E012 資料路徑存在性', () => {
  test('條件式路徑打錯字時，發布被擋下', async ({ request }) => {
    // 最危險的情境：discount_rat 少一個字母。
    // 沒有 E012 的話求值取不到值 → 當成 false → 高折扣報價單
    // 靜默跳過財務簽核，且沒有任何錯誤訊息。
    const jwt = await token(request, 'designer@demo.local')
    const key = `e012_typo_${Date.now()}`

    const dsl = fixtureDsl()
    for (const node of dsl.nodes as Array<Record<string, unknown>>) {
      if (node.type === 'condition') {
        node.expression = 'quotation.discount_rat > 0.15'
      }
    }

    await createWorkflow(request, jwt, key, dsl)

    const res = await request.post(`${API}/workflows/${key}/draft/publish`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })

    expect(res.status(), '打錯字的路徑應擋下發布').toBe(422)

    const body = await res.json()
    expect(JSON.stringify(body)).toContain('WF-E012')
    expect(JSON.stringify(body)).toContain('quotation.discount_rat')
  })

  test('resolver 路徑不存在時，發布被擋下', async ({ request }) => {
    // resolver 取不到值會讓流程 NoParticipants → FAILED。
    // 那是明確失敗（比靜默跳過安全），但在發布前擋下仍優於上線後第一張單卡住。
    const jwt = await token(request, 'designer@demo.local')
    const key = `e012_resolver_${Date.now()}`

    const dsl = fixtureDsl()
    for (const node of dsl.nodes as Array<Record<string, unknown>>) {
      const resolver = node.resolver as Record<string, unknown> | undefined
      if (resolver?.path) {
        resolver.path = 'quotation.wrong_contacts'
      }
    }

    await createWorkflow(request, jwt, key, dsl)

    const res = await request.post(`${API}/workflows/${key}/draft/publish`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })

    expect(res.status(), 'resolver 路徑錯誤應擋下發布').toBe(422)
    expect(JSON.stringify(await res.json())).toContain('WF-E012')
  })

  test('正確的路徑可以正常發布', async ({ request }) => {
    // 回歸：E012 不可誤擋合法流程。
    // 誤擋會讓使用者無法發布正確的流程，比漏擋更快失去信任。
    const jwt = await token(request, 'designer@demo.local')
    const key = `e012_ok_${Date.now()}`

    await createWorkflow(request, jwt, key, fixtureDsl())

    const res = await request.post(`${API}/workflows/${key}/draft/publish`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })

    expect(res.status(), 'fixture 的路徑都合法，應可發布').toBe(200)
    expect((await res.json()).version).toBe(1)
  })

  test('驗證端點逐條回報錯誤，供設計器標記節點', async ({ request }) => {
    // validate_draft 回 200 加結果物件而非 4xx，
    // 前端需要逐條錯誤來標記是哪個節點出問題。
    const jwt = await token(request, 'designer@demo.local')
    const key = `e012_validate_${Date.now()}`

    const dsl = fixtureDsl()
    for (const node of dsl.nodes as Array<Record<string, unknown>>) {
      if (node.type === 'condition') {
        node.expression = 'quotation.nonexistent_field > 1'
      }
    }

    await createWorkflow(request, jwt, key, dsl)

    const res = await request.post(`${API}/workflows/${key}/draft/validate`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })
    expect(res.status(), '驗證端點回 200 加結果物件，不用 4xx 表達驗證失敗').toBe(
      200,
    )

    const text = JSON.stringify(await res.json())
    expect(text, '應指出是哪個節點').toContain('WF-E012')
    expect(text).toContain('nonexistent_field')
  })
})

test.describe('權限預覽帶真實單據', () => {
  test('不帶 instance_id 時行為與先前相同', async ({ request }) => {
    // 回歸：設計器預覽不傳 instance_id，data 為空物件。
    const jwt = await token(request, 'designer@demo.local')

    const res = await request.get(
      `${API}/forms/quotation_form/permissions/preview?roles=designer`,
      { headers: { Authorization: `Bearer ${jwt}` } },
    )

    expect(res.status(), '設計器預覽應照舊可用').toBe(200)
    expect(Array.isArray(await res.json())).toBeTruthy()
  })

  test('不存在的 instance_id 回 404 而非空物件', async ({ request }) => {
    // 默默退回空物件會讓模擬畫面判錯而無人察覺——
    // 正是本專案一再踩到的靜默失敗類型。
    const jwt = await token(request, 'designer@demo.local')

    const res = await request.get(
      `${API}/forms/quotation_form/permissions/preview` +
        `?roles=designer&instance_id=00000000-0000-0000-0000-000000000000`,
      { headers: { Authorization: `Bearer ${jwt}` } },
    )

    expect(res.status(), '查不到實例應回 404').toBe(404)
  })

  test('帶真實 instance_id 時可取得權限結果', async ({ request }) => {
    // 先啟動一張真實報價單，再用它的 id 查權限預覽。
    const jwt = await token(request, 'designer@demo.local')
    const businessKey = `QT-E012-${Date.now()}`

    const started = await request.post(`${API}/instances`, {
      headers: { Authorization: `Bearer ${jwt}` },
      data: {
        workflow_key: 'quotation_approval',
        business_key: businessKey,
        input: {
          quotation: {
            discount_rate: 0.2,
            total: 2_000_000,
            customer_name: '台灣松下精密機械股份有限公司',
            customer_contact_ids: ['cust@example.com'],
          },
        },
      },
    })
    expect(started.status(), '啟動流程失敗').toBe(201)
    const instanceId = (await started.json()).id

    // 這張單不會被簽完，收掉它——見 support/instances.ts
    trackInstance(instanceId)

    const res = await request.get(
      `${API}/forms/quotation_form/permissions/preview` +
        `?roles=designer&instance_id=${instanceId}`,
      { headers: { Authorization: `Bearer ${jwt}` } },
    )

    expect(res.status(), '帶真實單據的預覽應成功').toBe(200)
    const fields = await res.json()
    expect(Array.isArray(fields)).toBeTruthy()
    expect(fields.length, '應回傳欄位權限清單').toBeGreaterThan(0)
  })
})

/**
 * 未定義的業務物件要明確警告
 *
 * `paths_for()` 查無業務物件時回空集合，而空集合在 WF-E012 代表
 * 「不檢查」（沿用 known_roles 的既有慣例）。所以建一個業務物件是
 * expense_claim 的流程，條件式打錯字也不會被擋——而畫面上沒有
 * 任何提示，使用者以為自己受 E012 保護。
 *
 * 這是 E012 本身要擋的那類靜默失敗，只是換了一層。
 */
test.describe('未定義的業務物件', () => {
  /** 建立業務物件為 unknown_object 的流程 */
  async function createWithUnknownObject(
    request: APIRequestContext,
    jwt: string,
    key: string,
  ): Promise<void> {
    const dsl = fixtureDsl()
    const res = await request.post(`${API}/workflows`, {
      headers: { Authorization: `Bearer ${jwt}` },
      data: {
        workflow_key: key,
        business_object: 'expense_claim',
        name: `未定義業務物件測試 ${key}`,
        content: { ...dsl, workflow_key: key, business_object: 'expense_claim' },
      },
    })
    expect(res.status(), '建立流程定義失敗').toBe(201)
  }

  test('驗證時回報警告', async ({ request }) => {
    const jwt = await token(request, 'designer@demo.local')
    const key = `unknown_bo_validate_${Date.now()}`
    await createWithUnknownObject(request, jwt, key)

    const res = await request.post(
      `${API}/workflows/${key}/draft/validate`,
      { headers: { Authorization: `Bearer ${jwt}` } },
    )

    expect(res.status()).toBe(200)
    const body = await res.json()

    // 警告與錯誤要分開：警告不影響 valid，使用者仍可發布
    expect(body.warnings, '應回報警告').toBeDefined()
    expect(JSON.stringify(body.warnings)).toContain('expense_claim')
  })

  test('發布成功但回報警告', async ({ request }) => {
    const jwt = await token(request, 'designer@demo.local')
    const key = `unknown_bo_publish_${Date.now()}`
    await createWithUnknownObject(request, jwt, key)

    const res = await request.post(`${API}/workflows/${key}/draft/publish`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })

    // 不擋下發布——新業務物件尚未定義時仍應可用，
    // 否則使用者要先請人幫他建 JSON 定義才能做事
    expect(res.status(), '未定義的業務物件不該擋下發布').toBe(200)

    const body = await res.json()
    expect(body.warnings, '應回報警告').toBeDefined()
    expect(JSON.stringify(body.warnings)).toContain('expense_claim')
  })

  test('已定義的業務物件不產生警告', async ({ request }) => {
    const jwt = await token(request, 'designer@demo.local')
    const key = `known_bo_${Date.now()}`
    await createWorkflow(request, jwt, key, fixtureDsl())

    const res = await request.post(
      `${API}/workflows/${key}/draft/validate`,
      { headers: { Authorization: `Bearer ${jwt}` } },
    )

    expect(res.status()).toBe(200)
    const body = await res.json()
    expect(
      body.warnings === undefined || body.warnings.length === 0,
      'quotation 有定義，不該有警告',
    ).toBeTruthy()
  })
})

/**
 * 表單的業務物件警告
 *
 * 表單不像流程會引用路徑，所以沒有 WF-E012 那種靜默跳過簽核的風險。
 * 但它有另一個：**欄位的 data.path 決定日後流程能引用什麼**。
 *
 * 路徑寫成 quotation.total_amount（舊名稱）時，表單本身能用，
 * 但流程的條件式 quotation.total > x 就取不到值——
 * 而這要等到流程跑起來才會發現，那時已經太晚。
 *
 * 所以表單發布時回報兩種警告：
 *   1. 業務物件未定義（之後建流程時路徑不會被檢查）
 *   2. 欄位路徑不在正式定義裡（流程可能引用不到）
 */
test.describe('表單的業務物件警告', () => {
  /** 建立表單，回傳 form_key */
  async function createForm(
    request: APIRequestContext,
    jwt: string,
    key: string,
    businessObject: string,
    fieldPath: string,
  ): Promise<void> {
    const res = await request.post(`${API}/forms`, {
      headers: { Authorization: `Bearer ${jwt}` },
      data: {
        form_key: key,
        business_object: businessObject,
        name: `警告測試 ${key}`,
        content: {
          form_key: key,
          version: 1,
          business_object: businessObject,
          name: `警告測試 ${key}`,
          layout: { columns: 2 },
          sections: [{ key: 'main', title: '基本資料' }],
          fields: [
            {
              key: 'test_field',
              section: 'main',
              ui: { component: 'input', label: '測試欄位' },
              data: { path: fieldPath, type: 'string' },
            },
          ],
        },
      },
    })
    expect(res.status(), '建立表單失敗').toBe(201)
  }

  test('業務物件未定義時回報警告', async ({ request }) => {
    const jwt = await token(request, 'designer@demo.local')
    const key = `form_unknown_bo_${Date.now()}`
    await createForm(request, jwt, key, 'expense_claim', 'expense_claim.amount')

    const res = await request.post(`${API}/forms/${key}/draft/validate`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })

    expect(res.status()).toBe(200)
    const body = await res.json()
    expect(body.warnings, '應回報警告').toBeDefined()
    expect(JSON.stringify(body.warnings)).toContain('expense_claim')
  })

  test('欄位路徑不在正式定義時回報警告', async ({ request }) => {
    const jwt = await token(request, 'designer@demo.local')
    const key = `form_bad_path_${Date.now()}`

    // total_amount 是已淘汰的舊名稱，正式名稱是 quotation.total。
    // 表單用舊名稱存得起來，但流程的條件式引用 quotation.total 會取不到值。
    await createForm(request, jwt, key, 'quotation', 'quotation.total_amount')

    const res = await request.post(`${API}/forms/${key}/draft/validate`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })

    expect(res.status()).toBe(200)
    const body = await res.json()
    expect(body.warnings, '應回報警告').toBeDefined()
    expect(JSON.stringify(body.warnings)).toContain('quotation.total_amount')
  })

  test('發布成功但回報警告', async ({ request }) => {
    const jwt = await token(request, 'designer@demo.local')
    const key = `form_publish_warn_${Date.now()}`
    await createForm(request, jwt, key, 'expense_claim', 'expense_claim.amount')

    const res = await request.post(`${API}/forms/${key}/draft/publish`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })

    // 不擋下發布——新業務物件尚未定義時仍應可用
    expect(res.status(), '未定義的業務物件不該擋下發布').toBe(200)
    const body = await res.json()
    expect(body.warnings, '應回報警告').toBeDefined()
  })

  test('路徑正確時不產生警告', async ({ request }) => {
    const jwt = await token(request, 'designer@demo.local')
    const key = `form_ok_${Date.now()}`
    await createForm(request, jwt, key, 'quotation', 'quotation.total')

    const res = await request.post(`${API}/forms/${key}/draft/validate`, {
      headers: { Authorization: `Bearer ${jwt}` },
    })

    expect(res.status()).toBe(200)
    const body = await res.json()
    expect(
      body.warnings === undefined || body.warnings.length === 0,
      'quotation.total 是正式路徑，不該有警告',
    ).toBeTruthy()
  })
})
