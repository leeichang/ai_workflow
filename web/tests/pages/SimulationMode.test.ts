/**
 * 模擬簽核畫面測試
 *
 * 使用者的原話：
 *   「執行時可以直接由系統依據目前的系統角色設定解析出實際簽核的人員，
 *     測試的人可以直接點選模擬這個人簽核」
 *
 * 所以這個畫面的核心是：
 *   1. 顯示流程走到哪、誰在簽（真實解析出來的人，不是假資料）
 *   2. 點一下就以那個人的身分簽核，不換身分登入
 *
 * 另一個硬性要求是**不可關閉的開發模式標示**——
 * 使用者在開發模式核准了，若以為正式也核准了，那比不給模擬更糟。
 */

import { HttpResponse, http } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { server } from '../msw/server'
import SimulationMode from '@/pages/SimulationMode.vue'
import { useSession } from '@/auth/useSession'

const API = '*/api'
const SANDBOX_ID = 'sb-1'

const push = vi.fn()
vi.mock('vue-router', () => ({
  useRouter: () => ({ push }),
  useRoute: () => ({ path: '/simulation', params: {}, query: {} }),
  RouterLink: { template: '<a><slot /></a>' },
}))

function session(over: Record<string, unknown> = {}) {
  return {
    id: 'ss-1',
    tenant_id: 't1',
    sandbox_tenant_id: SANDBOX_ID,
    created_by: 'u1',
    created_by_in_sandbox: 'u1-sb',
    status: 'ACTIVE',
    created_at: '2026-09-19T00:00:00Z',
    expires_at: '2026-09-26T00:00:00Z',
    ...over,
  }
}

function task(over: Record<string, unknown> = {}) {
  return {
    task_id: 'tk-1',
    instance_id: 'in-1',
    business_key: 'QT-SIM-1',
    node_id: 'manager_approval',
    node_label: '主管簽核',
    assignee_user_id: 'cfo-id',
    assignee_name: '張文華',
    assignee_role: 'cfo',
    // 後端一律回這兩個欄位
    branch: null,
    timeout: null,
    ...over,
  }
}

function login() {
  localStorage.setItem(
    'workflow.session',
    JSON.stringify({
      access_token: 'h.p.s',
      user: {
        id: 'u1',
        name: '陳雅婷',
        email: 'designer@demo.local',
        tenant_id: 't1',
        roles: ['designer'],
      },
    }),
  )
  useSession().restore()
}

function formView(over: Record<string, unknown> = {}) {
  return {
    form_key: 'quotation_form',
    acting_as_roles: ['approver', 'cfo'],
    acting_as_name: '張文華',
    fields: [
      {
        key: 'customer_name',
        permission: 'READONLY',
        required: false,
        reason: '系統計算欄位',
      },
      {
        key: 'approval_comment',
        permission: 'EDITABLE',
        required: true,
        reason: '',
      },
      {
        key: 'unit_cost',
        permission: 'HIDDEN',
        required: false,
        reason: '角色不在可讀清單',
      },
    ],
    ...over,
  }
}

function mockSandbox(sessions: unknown[], tasks: unknown[], form?: unknown) {
  server.use(
    http.get(`${API}/sandboxes`, () => HttpResponse.json(sessions)),
    http.get(`${API}/sandboxes/${SANDBOX_ID}/tasks`, () =>
      HttpResponse.json(tasks),
    ),
    http.get(`${API}/sandboxes/${SANDBOX_ID}/tasks/tk-1/form`, () =>
      HttpResponse.json(form ?? formView()),
    ),
  )
}

async function render() {
  const w = mount(SimulationMode)
  await flushPromises()
  return w
}

