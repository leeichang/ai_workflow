/**
 * 流程設計器狀態
 *
 * 與表單設計器同樣採整份快照的 undo / redo。
 *
 * 流程比表單多一層難處：節點與邊是兩份資料，任何節點操作
 * 都必須同時維護 edges，否則圖會斷裂。所有變更因此集中在
 * 這裡，畫面只呼叫語意化的動作，不直接碰 nodes / edges。
 */

import { computed, ref, shallowRef } from 'vue'
import type {
  GraphError,
  WorkflowContent,
  WorkflowEdge,
  WorkflowNode,
} from '@/api/workflows'
import { ApiError } from '@/api/types'
import {
  getWorkflow,
  publishWorkflowDraft,
  saveWorkflowDraft,
  validateWorkflowDraft,
} from '@/api/workflows'
import { computeLayout } from './layout'

const MAX_HISTORY = 50

export function useWorkflowDesigner(workflowKey: string) {
  const content = ref<WorkflowContent | null>(null)
  const selectedId = ref<string | null>(null)

  const loading = ref(false)
  const saving = ref(false)
  const error = ref<string | null>(null)
  const graphErrors = ref<GraphError[]>([])
  /**
   * 不影響發布的提醒
   *
   * 目前只有一種——業務物件未定義，該流程的路徑不會被 WF-E012 檢查。
   * 與 graphErrors 分開：警告不擋下發布，但使用者要看得到。
   */
  const warnings = ref<string[]>([])
  const validatedAt = ref<Date | null>(null)

  const lastSavedAt = ref<Date | null>(null)
  const publishedVersion = ref<number | null>(null)

  const past = shallowRef<string[]>([])
  const future = shallowRef<string[]>([])
  const dirty = ref(false)

  const canUndo = computed(() => past.value.length > 0)
  const canRedo = computed(() => future.value.length > 0)

  const selectedNode = computed(
    () => content.value?.nodes.find((n) => n.id === selectedId.value) ?? null,
  )

  const layout = computed(() =>
    content.value ? computeLayout(content.value) : null,
  )

  /** 出錯節點的 id 集合，畫布據此加紅框 */
  const errorNodeIds = computed(
    () => new Set(graphErrors.value.map((e) => e.node_id).filter((id): id is string => !!id)),
  )

  function snapshot() {
    if (!content.value) return
    past.value = [...past.value, JSON.stringify(content.value)].slice(-MAX_HISTORY)
    future.value = []
    dirty.value = true
    // 圖改了，上次的驗證結果就不再成立
    graphErrors.value = []
    validatedAt.value = null
  }

  function undo() {
    if (!canUndo.value || !content.value) return
    const previous = past.value[past.value.length - 1]
    future.value = [JSON.stringify(content.value), ...future.value]
    past.value = past.value.slice(0, -1)
    content.value = JSON.parse(previous)
    dirty.value = true
  }

  function redo() {
    if (!canRedo.value || !content.value) return
    const next = future.value[0]
    past.value = [...past.value, JSON.stringify(content.value)]
    future.value = future.value.slice(1)
    content.value = JSON.parse(next)
    dirty.value = true
  }

  // ── 節點操作 ──────────────────────────────────────────

  /**
   * 在指定的邊上插入節點
   *
   * 原本 a → b 變成 a → new → b。原邊的 meta（例如 condition 的
   * when）要留在 a → new 這一段，否則分支條件會消失。
   */
  function insertNode(node: WorkflowNode, on: { from: string; to: string }) {
    if (!content.value) return
    snapshot()

    const id = uniqueId(node.id, content.value.nodes)
    const inserted = { ...node, id }

    const edges: WorkflowEdge[] = []
    for (const e of content.value.edges) {
      if (e[0] === on.from && e[1] === on.to) {
        const meta = e[2]
        edges.push(meta ? [on.from, id, meta] : [on.from, id])
        edges.push([id, on.to])
      } else {
        edges.push(e)
      }
    }

    content.value = {
      ...content.value,
      nodes: [...content.value.nodes, inserted],
      edges,
    }
    selectedId.value = id
  }

  /**
   * 附加節點到某節點之後
   *
   * 用於該節點還沒有出邊的情況（例如剛加的 condition 的 false 分支）。
   */
  function appendNode(node: WorkflowNode, afterId: string, meta?: { when?: boolean }) {
    if (!content.value) return
    snapshot()

    const id = uniqueId(node.id, content.value.nodes)
    const edge: WorkflowEdge = meta ? [afterId, id, meta] : [afterId, id]

    content.value = {
      ...content.value,
      nodes: [...content.value.nodes, { ...node, id }],
      edges: [...content.value.edges, edge],
    }
    selectedId.value = id
  }

  // ── 並行分支 ──────────────────────────────────────────

  /**
   * 在指定的邊上插入一組並行
   *
   * 一次建立 parallel + N 個分支 + join，而不是讓使用者分開加。
   * 單獨插入一個 parallel 會立刻觸發 WF-E009（分支不足）與
   * WF-E010（沒有匯合點），使用者得連做好幾步才能回到合法狀態。
   *
   * 原本 a → b 變成：
   *   a → parallel ⇉ 分支1..N ⇉ join → b
   */
  function insertParallel(on: { from: string; to: string }, branchCount = 2) {
    if (!content.value) return
    snapshot()

    const existing = [...content.value.nodes]
    const parallelId = uniqueId('parallel', existing)
    const joinId = uniqueId('join', [...existing, { id: parallelId } as WorkflowNode])

    const added: WorkflowNode[] = [
      { id: parallelId, type: 'parallel', label: '並行開始' },
      {
        id: joinId,
        type: 'join',
        label: '匯合',
        join: { completion: 'ALL', result: 'ALL_SUCCESS' },
      },
    ]

    const branchIds: string[] = []
    for (let i = 0; i < branchCount; i += 1) {
      const id = uniqueId('branch', [...existing, ...added])
      branchIds.push(id)
      added.push({
        id,
        type: 'human_approval',
        label: `分支簽核 ${i + 1}`,
        participant: 'internal',
      })
    }

    const edges: WorkflowEdge[] = []
    for (const e of content.value.edges) {
      if (e[0] === on.from && e[1] === on.to) {
        // 原邊的 meta 留在 a → parallel，否則 condition 的分支條件會消失
        const meta = e[2]
        edges.push(meta ? [on.from, parallelId, meta] : [on.from, parallelId])
      } else {
        edges.push(e)
      }
    }
    for (const b of branchIds) {
      edges.push([parallelId, b])
      edges.push([b, joinId])
    }
    edges.push([joinId, on.to])

    content.value = {
      ...content.value,
      nodes: [...content.value.nodes, ...added],
      edges,
    }
    selectedId.value = parallelId
  }

  /** 為並行加一個分支，兩端都接好 */
  function addBranch(parallelId: string) {
    if (!content.value) return

    const joinId = findJoinOf(parallelId)
    if (joinId === null) {
      error.value = '找不到對應的匯合節點'
      return
    }

    snapshot()

    const id = uniqueId('branch', content.value.nodes)
    const count = content.value.edges.filter((e) => e[0] === parallelId).length

    content.value = {
      ...content.value,
      nodes: [
        ...content.value.nodes,
        {
          id,
          type: 'human_approval',
          label: `分支簽核 ${count + 1}`,
          participant: 'internal',
        },
      ],
      edges: [...content.value.edges, [parallelId, id], [id, joinId]],
    }
    selectedId.value = id
  }

  /**
   * 移除一個分支
   *
   * 低於兩個分支的 parallel 沒有意義，驗證也會擋（WF-E009）。
   * 與其讓使用者刪到非法狀態再看錯誤，不如直接不給刪。
   */
  function removeBranch(parallelId: string, branchId: string) {
    if (!content.value) return

    const branches = content.value.edges
      .filter((e) => e[0] === parallelId)
      .map((e) => e[1])

    if (branches.length <= 2) {
      error.value = '並行至少需要 2 個分支'
      return
    }

    snapshot()

    // 分支可能有多個節點，整段都要移除
    const toRemove = collectBranchNodes(branchId, findJoinOf(parallelId))

    content.value = {
      ...content.value,
      nodes: content.value.nodes
        .filter((n) => !toRemove.has(n.id))
        .map((n) => clearGotoInto(n, toRemove)),
      edges: content.value.edges.filter(
        (e) => !toRemove.has(e[0]) && !toRemove.has(e[1]),
      ),
    }

    if (selectedId.value !== null && toRemove.has(selectedId.value)) {
      selectedId.value = null
    }
  }

  /** 找 parallel 對應的 join */
  function findJoinOf(parallelId: string): string | null {
    if (!content.value) return null

    const seen = new Set<string>()
    const queue = content.value.edges
      .filter((e) => e[0] === parallelId)
      .map((e) => e[1])

    while (queue.length > 0) {
      const id = queue.shift()!
      if (seen.has(id)) continue
      seen.add(id)

      const node = content.value.nodes.find((n) => n.id === id)
      if (node?.type === 'join') return id

      queue.push(
        ...content.value.edges.filter((e) => e[0] === id).map((e) => e[1]),
      )
    }
    return null
  }

  /** 收集一個分支從起點到 join（不含）之間的所有節點 */
  function collectBranchNodes(start: string, joinId: string | null): Set<string> {
    const result = new Set<string>()
    if (!content.value) return result

    const queue = [start]
    while (queue.length > 0) {
      const id = queue.shift()!
      if (result.has(id) || id === joinId) continue
      result.add(id)
      queue.push(
        ...content.value.edges.filter((e) => e[0] === id).map((e) => e[1]),
      )
    }
    return result
  }

  function updateNode(id: string, patch: Partial<WorkflowNode>) {
    if (!content.value) return
    snapshot()

    content.value = {
      ...content.value,
      nodes: content.value.nodes.map((n) => (n.id === id ? mergeNode(n, patch) : n)),
    }
  }

  /**
   * 刪除節點並接合前後
   *
   * 直接刪會讓圖斷成兩半。把每個前驅接到每個後繼，流程才仍然連通。
   * 同時要清掉指向它的 goto，否則驗證會報 WF-E006（目標不存在）。
   */
  function removeNode(id: string) {
    if (!content.value) return

    const target = content.value.nodes.find((n) => n.id === id)
    if (!target) return
    if (target.type === 'trigger') {
      error.value = '開始節點不可刪除'
      return
    }

    // parallel 與 join 必須成對存在。刪掉其一會讓另一個立刻變成
    // 非法狀態（WF-E010），所以整組一起刪。
    if (target.type === 'parallel' || target.type === 'join') {
      removeParallelGroup(target.type === 'parallel' ? id : null, id)
      return
    }

    snapshot()

    const predecessors = content.value.edges.filter((e) => e[1] === id).map((e) => e[0])
    const successors = content.value.edges.filter((e) => e[0] === id).map((e) => e[1])

    const kept = content.value.edges.filter((e) => e[0] !== id && e[1] !== id)
    const bridged: WorkflowEdge[] = []
    for (const p of predecessors) {
      for (const s of successors) {
        if (p === s) continue
        if (kept.some((e) => e[0] === p && e[1] === s)) continue
        if (bridged.some((e) => e[0] === p && e[1] === s)) continue
        bridged.push([p, s])
      }
    }

    content.value = {
      ...content.value,
      nodes: content.value.nodes
        .filter((n) => n.id !== id)
        .map((n) => clearDanglingGoto(n, id)),
      edges: [...kept, ...bridged],
    }

    if (selectedId.value === id) selectedId.value = null
  }

  /**
   * 刪除整組並行
   *
   * parallel、所有分支、join 一起移除，並把 parallel 的前驅
   * 接到 join 的後繼——否則圖會斷成兩半。
   *
   * anyId 可以是 parallel 或 join 的 id，兩者都能定位整組。
   */
  function removeParallelGroup(parallelId: string | null, anyId: string) {
    if (!content.value) return

    const pid = parallelId ?? findParallelOf(anyId)
    if (pid === null) {
      error.value = '找不到對應的並行節點'
      return
    }
    const jid = findJoinOf(pid)
    if (jid === null) {
      error.value = '找不到對應的匯合節點'
      return
    }

    snapshot()

    const toRemove = new Set<string>([pid, jid])
    for (const e of content.value.edges) {
      if (e[0] !== pid) continue
      for (const n of collectBranchNodes(e[1], jid)) toRemove.add(n)
    }

    const before = content.value.edges
      .filter((e) => e[1] === pid && !toRemove.has(e[0]))
      .map((e) => e[0])
    const after = content.value.edges
      .filter((e) => e[0] === jid && !toRemove.has(e[1]))
      .map((e) => e[1])

    const kept = content.value.edges.filter(
      (e) => !toRemove.has(e[0]) && !toRemove.has(e[1]),
    )
    const bridged: WorkflowEdge[] = []
    for (const p of before) {
      for (const s of after) {
        if (p === s) continue
        if (kept.some((e) => e[0] === p && e[1] === s)) continue
        if (bridged.some((e) => e[0] === p && e[1] === s)) continue
        bridged.push([p, s])
      }
    }

    content.value = {
      ...content.value,
      nodes: content.value.nodes
        .filter((n) => !toRemove.has(n.id))
        .map((n) => clearGotoInto(n, toRemove)),
      edges: [...kept, ...bridged],
    }

    if (selectedId.value !== null && toRemove.has(selectedId.value)) {
      selectedId.value = null
    }
  }

  /** 由 join 反查它的 parallel */
  function findParallelOf(joinId: string): string | null {
    if (!content.value) return null
    for (const n of content.value.nodes) {
      if (n.type !== 'parallel') continue
      if (findJoinOf(n.id) === joinId) return n.id
    }
    return null
  }

  /** 設定退回目標。傳 null 代表改為直接結束流程。 */
  function setRejectTarget(id: string, targetId: string | null) {
    updateNode(id, {
      on_reject: targetId
        ? { action: 'goto', node: targetId }
        : { action: 'end', result: 'rejected' },
    })
  }

  // ── 伺服器往返 ────────────────────────────────────────

  async function load() {
    loading.value = true
    error.value = null
    try {
      const detail = await getWorkflow(workflowKey)
      const source = detail.draft ?? detail.published
      if (!source) {
        error.value = '此流程尚無任何版本'
        return
      }
      content.value = source.content
      publishedVersion.value = detail.published?.version ?? null
      past.value = []
      future.value = []
      dirty.value = false
      graphErrors.value = []
    } catch (e) {
      error.value = e instanceof ApiError ? e.message : '載入失敗'
    } finally {
      loading.value = false
    }
  }

  async function save() {
    if (!content.value || saving.value) return
    saving.value = true
    error.value = null
    try {
      await saveWorkflowDraft(workflowKey, content.value)
      lastSavedAt.value = new Date()
      dirty.value = false
    } catch (e) {
      error.value = e instanceof ApiError ? e.message : '儲存失敗'
      throw e
    } finally {
      saving.value = false
    }
  }

  async function validate(): Promise<boolean> {
    error.value = null
    try {
      if (dirty.value) await save()
      const result = await validateWorkflowDraft(workflowKey)
      graphErrors.value = result.errors ?? []
      warnings.value = result.warnings ?? []
      validatedAt.value = new Date()
      return result.valid
    } catch (e) {
      error.value = e instanceof ApiError ? e.message : '驗證失敗'
      return false
    }
  }

  async function publish(): Promise<boolean> {
    error.value = null
    graphErrors.value = []
    try {
      if (dirty.value) await save()
      const version = await publishWorkflowDraft(workflowKey)
      // 發布可能帶著警告成功——業務物件未定義時仍會發布
      warnings.value = version.warnings ?? []
      publishedVersion.value = version.version
      dirty.value = false
      await load()
      return true
    } catch (e) {
      if (e instanceof ApiError) {
        error.value = e.message
        // 後端發布失敗時，圖錯誤放在 validationErrors（字串），
        // 這裡轉成同一種結構讓畫面只處理一種型別
        graphErrors.value = e.validationErrors.map((m) => ({
          code: '',
          node_id: null,
          message: m,
        }))
      } else {
        error.value = '發布失敗'
      }
      return false
    }
  }

  return {
    content,
    layout,
    selectedId,
    selectedNode,
    loading,
    saving,
    error,
    graphErrors,
    warnings,
    errorNodeIds,
    validatedAt,
    lastSavedAt,
    publishedVersion,
    dirty,
    canUndo,
    canRedo,
    undo,
    redo,
    insertNode,
    appendNode,
    insertParallel,
    addBranch,
    removeBranch,
    updateNode,
    removeNode,
    setRejectTarget,
    load,
    save,
    validate,
    publish,
  }
}

