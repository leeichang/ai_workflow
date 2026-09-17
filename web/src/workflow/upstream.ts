/**
 * 上游節點查詢
 *
 * 退回目標必須在上游，這是後端 WF-E004 的規則。
 * 前端先過濾一次，讓使用者根本選不到會被擋下的選項。
 *
 * 關鍵：只能沿 edges 回溯，不能含 on_reject / on_failure 的 goto。
 * 含了 goto 就會把退回路徑當成正常前進路徑，於是 A 退回 B、
 * B 又能退回 A，形成後端會拒絕的迴圈。
 * （可達性分析 WF-E002 則相反，必須含 goto。見 schemas/README.md）
 */

import type { WorkflowContent, WorkflowNode } from '@/api/workflows'

export interface UpstreamOption {
  id: string
  label: string
}

/** 取得指定節點的所有上游節點（沿 edges 回溯，不含自己） */
export function upstreamOf(content: WorkflowContent, nodeId: string): Set<string> {
  const incoming = new Map<string, string[]>()
  for (const e of content.edges) {
    if (!incoming.has(e[1])) incoming.set(e[1], [])
    incoming.get(e[1])!.push(e[0])
  }

  const seen = new Set<string>()
  const stack = [...(incoming.get(nodeId) ?? [])]

  while (stack.length > 0) {
    const id = stack.pop()!
    if (seen.has(id)) continue
    seen.add(id)
    stack.push(...(incoming.get(id) ?? []))
  }

  seen.delete(nodeId)
  return seen
}

/**
 * 可作為退回目標的節點
 *
 * 排除 trigger 與 end：退回到 trigger 等於重跑整個流程，
 * 語意上應該用「撤回」而不是退回；end 不可能在上游。
 */
export function rejectTargetsFor(
  content: WorkflowContent | null,
  nodeId: string | null,
): UpstreamOption[] {
  if (!content || !nodeId) return []

  const upstream = upstreamOf(content, nodeId)
  const byId = new Map(content.nodes.map((n) => [n.id, n]))

  const options: UpstreamOption[] = []
  for (const id of upstream) {
    const n = byId.get(id)
    if (!n || n.type === 'trigger' || n.type === 'end') continue
    options.push({ id, label: labelOf(n) })
  }

  // 依 nodes 的原始順序排列，使用者看到的順序才與畫布一致
  const order = new Map(content.nodes.map((n, i) => [n.id, i]))
  options.sort((a, b) => (order.get(a.id) ?? 0) - (order.get(b.id) ?? 0))

  return options
}

function labelOf(n: WorkflowNode): string {
  return n.label && n.label.length > 0 ? n.label : n.id
}
