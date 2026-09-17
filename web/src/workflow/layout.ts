/**
 * 流程圖佈局
 *
 * 依決議 D-03：固定由上往下自動排版，不自由拖曳。
 * 使用者插入節點，位置由演算法決定。
 *
 * 為何不用 dagre：
 *   我們的圖是「主幹加分支」的受限結構，不是任意有向圖。
 *   自己算可以精確控制分支的對稱性與間距，也少一個相依。
 *   若日後需要支援複雜巢狀，再換 dagre。
 */

import type { WorkflowContent, WorkflowEdge, WorkflowNode } from '@/api/workflows'

export const NODE_WIDTH = 320
export const NODE_HEIGHT = 84
export const VERTICAL_GAP = 56
export const HORIZONTAL_GAP = 40

export interface PositionedNode {
  node: WorkflowNode
  x: number
  y: number
  /** 同層的分支索引，0 為主幹 */
  branch: number
}

export interface LayoutEdge {
  from: string
  to: string
  when?: boolean
  label?: string
  /** 退回路徑。虛線紅色，走左側外緣。 */
  isReturn: boolean
  path: string
}

export interface Layout {
  nodes: PositionedNode[]
  edges: LayoutEdge[]
  width: number
  height: number
}

function edgeParts(e: WorkflowEdge) {
  return {
    from: e[0],
    to: e[1],
    when: e[2]?.when,
    label: e[2]?.label,
  }
}

/**
 * 計算佈局
 *
 * 演算法：
 *   1. 從 trigger 開始 BFS，計算每個節點的層級（深度）
 *   2. 同層節點水平並排，整體置中
 *   3. 退回路徑（on_reject 的 goto）走左側外緣，不參與層級計算
 */
export function computeLayout(content: WorkflowContent): Layout {
  const { nodes, edges } = content
  if (nodes.length === 0) {
    return { nodes: [], edges: [], width: 0, height: 0 }
  }

  const byId = new Map(nodes.map((n) => [n.id, n]))
  const forward = buildForwardAdjacency(edges, byId)

  const trigger = nodes.find((n) => n.type === 'trigger')
  const start = trigger?.id ?? nodes[0].id

  const levels = assignLevels(start, forward, nodes)
  const positioned = positionNodes(levels, byId)
  const posById = new Map(positioned.map((p) => [p.node.id, p]))

  const layoutEdges = [
    ...routeForwardEdges(edges, posById),
    ...routeReturnEdges(nodes, posById),
  ]

  const maxX = Math.max(...positioned.map((p) => p.x + NODE_WIDTH), NODE_WIDTH)
  const maxY = Math.max(...positioned.map((p) => p.y + NODE_HEIGHT), NODE_HEIGHT)

  return {
    nodes: positioned,
    edges: layoutEdges,
    // 左側留白給退回路徑
    width: maxX + HORIZONTAL_GAP * 2,
    height: maxY + VERTICAL_GAP,
  }
}

function buildForwardAdjacency(
  edges: WorkflowEdge[],
  byId: Map<string, WorkflowNode>,
): Map<string, string[]> {
  const adj = new Map<string, string[]>()
  for (const id of byId.keys()) adj.set(id, [])

  for (const e of edges) {
    const { from, to } = edgeParts(e)
    if (byId.has(from) && byId.has(to)) {
      adj.get(from)!.push(to)
    }
  }
  return adj
}

/**
 * 指派層級
 *
 * 用最長路徑而非最短：若節點同時是短路徑與長路徑的終點，
 * 放在較深的層級才不會讓邊向上回繞。
 */
function assignLevels(
  start: string,
  forward: Map<string, string[]>,
  nodes: WorkflowNode[],
): Map<number, string[]> {
  const depth = new Map<string, number>()
  depth.set(start, 0)

  // 迭代到收斂。節點數不多，最多 n 輪。
  let changed = true
  let guard = nodes.length + 1
  while (changed && guard-- > 0) {
    changed = false
    for (const [from, tos] of forward) {
      const d = depth.get(from)
      if (d === undefined) continue
      for (const to of tos) {
        const next = d + 1
        if ((depth.get(to) ?? -1) < next) {
          depth.set(to, next)
          changed = true
        }
      }
    }
  }

  // 只靠 goto 進入的節點（例如 revise）不在 forward 圖中。
  //
  // 放在最後一層會讓 end 不再是視覺上的終點，流程圖失去「由上往下
  // 走到底就結束」的直覺。改為放在其出邊目標的前一層，
  // 這樣 revise 會出現在它要跳回的節點旁邊，語意也較貼近。
  const placed = new Set(depth.keys())
  let pending = nodes.filter((n) => !placed.has(n.id))

  let guard2 = pending.length + 1
  while (pending.length > 0 && guard2-- > 0) {
    const stillPending: WorkflowNode[] = []

    for (const n of pending) {
      // 取其後繼節點中最淺的層級，放在它前一層
      const successors = forward.get(n.id) ?? []
      const depths = successors
        .map((s) => depth.get(s))
        .filter((d): d is number => d !== undefined)

      if (depths.length > 0) {
        // 與後繼同層而非前一層。前一層可能撞到 trigger（第 0 層），
        // 讓修改節點擠在開始節點旁邊，視覺上像是流程有兩個起點。
        // 同層則會並排在它要跳回的節點旁，語意清楚。
        depth.set(n.id, Math.min(...depths))
      } else {
        stillPending.push(n)
      }
    }

    if (stillPending.length === pending.length) break // 無進展
    pending = stillPending
  }

  // 完全孤立的節點放最後一層，至少讓使用者看得到它存在
  const maxDepth = Math.max(0, ...depth.values())
  for (const n of pending) {
    depth.set(n.id, maxDepth + 1)
  }

  const levels = new Map<number, string[]>()
  for (const [id, d] of depth) {
    if (!levels.has(d)) levels.set(d, [])
    levels.get(d)!.push(id)
  }
  return levels
}

