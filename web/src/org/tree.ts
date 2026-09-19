/**
 * 部門樹的組建
 *
 * 從扁平清單組成樹。單獨抽出來是因為**孤兒節點**的處理容易出錯：
 * `parent_id` 指向不存在的部門時，若只從根節點往下走，那棵子樹
 * 會整個消失在畫面上——使用者看不到，也就無從修正。
 *
 * 組織健康檢查會把這種情況報成 DEPARTMENT_BROKEN_PARENT，
 * 但報表指向的部門必須在樹上點得到才修得了。
 */

import type { Department } from '@/api/org'

export interface DeptNode extends Department {
  children: DeptNode[]
}

/**
 * 組樹
 *
 * 回傳頂層節點。`parent_id` 為 null 的是真正的根；
 * `parent_id` 指向不存在的部門的，**也當成根**——
 * 寧可位置放錯也不要讓它消失。
 */
export function buildTree(departments: Department[]): DeptNode[] {
  const nodes = new Map<string, DeptNode>()
  for (const dept of departments) {
    nodes.set(dept.id, { ...dept, children: [] })
  }

  const roots: DeptNode[] = []
  for (const node of nodes.values()) {
    const parent = node.parent_id ? nodes.get(node.parent_id) : undefined
    if (parent) {
      parent.children.push(node)
    } else {
      // parent_id 指向不存在的部門時也會走到這裡。
      // 孤兒放到頂層，總比整棵子樹消失好
      roots.push(node)
    }
  }

  return roots
}

/**
 * 某個部門與它所有下層部門的 id
 *
 * 選取父部門時，員工清單要顯示整棵子樹的人——
 * 只顯示直屬成員會讓上層部門看起來幾乎是空的。
 *
 * 在前端算而非讓後端展開：後端的 `department_id` 只收單一值，
 * 而前端本來就有完整的部門清單。改後端等於為了一個畫面的需求
 * 在 API 上加一個「要不要含下層」的旗標，那是把 UI 的決定
 * 推進契約裡。
 */
export function descendantIds(nodes: DeptNode[], id: string): string[] {
  const found = findNode(nodes, id)
  if (!found) return [id]

  const result: string[] = []
  const stack: DeptNode[] = [found]
  while (stack.length > 0) {
    const node = stack.pop()!
    result.push(node.id)
    stack.push(...node.children)
  }
  return result
}

function findNode(nodes: DeptNode[], id: string): DeptNode | undefined {
  for (const node of nodes) {
    if (node.id === id) return node
    const inChild = findNode(node.children, id)
    if (inChild) return inChild
  }
  return undefined
}