describe('開發模式標示', () => {
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('有進行中的開發模式時顯示不可關閉的標示', async () => {
    // 硬性要求。使用者在開發模式核准了，若以為正式也核准了，
    // 那比不給模擬更糟。
    mockSandbox([session()], [task()])
    const w = await render()

    expect(w.find('[data-testid="sandbox-banner"]').exists()).toBe(true)
    // 沒有關閉按鈕
    expect(w.find('[data-testid="sandbox-banner-close"]').exists()).toBe(false)
  })

  it('沒有開發模式時顯示建立入口而非空白', async () => {
    mockSandbox([], [])
    const w = await render()

    expect(w.find('[data-testid="sandbox-empty"]').exists()).toBe(true)
    expect(w.find('[data-testid="sandbox-create"]').exists()).toBe(true)
    expect(w.find('[data-testid="sandbox-banner"]').exists()).toBe(false)
  })
})

describe('待簽清單', () => {
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('顯示流程走到哪、誰在簽', async () => {
    mockSandbox([session()], [task()])
    const w = await render()

    const row = w.find('[data-testid="sim-task-tk-1"]')
    expect(row.exists()).toBe(true)
    expect(row.text()).toContain('主管簽核')
    expect(row.text()).toContain('張文華')
    // 解析依據要看得到——這是模擬要驗證的東西
    expect(row.text()).toContain('cfo')
  })

  it('每一筆都有「以這個人簽核」的按鈕', async () => {
    // 使用者要的是點選扮演，不是登入五次
    mockSandbox([session()], [task()])
    const w = await render()

    expect(w.find('[data-testid="act-as-tk-1"]').exists()).toBe(true)
  })

  it('沒有待簽時明說流程已走完或尚未啟動', async () => {
    mockSandbox([session()], [])
    const w = await render()

    expect(w.find('[data-testid="sim-no-tasks"]').exists()).toBe(true)
  })

  it('解析不到簽核人時顯示為待補，而非假裝有人', async () => {
    // resolver 解析不到人要明確顯示，不可跳過或用預設人員頂替——
    // 頂替會讓使用者以為設定是對的
    mockSandbox(
      [session()],
      [task({ assignee_user_id: null, assignee_name: null, assignee_role: 'finance_manager' })],
    )
    const w = await render()

    const row = w.find('[data-testid="sim-task-tk-1"]')
    expect(row.text()).toContain('無法解析')
    // 不能給扮演按鈕——沒有人可以扮演
    expect(w.find('[data-testid="act-as-tk-1"]').exists()).toBe(false)
  })
})

describe('模擬簽核', () => {
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('送出時帶上扮演對象與決策', async () => {
    let captured: unknown
    mockSandbox([session()], [task()])
    server.use(
      http.post(`${API}/sandboxes/${SANDBOX_ID}/tasks/tk-1/simulate`, async ({ request }) => {
        captured = await request.json()
        return HttpResponse.json({
          task_id: 'tk-1',
          acting_as: 'cfo-id',
          decision: 'APPROVE',
        })
      }),
    )

    const w = await render()
    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    await w.find('[data-testid="sim-approve"]').trigger('click')
    await flushPromises()

    expect(captured).toMatchObject({
      acting_as: 'cfo-id',
      decision: 'APPROVE',
    })
  })

  it('可以退回並填意見', async () => {
    let captured: { decision?: string; comment?: string } | undefined
    mockSandbox([session()], [task()])
    server.use(
      http.post(`${API}/sandboxes/${SANDBOX_ID}/tasks/tk-1/simulate`, async ({ request }) => {
        captured = (await request.json()) as typeof captured
        return HttpResponse.json({
          task_id: 'tk-1',
          acting_as: 'cfo-id',
          decision: 'REJECT',
        })
      }),
    )

    const w = await render()
    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    await w.find('[data-testid="sim-comment"]').setValue('金額需要再確認')
    await w.find('[data-testid="sim-reject"]').trigger('click')
    await flushPromises()

    expect(captured?.decision).toBe('REJECT')
    expect(captured?.comment).toBe('金額需要再確認')
  })

  it('扮演面板顯示正在扮演誰', async () => {
    // 使用者要知道自己現在是以誰的身分在看、在簽
    mockSandbox([session()], [task()])
    const w = await render()

    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    const panel = w.find('[data-testid="sim-panel"]')
    expect(panel.exists()).toBe(true)
    expect(panel.text()).toContain('張文華')
  })

  it('簽核失敗時顯示後端訊息', async () => {
    mockSandbox([session()], [task()])
    server.use(
      http.post(`${API}/sandboxes/${SANDBOX_ID}/tasks/tk-1/simulate`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '待辦已被處理' },
          { status: 409 },
        ),
      ),
    )

    const w = await render()
    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()
    await w.find('[data-testid="sim-approve"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="sim-error"]').text()).toContain('已被處理')
  })
})