function positionNodes(
  levels: Map<number, string[]>,
  byId: Map<string, WorkflowNode>,
): PositionedNode[] {
  const result: PositionedNode[] = []
  const sorted = [...levels.keys()].sort((a, b) => a - b)

  // 畫布中心。留左側空間給退回路徑。
  const maxPerLevel = Math.max(...[...levels.values()].map((v) => v.length))
  const totalWidth = maxPerLevel * NODE_WIDTH + (maxPerLevel - 1) * HORIZONTAL_GAP
  const centerX = HORIZONTAL_GAP * 2 + totalWidth / 2

  for (const level of sorted) {
    const ids = levels.get(level)!
    const rowWidth = ids.length * NODE_WIDTH + (ids.length - 1) * HORIZONTAL_GAP
    const startX = centerX - rowWidth / 2

    ids.forEach((id, i) => {
      const node = byId.get(id)
      if (!node) return
      result.push({
        node,
        x: startX + i * (NODE_WIDTH + HORIZONTAL_GAP),
        y: level * (NODE_HEIGHT + VERTICAL_GAP),
        branch: i,
      })
    })
  }

  return result
}

function routeForwardEdges(
  edges: WorkflowEdge[],
  pos: Map<string, PositionedNode>,
): LayoutEdge[] {
  const result: LayoutEdge[] = []

  for (const e of edges) {
    const { from, to, when, label } = edgeParts(e)
    const a = pos.get(from)
    const b = pos.get(to)
    if (!a || !b) continue

    const x1 = a.x + NODE_WIDTH / 2
    const y1 = a.y + NODE_HEIGHT
    const x2 = b.x + NODE_WIDTH / 2
    const y2 = b.y

    result.push({
      from,
      to,
      when,
      label,
      isReturn: false,
      path: verticalPath(x1, y1, x2, y2),
    })
  }

  return result
}

/**
 * 退回路徑
 *
 * 走左側外緣，避免與主幹交錯。虛線紅色以區別。
 */
function routeReturnEdges(
  nodes: WorkflowNode[],
  pos: Map<string, PositionedNode>,
): LayoutEdge[] {
  const result: LayoutEdge[] = []

  for (const n of nodes) {
    for (const key of ['on_reject', 'on_failure'] as const) {
      const p = n[key]
      if (p?.action !== 'goto' || !p.node) continue

      const a = pos.get(n.id)
      const b = pos.get(p.node)
      if (!a || !b) continue

      const laneX = HORIZONTAL_GAP
      const x1 = a.x
      const y1 = a.y + NODE_HEIGHT / 2
      const x2 = b.x
      const y2 = b.y + NODE_HEIGHT / 2

      result.push({
        from: n.id,
        to: p.node,
        label: key === 'on_reject' ? '退回' : '失敗',
        isReturn: true,
        path: [
          `M ${x1} ${y1}`,
          `L ${laneX + 20} ${y1}`,
          `Q ${laneX} ${y1} ${laneX} ${y1 - 20}`,
          `L ${laneX} ${y2 + 20}`,
          `Q ${laneX} ${y2} ${laneX + 20} ${y2}`,
          `L ${x2} ${y2}`,
        ].join(' '),
      })
    }
  }

  return result
}

/** 帶圓角的垂直連接線 */
function verticalPath(x1: number, y1: number, x2: number, y2: number): string {
  if (Math.abs(x1 - x2) < 1) {
    return `M ${x1} ${y1} L ${x2} ${y2}`
  }

  const midY = y1 + (y2 - y1) / 2
  const r = Math.min(16, Math.abs(x2 - x1) / 2, Math.abs(y2 - y1) / 2)
  const dir = x2 > x1 ? 1 : -1

  return [
    `M ${x1} ${y1}`,
    `L ${x1} ${midY - r}`,
    `Q ${x1} ${midY} ${x1 + r * dir} ${midY}`,
    `L ${x2 - r * dir} ${midY}`,
    `Q ${x2} ${midY} ${x2} ${midY + r}`,
    `L ${x2} ${y2}`,
  ].join(' ')
}

/**
 * 取得節點的插入點
 *
 * 使用者點擊連接線上的加號時，在該處插入新節點。
 * 回傳每條邊的中點座標。
 */
export function insertionPoints(layout: Layout): {
  from: string
  to: string
  x: number
  y: number
}[] {
  return layout.edges
    .filter((e) => !e.isReturn)
    .map((e) => {
      const match = /M ([\d.]+) ([\d.]+)/.exec(e.path)
      const x1 = Number(match?.[1] ?? 0)
      const y1 = Number(match?.[2] ?? 0)
      return {
        from: e.from,
        to: e.to,
        x: x1,
        y: y1 + VERTICAL_GAP / 2,
      }
    })
}
