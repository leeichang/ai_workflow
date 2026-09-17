/**
 * 流程圖佈局測試
 *
 * 佈局是設計器的核心。演算法錯誤會讓流程圖難以閱讀，
 * 但不會有任何執行期錯誤，只能靠測試守住。
 */

import { describe, expect, it } from 'vitest'
import {
  computeLayout,
  insertionPoints,
  NODE_HEIGHT,
  NODE_WIDTH,
  VERTICAL_GAP,
} from '@/workflow/layout'
import type { WorkflowContent } from '@/api/workflows'

function linear(): WorkflowContent {
  return {
    workflow_key: 'test',
    version: 1,
    business_object: 'quotation',
    trigger: { type: 'form_submit' },
    nodes: [
      { id: 'start', type: 'trigger', label: '開始' },
      { id: 'approve', type: 'human_approval', label: '簽核', participant: 'internal' },
      { id: 'end', type: 'end', label: '結束' },
    ],
    edges: [
      ['start', 'approve'],
      ['approve', 'end'],
    ],
  }
}

function branching(): WorkflowContent {
  return {
    workflow_key: 'test',
    version: 1,
    business_object: 'quotation',
    trigger: { type: 'form_submit' },
    nodes: [
      { id: 'start', type: 'trigger' },
      { id: 'gate', type: 'condition', expression: 'a > 1' },
      { id: 'yes_path', type: 'human_approval', participant: 'internal' },
      { id: 'merge', type: 'action', action: 'x.y' },
      { id: 'end', type: 'end' },
    ],
    edges: [
      ['start', 'gate'],
      ['gate', 'yes_path', { when: true }],
      ['gate', 'merge', { when: false }],
      ['yes_path', 'merge'],
      ['merge', 'end'],
    ],
  }
}

function withReturn(): WorkflowContent {
  const c = linear()
  c.nodes.push({
    id: 'revise',
    type: 'human_task',
    label: '修改',
    participant: 'internal',
  })
  c.nodes.find((n) => n.id === 'approve')!.on_reject = {
    action: 'goto',
    node: 'revise',
  }
  c.edges.push(['revise', 'approve'])
  return c
}

describe('基本佈局', () => {
  it('線性流程由上往下排列', () => {
    const l = computeLayout(linear())

    const y = (id: string) => l.nodes.find((n) => n.node.id === id)!.y
    expect(y('start')).toBeLessThan(y('approve'))
    expect(y('approve')).toBeLessThan(y('end'))
  })

  it('線性流程水平對齊', () => {
    const l = computeLayout(linear())
    const xs = l.nodes.map((n) => n.x)
    expect(new Set(xs).size, '單一主幹的節點應在同一垂直線上').toBe(1)
  })

  it('層級間距一致', () => {
    const l = computeLayout(linear())
    const sorted = [...l.nodes].sort((a, b) => a.y - b.y)
    const gaps = sorted.slice(1).map((n, i) => n.y - sorted[i].y)
    expect(new Set(gaps).size).toBe(1)
    expect(gaps[0]).toBe(NODE_HEIGHT + VERTICAL_GAP)
  })

  it('空流程不崩潰', () => {
    const empty: WorkflowContent = {
      workflow_key: 'e',
      version: 1,
      business_object: 'x',
      trigger: { type: 'manual' },
      nodes: [],
      edges: [],
    }
    const l = computeLayout(empty)
    expect(l.nodes).toHaveLength(0)
    expect(l.width).toBe(0)
  })
})

describe('分支', () => {
  it('分支節點水平並排', () => {
    const l = computeLayout(branching())
    const yes = l.nodes.find((n) => n.node.id === 'yes_path')!
    const gate = l.nodes.find((n) => n.node.id === 'gate')!

    expect(yes.y).toBeGreaterThan(gate.y)
  })

  it('匯合節點在兩分支之下', () => {
    const l = computeLayout(branching())
    const merge = l.nodes.find((n) => n.node.id === 'merge')!
    const yes = l.nodes.find((n) => n.node.id === 'yes_path')!

    // merge 同時是 yes_path 與 gate 的後繼。用最長路徑指派層級，
    // 才不會讓 yes_path 的邊向上回繞。
    expect(merge.y).toBeGreaterThan(yes.y)
  })

  it('condition 的兩條邊帶 when 標記', () => {
    const l = computeLayout(branching())
    const fromGate = l.edges.filter((e) => e.from === 'gate')

    expect(fromGate).toHaveLength(2)
    expect(fromGate.some((e) => e.when === true)).toBe(true)
    expect(fromGate.some((e) => e.when === false)).toBe(true)
  })
})