describe('權限過濾畫面', () => {
  // 使用者原話的第三件事：
  //   「系統依據權限設定顯示每個簽核人員的畫面」
  //
  // 少了這塊，模擬只證明流程會動，沒證明那個人簽核時
  // 看得到該看的、看不到不該看的。
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('扮演時顯示這個人看得到的欄位與權限', async () => {
    mockSandbox([session()], [task()])
    const w = await render()

    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="sim-form-view"]').exists()).toBe(true)
    expect(w.find('[data-testid="sim-perm-customer_name"]').text()).toBe('唯讀')
    expect(w.find('[data-testid="sim-perm-approval_comment"]').text()).toBe(
      '可編輯',
    )
  })

  it('看不到的欄位不混在清單裡，另外列出並說明原因', async () => {
    // 「為什麼這個人看不到成本」正是設定對不對的關鍵。
    // 把 HIDDEN 混在可見清單裡等於沒有過濾。
    mockSandbox([session()], [task()])
    const w = await render()

    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    // 不出現在欄位列
    expect(w.find('[data-testid="sim-field-unit_cost"]').exists()).toBe(false)

    const hidden = w.find('[data-testid="sim-hidden-fields"]')
    expect(hidden.exists()).toBe(true)
    expect(hidden.text()).toContain('unit_cost')
    // 原因要說出來，不能只說「看不到」。
    // 這些字串由後端的 resolve_form 產生（固定字串），
    // mock 用的是後端真的會回的值。
    expect(hidden.text()).toContain('角色不在可讀清單')
  })

  it('顯示的是被扮演者的角色，不是測試者的', async () => {
    // 這條是整塊的重點。測試者登入時帶的是 designer；
    // 若畫面顯示 designer，代表後端用錯了角色來源，
    // 模擬出來的畫面與那個人真正會看到的不同。
    mockSandbox([session()], [task()])
    const w = await render()

    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    const roles = w.find('[data-testid="sim-form-roles"]').text()
    expect(roles).toContain('approver')
    expect(roles).toContain('cfo')
    expect(roles).not.toContain('designer')
  })

  it('沒有任何可見欄位時仍然顯示被隱藏的原因', async () => {
    // 全部看不到是合法結果（例如只掛了旁觀角色），
    // 但畫面要說得出來，不能是一片空白讓人以為壞了。
    mockSandbox(
      [session()],
      [task()],
      formView({
        acting_as_roles: ['viewer'],
        fields: [
          {
            key: 'unit_cost',
            permission: 'HIDDEN',
            required: false,
            reason: '角色不在可讀清單',
          },
        ],
      }),
    )
    const w = await render()

    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="sim-hidden-fields"]').text()).toContain(
      '1 個欄位',
    )
  })

  it('載不到權限時明說，不靜默顯示全部欄位', async () => {
    // 靜默顯示全部會讓使用者以為「這個人什麼都看得到」，
    // 比不顯示更糟——那是一個錯誤的結論，不只是缺資訊。
    mockSandbox([session()], [task()])
    server.use(
      http.get(`${API}/sandboxes/${SANDBOX_ID}/tasks/tk-1/form`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '這張單沒有鎖定表單版本' },
          { status: 409 },
        ),
      ),
    )

    const w = await render()
    await w.find('[data-testid="act-as-tk-1"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="sim-form-error"]').text()).toContain(
      '沒有鎖定表單版本',
    )
    expect(w.find('[data-testid="sim-field-customer_name"]').exists()).toBe(
      false,
    )
  })
})

