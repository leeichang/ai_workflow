/**
 * 並行分支的設計器操作測試
 *
 * parallel 與 join 必須成對存在——單獨插入一個 parallel 會立刻
 * 觸發 WF-E009／WF-E010。因此設計器提供的是「插入一組並行」，
 * 一次建立 parallel + N 個分支 + join。
 *
 * 分支的增刪也要維護兩端的邊，漏掉任一端圖就斷了。
 */

import { describe, expect, it } from 'vitest'
import { useWorkflowDesigner } from '@/workflow/useWorkflowDesigner'
import type { WorkflowContent } from '@/api/workflows'

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

function designer(c: WorkflowContent = base()) {
  const d = useWorkflowDesigner('test_flow')
  d.content.value = c
  return d
}

/** 取出圖中所有 parallel 節點 */
function parallels(d: ReturnType<typeof designer>) {
  return d.content.value!.nodes.filter((n) => n.type === 'parallel')
}

function joins(d: ReturnType<typeof designer>) {
  return d.content.value!.nodes.filter((n) => n.type === 'join')
}

/** 某節點的所有出邊目標 */
function successors(d: ReturnType<typeof designer>, id: string): string[] {
  return d.content.value!.edges.filter((e) => e[0] === id).map((e) => e[1])
}

function predecessors(d: ReturnType<typeof designer>, id: string): string[] {
  return d.content.value!.edges.filter((e) => e[1] === id).map((e) => e[0])
}

describe('插入並行組', () => {
  it('一次建立 parallel、分支與 join', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' }, 2)

    expect(parallels(d)).toHaveLength(1)
    expect(joins(d)).toHaveLength(1)
    // 兩個分支節點
    expect(d.content.value!.nodes.filter((n) => n.type === 'human_approval')).toHaveLength(
      3, // 原本的 approve + 兩個分支
    )
  })

  it('預設兩個分支——一個分支的 parallel 無意義', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' })

    const pid = parallels(d)[0].id
    expect(successors(d, pid)).toHaveLength(2)
  })

  it('可指定分支數', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' }, 4)

    const pid = parallels(d)[0].id
    expect(successors(d, pid)).toHaveLength(4)
  })

  it('每個分支都連到同一個 join', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' }, 3)

    const pid = parallels(d)[0].id
    const jid = joins(d)[0].id
    const branches = successors(d, pid)

    for (const b of branches) {
      expect(successors(d, b)).toEqual([jid])
    }
  })

  it('接在原本的邊上：start → parallel，join → approve', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' })

    const pid = parallels(d)[0].id
    const jid = joins(d)[0].id

    expect(predecessors(d, pid)).toEqual(['start'])
    expect(successors(d, jid)).toEqual(['approve'])
    // 原本的 start → approve 要移除，否則會有一條繞過並行的捷徑
    expect(
      d.content.value!.edges.some((e) => e[0] === 'start' && e[1] === 'approve'),
    ).toBe(false)
  })

  it('join 預設全部完成且全部核准', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' })

    const join = joins(d)[0]
    expect(join.join?.completion).toBe('ALL')
    expect(join.join?.result).toBe('ALL_SUCCESS')
  })

  it('保留原邊的 meta，分支條件不會消失', () => {
    const c = base()
    c.nodes.push({ id: 'gate', type: 'condition', label: '判斷', expression: 'a > 1' })
    c.edges = [
      ['start', 'gate'],
      ['gate', 'approve', { when: true }],
      ['gate', 'done', { when: false }],
      ['approve', 'done'],
    ]
    const d = designer(c)
    d.insertParallel({ from: 'gate', to: 'approve' })

    const pid = parallels(d)[0].id
    const edge = d.content.value!.edges.find((e) => e[0] === 'gate' && e[1] === pid)
    expect(edge?.[2]?.when).toBe(true)
  })

  it('選取新建立的 parallel', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' })
    expect(d.selectedId.value).toBe(parallels(d)[0].id)
  })

  it('可以 undo', () => {
    const d = designer()
    const before = d.content.value!.nodes.length
    d.insertParallel({ from: 'start', to: 'approve' })
    d.undo()
    expect(d.content.value!.nodes).toHaveLength(before)
  })
})

describe('增減分支', () => {
  function withParallel() {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' }, 2)
    return { d, pid: parallels(d)[0].id, jid: joins(d)[0].id }
  }

  it('新增分支同時接上 parallel 與 join', () => {
    const { d, pid, jid } = withParallel()
    d.addBranch(pid)

    const branches = successors(d, pid)
    expect(branches).toHaveLength(3)
    // 新分支兩端都要接好，只接一端圖就斷了
    for (const b of branches) {
      expect(successors(d, b)).toEqual([jid])
    }
  })

  it('刪除分支移除節點與兩端的邊', () => {
    const { d, pid } = withParallel()
    d.addBranch(pid)
    const target = successors(d, pid)[0]

    d.removeBranch(pid, target)

    expect(successors(d, pid)).toHaveLength(2)
    expect(d.content.value!.nodes.some((n) => n.id === target)).toBe(false)
    expect(
      d.content.value!.edges.some((e) => e[0] === target || e[1] === target),
    ).toBe(false)
  })

  it('剩兩個分支時不可再刪——會讓 parallel 變成無意義', () => {
    const { d, pid } = withParallel()
    const target = successors(d, pid)[0]

    d.removeBranch(pid, target)

    expect(successors(d, pid)).toHaveLength(2)
    expect(d.error.value).toContain('分支')
  })
})

describe('刪除並行組', () => {
  it('刪除 parallel 會一併移除分支與 join', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' }, 3)
    const pid = parallels(d)[0].id

    d.removeNode(pid)

    expect(parallels(d)).toHaveLength(0)
    expect(joins(d)).toHaveLength(0)
    // 分支節點也要清掉，否則變成孤立節點
    expect(d.content.value!.nodes.filter((n) => n.type === 'human_approval')).toHaveLength(
      1,
    )
  })

  it('刪除後前後重新接起來', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' })
    d.removeNode(parallels(d)[0].id)

    expect(successors(d, 'start')).toEqual(['approve'])
  })

  it('刪除 join 等同刪除整組', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' })
    d.removeNode(joins(d)[0].id)

    expect(parallels(d)).toHaveLength(0)
    expect(joins(d)).toHaveLength(0)
    expect(successors(d, 'start')).toEqual(['approve'])
  })
})

describe('join 策略', () => {
  it('可改為任一完成', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' })
    const jid = joins(d)[0].id

    d.updateNode(jid, { join: { completion: 'ANY', result: 'ALL_SUCCESS' } })

    expect(joins(d)[0].join?.completion).toBe('ANY')
  })

  it('N_OF_M 要帶 n', () => {
    const d = designer()
    d.insertParallel({ from: 'start', to: 'approve' }, 3)
    const jid = joins(d)[0].id

    d.updateNode(jid, { join: { completion: 'N_OF_M', n: 2, result: 'ALL_SUCCESS' } })

    const join = joins(d)[0].join
    expect(join?.completion).toBe('N_OF_M')
    expect(join?.n).toBe(2)
  })
})
