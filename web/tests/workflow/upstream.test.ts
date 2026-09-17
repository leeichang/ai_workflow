/**
 * 上游查詢測試
 *
 * 這裡藏著整個流程設計最容易搞錯的一點：
 * 上游判斷只能沿 edges，不能含 goto。測試把它釘住。
 */

import { describe, expect, it } from 'vitest'
import { rejectTargetsFor, upstreamOf } from '@/workflow/upstream'
import type { WorkflowContent } from '@/api/workflows'

function chain(): WorkflowContent {
  return {
    workflow_key: 'test',
    version: 1,
    business_object: 'quotation',
    trigger: { type: 'form_submit' },
    nodes: [
      { id: 'start', type: 'trigger', label: '開始' },
      { id: 'revise', type: 'human_task', label: '修改', participant: 'internal' },
      { id: 'mgr', type: 'human_approval', label: '主管簽核', participant: 'internal' },
      { id: 'fin', type: 'human_approval', label: '財務簽核', participant: 'internal' },
      { id: 'done', type: 'end', label: '結束' },
    ],
    edges: [
      ['start', 'mgr'],
      ['mgr', 'fin'],
      ['fin', 'done'],
      ['revise', 'mgr'],
    ],
  }
}

describe('upstreamOf', () => {
  it('沿 edges 回溯所有祖先', () => {
    const u = upstreamOf(chain(), 'fin')
    expect([...u].sort()).toEqual(['mgr', 'revise', 'start'])
  })

  it('不含自己', () => {
    expect(upstreamOf(chain(), 'mgr').has('mgr')).toBe(false)
  })

  it('起點無上游', () => {
    expect(upstreamOf(chain(), 'start').size).toBe(0)
  })

  it('goto 不算上游關係', () => {
    // fin 的 on_reject 指向 revise。若把 goto 當成邊，
    // revise 會被誤判為 fin 的下游，導致 fin 消失在 revise 的
    // 可退回清單中；實際上後端允許 revise 退回到 fin 之前的節點。
    const c = chain()
    c.nodes.find((n) => n.id === 'fin')!.on_reject = { action: 'goto', node: 'revise' }

    const u = upstreamOf(c, 'revise')
    expect(u.has('fin'), 'goto 不該讓 fin 變成 revise 的上游').toBe(false)
  })

  it('分支匯合時兩條路都算上游', () => {
    const c = chain()
    c.nodes.push({ id: 'alt', type: 'action', action: 'x' })
    c.edges.push(['start', 'alt'], ['alt', 'fin'])

    const u = upstreamOf(c, 'fin')
    expect(u.has('alt')).toBe(true)
    expect(u.has('mgr')).toBe(true)
  })
})

describe('rejectTargetsFor', () => {
  it('只列出上游的可退回節點', () => {
    const t = rejectTargetsFor(chain(), 'fin')
    expect(t.map((x) => x.id)).toEqual(['revise', 'mgr'])
  })

  it('排除 trigger', () => {
    // 退回到開始節點等於重跑整個流程，語意上是撤回不是退回
    const t = rejectTargetsFor(chain(), 'fin')
    expect(t.some((x) => x.id === 'start')).toBe(false)
  })

  it('排除 end', () => {
    const c = chain()
    c.edges.push(['done', 'fin'])
    const t = rejectTargetsFor(c, 'fin')
    expect(t.some((x) => x.id === 'done')).toBe(false)
  })

  it('顯示節點名稱，無名稱時退回用 id', () => {
    const c = chain()
    delete c.nodes.find((n) => n.id === 'mgr')!.label

    const t = rejectTargetsFor(c, 'fin')
    expect(t.find((x) => x.id === 'mgr')!.label).toBe('mgr')
    expect(t.find((x) => x.id === 'revise')!.label).toBe('修改')
  })

  it('依畫布順序排列', () => {
    const t = rejectTargetsFor(chain(), 'fin')
    // nodes 陣列中 revise 在 mgr 之前
    expect(t[0].id).toBe('revise')
  })

  it('沒有選取節點時回空陣列', () => {
    expect(rejectTargetsFor(chain(), null)).toEqual([])
    expect(rejectTargetsFor(null, 'fin')).toEqual([])
  })
})
