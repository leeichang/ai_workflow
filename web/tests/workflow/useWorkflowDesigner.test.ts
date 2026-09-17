/**
 * 流程設計器狀態測試
 *
 * 重點在節點操作對 edges 的維護。這類錯誤不會丟例外，
 * 只會讓圖悄悄斷裂，要到發布被驗證擋下才發現。
 */

import { describe, expect, it } from 'vitest'
import { useWorkflowDesigner } from '@/workflow/useWorkflowDesigner'
import type { WorkflowContent, WorkflowNode } from '@/api/workflows'

function base(): WorkflowContent {
  return {
    workflow_key: 'test_flow',
    version: 1,
    business_object: 'quotation',
    trigger: { type: 'form_submit' },
    nodes: [
      { id: 'start', type: 'trigger', label: '開始' },
      { id: 'approve', type: 'human_approval', label: '簽核', participant: 'internal' },
      { id: 'done', type: 'end', label: '結束', result: 'completed' },
    ],
    edges: [
      ['start', 'approve'],
      ['approve', 'done'],
    ],
  }
}

function newNode(id: string, type: WorkflowNode['type'] = 'notification'): WorkflowNode {
  return { id, type, label: id }
}

/** 建立已載入內容的設計器，跳過 API */
function designer(c: WorkflowContent = base()) {
  const d = useWorkflowDesigner('test_flow')
  d.content.value = c
  return d
}

describe('插入節點', () => {
  it('在邊上插入會接成 a → new → b', () => {
    const d = designer()
    d.insertNode(newNode('notify'), { from: 'start', to: 'approve' })

    const edges = d.content.value!.edges
    expect(edges).toContainEqual(['start', 'notify'])
    expect(edges).toContainEqual(['notify', 'approve'])
    expect(edges.some((e) => e[0] === 'start' && e[1] === 'approve')).toBe(false)
  })

  it('保留原邊的 when 在前半段', () => {
    // condition 的分支條件屬於「離開 condition 的那條邊」。
    // 若插入後 when 跑到後半段，分支就失效了。
    const c = base()
    c.nodes.push({ id: 'gate', type: 'condition', expression: 'amount > 100' })
    c.edges = [
      ['start', 'gate'],
      ['gate', 'approve', { when: true }],
      ['approve', 'done'],
    ]

    const d = designer(c)
    d.insertNode(newNode('notify'), { from: 'gate', to: 'approve' })

    const first = d.content.value!.edges.find((e) => e[0] === 'gate' && e[1] === 'notify')
    const second = d.content.value!.edges.find((e) => e[0] === 'notify' && e[1] === 'approve')

    expect(first?.[2]).toEqual({ when: true })
    expect(second?.[2]).toBeUndefined()
  })

  it('id 重複時自動加序號', () => {
    const d = designer()
    d.insertNode(newNode('approve'), { from: 'start', to: 'approve' })

    const ids = d.content.value!.nodes.map((n) => n.id)
    expect(ids).toContain('approve_2')
    expect(new Set(ids).size, '節點 id 必須唯一').toBe(ids.length)
  })

  it('插入後選取新節點', () => {
    const d = designer()
    d.insertNode(newNode('notify'), { from: 'start', to: 'approve' })
    expect(d.selectedId.value).toBe('notify')
  })
})

describe('附加節點', () => {
  it('建立單向邊', () => {
    const d = designer()
    d.appendNode(newNode('extra'), 'approve', { when: false })

    expect(d.content.value!.edges).toContainEqual(['approve', 'extra', { when: false }])
  })
})