describe('平行分支模擬', () => {
  // 驗收標準（交辦 §5）：
  //   「一個測試者從頭走完含平行分支的報價單流程」
  //
  // 平行分支會同時產生多張待辦。平鋪列出的話使用者會以為
  // 流程分岔成兩條獨立的路，而實際上要兩邊都簽完才會往下走。
  beforeEach(() => {
    push.mockClear()
    login()
  })

  function branch(over: Record<string, unknown> = {}) {
    return {
      parallel_id: 'cosign',
      parallel_label: '財務與法務會簽',
      join_id: 'cosign_join',
      branch_total: 2,
      branch_id: 'finance_approval',
      completion: 'ALL',
      n: null,
      done: 0,
      ...over,
    }
  }

  function cosignTasks(done = 0) {
    return [
      task({
        task_id: 'tk-1',
        node_id: 'finance_approval',
        node_label: '財務簽核',
        assignee_name: '李淑芬',
        branch: branch({ branch_id: 'finance_approval', done }),
      }),
      task({
        task_id: 'tk-2',
        node_id: 'legal_approval',
        node_label: '法務簽核',
        assignee_name: '張文華',
        branch: branch({ branch_id: 'legal_approval', done }),
      }),
    ]
  }

  it('同一個會簽的多張待辦歸在一組，只出現一個標頭', async () => {
    mockSandbox([session()], cosignTasks())
    const w = await render()

    // 兩張待辦都在
    expect(w.find('[data-testid="sim-task-tk-1"]').exists()).toBe(true)
    expect(w.find('[data-testid="sim-task-tk-2"]').exists()).toBe(true)
    // 但標頭只有一個——兩個標頭代表被當成兩組獨立的事
    expect(w.findAll('[data-testid="sim-branch-cosign"]')).toHaveLength(1)
  })

  it('標頭說明還在等幾條分支', async () => {
    // 使用者要看的是「還在等誰」，不是「有兩張待辦」
    mockSandbox([session()], cosignTasks(1))
    const w = await render()

    const progress = w.find('[data-testid="sim-branch-progress-cosign"]').text()
    expect(progress).toContain('2 條分支')
    expect(progress).toContain('已完成')
    expect(progress).toContain('1')
  })

  it('說明通過條件，ALL 與 ANY 的意思完全不同', async () => {
    // ALL 是「兩邊都要過」，ANY 是「一邊過就行」。
    // 不說清楚，使用者無從判斷會簽設定對不對——
    // 而那正是模擬要驗證的東西。
    mockSandbox([session()], cosignTasks())
    const w = await render()
    expect(w.find('[data-testid="sim-branch-cosign"]').text()).toContain(
      '所有分支都要通過',
    )

    mockSandbox(
      [session()],
      cosignTasks().map((t) => ({
        ...t,
        branch: { ...(t.branch as object), completion: 'ANY' },
      })),
    )
    const w2 = await render()
    expect(w2.find('[data-testid="sim-branch-cosign"]').text()).toContain(
      '任一分支通過即可',
    )
  })

  it('N_OF_M 顯示需要幾條，而不是總共幾條', async () => {
    // 「5 人中 3 人同意」顯示成 5 的話，使用者會多簽兩關
    // 才發現流程早就過了
    mockSandbox(
      [session()],
      cosignTasks().map((t) => ({
        ...t,
        branch: { ...(t.branch as object), completion: 'N_OF_M', n: 1 },
      })),
    )
    const w = await render()
    expect(w.find('[data-testid="sim-branch-cosign"]').text()).toContain(
      '1 條分支通過即可',
    )
  })

  it('不在平行結構裡的待辦不顯示會簽標頭', async () => {
    // 多數節點都不是會簽。硬掛一個標頭會讓使用者
    // 以為在等其他不存在的分支
    mockSandbox([session()], [task()])
    const w = await render()

    expect(w.find('[data-testid="sim-task-tk-1"]').exists()).toBe(true)
    expect(w.findAll('[data-testid^="sim-branch-"]')).toHaveLength(0)
  })

  it('不同單據的同名會簽不會被併成一組', async () => {
    // 只用 parallel_id 分群會把兩張單的「財務與法務會簽」
    // 混在一起，進度就變成 4 條分支
    mockSandbox(
      [session()],
      [
        task({ task_id: 'tk-1', instance_id: 'in-1', branch: branch() }),
        task({ task_id: 'tk-2', instance_id: 'in-2', branch: branch() }),
      ],
    )
    const w = await render()

    expect(w.findAll('[data-testid="sim-branch-cosign"]')).toHaveLength(2)
  })
})