/** 節點 id 必須唯一，重複時加序號 */
function uniqueId(base: string, nodes: WorkflowNode[]): string {
  const taken = new Set(nodes.map((n) => n.id))
  if (!taken.has(base)) return base

  let i = 2
  while (taken.has(`${base}_${i}`)) i += 1
  return `${base}_${i}`
}

/**
 * 移除指向任一已刪除節點的 goto
 *
 * 刪整組並行時一次移除多個節點，逐一呼叫 clearDanglingGoto
 * 會重複複製物件，而且容易漏掉。
 */
function clearGotoInto(node: WorkflowNode, removed: Set<string>): WorkflowNode {
  const next = { ...node }
  let changed = false

  for (const key of ['on_reject', 'on_failure'] as const) {
    const p = next[key]
    if (p?.action === 'goto' && p.node !== undefined && removed.has(p.node)) {
      next[key] = { action: 'end', result: 'rejected' }
      changed = true
    }
  }

  return changed ? next : node
}

/** 移除指向已刪除節點的 goto，避免留下懸空參照 */
function clearDanglingGoto(node: WorkflowNode, removedId: string): WorkflowNode {
  const next = { ...node }
  let changed = false

  for (const key of ['on_reject', 'on_failure'] as const) {
    const p = next[key]
    if (p?.action === 'goto' && p.node === removedId) {
      next[key] = { action: 'end', result: 'rejected' }
      changed = true
    }
  }

  return changed ? next : node
}

function mergeNode(base: WorkflowNode, patch: Partial<WorkflowNode>): WorkflowNode {
  return {
    ...base,
    ...patch,
    // resolver 是整組取代而非合併：切換 type 時舊欄位必須消失，
    // 否則會留下 role 的 value 混在 user 的設定裡，送到後端才被擋。
  }
}