describe('刪除節點', () => {
  it('接合前後節點', () => {
    const d = designer()
    d.removeNode('approve')

    expect(d.content.value!.nodes.map((n) => n.id)).toEqual(['start', 'done'])
    expect(d.content.value!.edges).toContainEqual(['start', 'done'])
  })

  it('不留下指向已刪節點的邊', () => {
    const d = designer()
    d.removeNode('approve')

    const ids = new Set(d.content.value!.nodes.map((n) => n.id))
    for (const e of d.content.value!.edges) {
      expect(ids.has(e[0]), `邊的來源 ${e[0]} 應存在`).toBe(true)
      expect(ids.has(e[1]), `邊的目標 ${e[1]} 應存在`).toBe(true)
    }
  })

  it('清掉指向已刪節點的 goto', () => {
    // 留著懸空的 goto 會讓發布被 WF-E006 擋下，
    // 而使用者只是刪了一個節點，不會聯想到錯誤來源。
    const c = base()
    c.nodes.push({ id: 'revise', type: 'human_task', participant: 'internal' })
    c.nodes.find((n) => n.id === 'approve')!.on_reject = { action: 'goto', node: 'revise' }
    c.edges.push(['revise', 'approve'])

    const d = designer(c)
    d.removeNode('revise')

    const approve = d.content.value!.nodes.find((n) => n.id === 'approve')!
    expect(approve.on_reject?.action).toBe('end')
    expect(approve.on_reject?.node).toBeUndefined()
  })

  it('分支匯合時不產生重複邊', () => {
    const c = base()
    c.nodes.push({ id: 'b', type: 'action', action: 'x' })
    c.edges = [
      ['start', 'approve'],
      ['start', 'b'],
      ['approve', 'done'],
      ['b', 'done'],
    ]

    const d = designer(c)
    d.removeNode('approve')

    const dup = d.content.value!.edges.filter((e) => e[0] === 'start' && e[1] === 'done')
    expect(dup).toHaveLength(1)
  })

  it('不允許刪除 trigger', () => {
    const d = designer()
    d.removeNode('start')

    expect(d.content.value!.nodes.some((n) => n.id === 'start')).toBe(true)
    expect(d.error.value).toContain('開始節點')
  })
})

describe('退回目標', () => {
  it('設定 goto', () => {
    const d = designer()
    d.setRejectTarget('approve', 'start')

    const n = d.content.value!.nodes.find((x) => x.id === 'approve')!
    expect(n.on_reject).toEqual({ action: 'goto', node: 'start' })
  })

  it('設 null 改為直接結束', () => {
    const d = designer()
    d.setRejectTarget('approve', null)

    const n = d.content.value!.nodes.find((x) => x.id === 'approve')!
    expect(n.on_reject).toEqual({ action: 'end', result: 'rejected' })
  })
})

describe('復原與重做', () => {
  it('復原還原插入前的狀態', () => {
    const d = designer()
    const before = d.content.value!.nodes.length

    d.insertNode(newNode('notify'), { from: 'start', to: 'approve' })
    expect(d.content.value!.nodes).toHaveLength(before + 1)

    d.undo()
    expect(d.content.value!.nodes).toHaveLength(before)
    expect(d.canRedo.value).toBe(true)
  })

  it('重做再套用一次', () => {
    const d = designer()
    d.insertNode(newNode('notify'), { from: 'start', to: 'approve' })
    d.undo()
    d.redo()

    expect(d.content.value!.nodes.some((n) => n.id === 'notify')).toBe(true)
  })

  it('修改後清除舊的驗證結果', () => {
    // 圖變了，上次的錯誤位置就不一定還對得上節點，
    // 留著會在畫布上標錯節點。
    const d = designer()
    d.graphErrors.value = [{ code: 'WF-E002', node_id: 'approve', message: '無法到達' }]

    d.updateNode('approve', { label: '改名' })
    expect(d.graphErrors.value).toHaveLength(0)
  })
})

describe('衍生狀態', () => {
  it('errorNodeIds 只收有 node_id 的錯誤', () => {
    const d = designer()
    d.graphErrors.value = [
      { code: 'WF-E002', node_id: 'approve', message: 'x' },
      { code: 'WF-E001', node_id: null, message: '缺少 trigger' },
    ]

    expect([...d.errorNodeIds.value]).toEqual(['approve'])
  })

  it('layout 隨 content 更新', () => {
    const d = designer()
    expect(d.layout.value!.nodes).toHaveLength(3)

    d.insertNode(newNode('notify'), { from: 'start', to: 'approve' })
    expect(d.layout.value!.nodes).toHaveLength(4)
  })
})