describe('時間快轉', () => {
  // 含 P2D、P7D 的流程在模擬時原本要等兩天、七天才看得到逾時行為，
  // 等於無法驗證。
  beforeEach(() => {
    push.mockClear()
    login()
  })

  it('節點有逾時才給快轉按鈕', async () => {
    // 沒有等待可以快轉時給了按鈕，按下去什麼都不會發生，
    // 比不給更讓人困惑
    mockSandbox([session()], [task()])
    const w = await render()
    expect(w.find('[data-testid="skip-time-tk-1"]').exists()).toBe(false)

    mockSandbox(
      [session()],
      [task({ timeout: { after: 'P2D', policy: 'AUTO_REJECT' } })],
    )
    const w2 = await render()
    expect(w2.find('[data-testid="skip-time-tk-1"]').exists()).toBe(true)
  })

  it('按鈕寫明快轉多久、以及之後會發生什麼', async () => {
    // **這條是重點。** 快轉不假造決策，跑的是真正的逾時策略。
    // AUTO_REJECT 的節點快轉後是**退回**——使用者以為是通過的話，
    // 會帶著錯誤的結論上線。
    mockSandbox(
      [session()],
      [task({ timeout: { after: 'P2D', policy: 'AUTO_REJECT' } })],
    )
    const w = await render()

    const btn = w.find('[data-testid="skip-time-tk-1"]')
    expect(btn.text()).toContain('2 天')
    expect(btn.text()).toContain('視為退回')
  })

  it('快轉後說明套用了哪個策略', async () => {
    mockSandbox(
      [session()],
      [task({ timeout: { after: 'P2D', policy: 'ESCALATE' } })],
    )
    server.use(
      http.post(`${API}/sandboxes/${SANDBOX_ID}/tasks/tk-1/skip-time`, () =>
        HttpResponse.json({
          task_id: 'tk-1',
          node_id: 'manager_approval',
          policy: 'ESCALATE',
          after: 'P2D',
        }),
      ),
    )

    const w = await render()
    await w.find('[data-testid="skip-time-tk-1"]').trigger('click')
    await flushPromises()

    const msg = w.find('[data-testid="sim-skip-message"]').text()
    expect(msg).toContain('2 天')
    expect(msg).toContain('加簽給上級')
  })

  it('快轉失敗顯示後端訊息', async () => {
    // 後端對沒設逾時的節點回 409 並說明原因，要照實呈現
    mockSandbox(
      [session()],
      [task({ timeout: { after: 'P2D', policy: 'WAIT' } })],
    )
    server.use(
      http.post(`${API}/sandboxes/${SANDBOX_ID}/tasks/tk-1/skip-time`, () =>
        HttpResponse.json(
          { code: 'CONFLICT', message: '節點「主管簽核」沒有設定逾時' },
          { status: 409 },
        ),
      ),
    )

    const w = await render()
    await w.find('[data-testid="skip-time-tk-1"]').trigger('click')
    await flushPromises()

    expect(w.find('[data-testid="sim-skip-error"]').text()).toContain(
      '沒有設定逾時',
    )
  })

  it('PT6H 這類時分也講成人話', async () => {
    mockSandbox(
      [session()],
      [task({ timeout: { after: 'PT6H', policy: 'WAIT' } })],
    )
    const w = await render()
    expect(w.find('[data-testid="skip-time-tk-1"]').text()).toContain('6 小時')
  })
})
