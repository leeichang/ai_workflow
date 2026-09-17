/**
 * 邊標記位置測試
 *
 * 這個規則被實機測試抓到兩次：
 *   取起點 → condition 的「是」「否」疊在一起
 *   改取終點 → 四條退回邊的標記全疊在 revise 上
 * 兩種邊的方向相反，取的端點也必須相反。
 *
 * 標記位置不影響任何邏輯，錯了只會在畫面上少一個字，
 * 沒有測試就沒有人會發現。
 */

import { describe, expect, it } from 'vitest'
import { computeLayout } from '@/workflow/layout'
import { edgeLabelPoint } from '@/workflow/edgeLabel'
import type { WorkflowContent } from '@/api/workflows'

/** 一個 condition 分岔、多個節點退回同一處的流程 */
function content(): WorkflowContent {
  return {
    workflow_key: 'test',
    version: 1,
    business_object: 'quotation',
    trigger: { type: 'form_submit' },
    nodes: [
      { id: 'start', type: 'trigger' },
      { id: 'revise', type: 'human_task', participant: 'internal' },
      { id: 'gate', type: 'condition', expression: 'a > 1' },
      {
        id: 'fin',
        type: 'human_approval',
        participant: 'internal',
        on_reject: { action: 'goto', node: 'revise' },
      },
      {
        id: 'pub',
        type: 'action',
        action: 'x.y',
        on_reject: { action: 'goto', node: 'revise' },
      },
      { id: 'end', type: 'end' },
    ],
    edges: [
      ['start', 'gate'],
      ['gate', 'fin', { when: true }],
      ['gate', 'pub', { when: false }],
      ['fin', 'pub'],
      ['pub', 'end'],
      ['revise', 'gate'],
    ],
  }
}

describe('分支標記', () => {
  it('condition 的兩條邊標記不重疊', () => {
    const l = computeLayout(content())
    const branches = l.edges.filter((e) => e.from === 'gate')

    expect(branches).toHaveLength(2)
    const [a, b] = branches.map((e) => edgeLabelPoint(e.path, e.isReturn))

    expect(a.x !== b.x || a.y !== b.y, '是／否標記必須分得開').toBe(true)
  })

  it('多條退回邊的標記不重疊', () => {
    const l = computeLayout(content())
    const returns = l.edges.filter((e) => e.isReturn)

    expect(returns.length).toBeGreaterThanOrEqual(2)
    const points = returns.map((e) => edgeLabelPoint(e.path, e.isReturn))
    const unique = new Set(points.map((p) => `${p.x},${p.y}`))

    expect(unique.size, '每條退回邊的標記應在各自的起點').toBe(points.length)
  })

  it('前進邊取終點，退回邊取起點', () => {
    const l = computeLayout(content())

    const forward = l.edges.find((e) => e.from === 'start')!
    const fwdEnd = lastPoint(forward.path)
    expect(edgeLabelPoint(forward.path, false)).toMatchObject({
      x: fwdEnd.x,
      y: fwdEnd.y,
    })

    const ret = l.edges.find((e) => e.isReturn)!
    const retStart = firstPoint(ret.path)
    expect(edgeLabelPoint(ret.path, true)).toMatchObject({
      x: retStart.x,
      y: retStart.y,
    })
  })

  it('路徑無法解析時不回傳 NaN', () => {
    // NaN 會變成 left: NaNpx，整個標記飛到畫面左上角
    const p = edgeLabelPoint('', false)
    expect(Number.isFinite(p.x)).toBe(true)
    expect(Number.isFinite(p.y)).toBe(true)
  })
})

function firstPoint(path: string) {
  const m = [...path.matchAll(/[ML] ([\d.]+) ([\d.]+)/g)][0]
  return { x: Number(m[1]), y: Number(m[2]) }
}

function lastPoint(path: string) {
  const all = [...path.matchAll(/[ML] ([\d.]+) ([\d.]+)/g)]
  const m = all[all.length - 1]
  return { x: Number(m[1]), y: Number(m[2]) }
}