describe('退回路徑', () => {
  it('goto 產生 isReturn 的邊', () => {
    const l = computeLayout(withReturn())
    const ret = l.edges.filter((e) => e.isReturn)

    expect(ret).toHaveLength(1)
    expect(ret[0].from).toBe('approve')
    expect(ret[0].to).toBe('revise')
    expect(ret[0].label).toBe('退回')
  })

  it('只靠 goto 進入的節點仍會被定位', () => {
    // revise 沒有任何 edges 指向它。若佈局只看 edges，
    // 這個節點會消失在畫布上。
    const l = computeLayout(withReturn())
    const revise = l.nodes.find((n) => n.node.id === 'revise')

    expect(revise, 'revise 必須出現在畫布').toBeDefined()
    expect(revise!.y).toBeGreaterThan(0)
  })

  it('退回路徑走左側外緣', () => {
    const l = computeLayout(withReturn())
    const ret = l.edges.find((e) => e.isReturn)!

    // 路徑應包含往左的座標，小於所有節點的 x
    const minNodeX = Math.min(...l.nodes.map((n) => n.x))
    const xs = [...ret.path.matchAll(/[ML] ([\d.]+)/g)].map((m) => Number(m[1]))
    expect(Math.min(...xs)).toBeLessThan(minNodeX)
  })

  it('on_failure 也產生退回路徑', () => {
    const c = linear()
    c.nodes.push({ id: 'retry', type: 'human_task', participant: 'internal' })
    c.nodes.find((n) => n.id === 'approve')!.on_failure = {
      action: 'goto',
      node: 'retry',
    }

    const l = computeLayout(c)
    const ret = l.edges.filter((e) => e.isReturn)
    expect(ret).toHaveLength(1)
    expect(ret[0].label).toBe('失敗')
  })
})

describe('尺寸與插入點', () => {
  it('畫布尺寸涵蓋所有節點', () => {
    const l = computeLayout(branching())
    const maxX = Math.max(...l.nodes.map((n) => n.x + NODE_WIDTH))
    const maxY = Math.max(...l.nodes.map((n) => n.y + NODE_HEIGHT))

    expect(l.width).toBeGreaterThanOrEqual(maxX)
    expect(l.height).toBeGreaterThanOrEqual(maxY)
  })

  it('左側預留退回路徑空間', () => {
    const l = computeLayout(withReturn())
    const minX = Math.min(...l.nodes.map((n) => n.x))
    expect(minX, '節點不該貼齊左緣，需留空間給退回路徑').toBeGreaterThan(0)
  })

  it('插入點在每條前進邊的中段', () => {
    const l = computeLayout(linear())
    const points = insertionPoints(l)

    expect(points).toHaveLength(2)
    expect(points[0].from).toBe('start')
    expect(points[0].to).toBe('approve')
  })

  it('退回邊不產生插入點', () => {
    const l = computeLayout(withReturn())
    const points = insertionPoints(l)

    expect(
      points.every((p) => !(p.from === 'approve' && p.to === 'revise')),
      '退回路徑不該可插入節點',
    ).toBe(true)
  })
})

describe('真實 fixture', () => {
  it('報價單流程佈局合理', async () => {
    const dsl = (await import('../../../schemas/fixtures/quotation_approval_v1.json'))
      .default as unknown as WorkflowContent

    const l = computeLayout(dsl)

    expect(l.nodes).toHaveLength(dsl.nodes.length)

    // trigger 在最上方
    const start = l.nodes.find((n) => n.node.type === 'trigger')!
    expect(start.y).toBe(0)

    // end 在最下方
    const end = l.nodes.find((n) => n.node.type === 'end')!
    const maxY = Math.max(...l.nodes.map((n) => n.y))
    expect(end.y).toBe(maxY)

    // 退回路徑：三個節點的 on_reject 都指向 revise
    const returns = l.edges.filter((e) => e.isReturn)
    expect(returns.length).toBeGreaterThanOrEqual(3)
    expect(returns.every((e) => e.to === 'revise')).toBe(true)
  })
})
