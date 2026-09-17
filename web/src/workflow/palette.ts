/**
 * 流程節點庫
 *
 * 依決議：MVP 支援九種節點，分四類。
 * 分類依據是「誰在執行」而非技術實作：
 *   流程控制 系統自動判斷
 *   人工     需要人介入
 *   系統     呼叫外部
 *   結束     終點
 */

import type { NodeType, WorkflowNode } from '@/api/workflows'

export interface NodePaletteItem {
  type: NodeType
  label: string
  description: string
  icon: string
  /** 主色，用於節點左側色條與圖示 */
  accent: 'gray' | 'blue' | 'purple' | 'amber' | 'green' | 'dark'
  defaults: () => Partial<WorkflowNode>
}

export interface NodePaletteGroup {
  key: string
  title: string
  items: NodePaletteItem[]
}

let counter = 0

export function nextNodeId(prefix: string): string {
  counter += 1
  return `${prefix}_${counter}`
}

export const NODE_PALETTE: NodePaletteGroup[] = [
  {
    key: 'control',
    title: '流程控制',
    items: [
      {
        type: 'condition',
        label: '條件分支',
        description: '依表達式決定走向',
        icon: 'alt_route',
        accent: 'amber',
        defaults: () => ({
          id: nextNodeId('condition'),
          type: 'condition',
          label: '條件判斷',
          expression: '',
        }),
      },
      {
        type: 'parallel',
        label: '並行',
        description: '同時執行多個分支',
        icon: 'call_split',
        accent: 'gray',
        defaults: () => ({
          id: nextNodeId('parallel'),
          type: 'parallel',
          label: '並行開始',
        }),
      },
      {
        type: 'join',
        label: '匯合',
        description: '等待分支完成',
        icon: 'call_merge',
        accent: 'gray',
        defaults: () => ({
          id: nextNodeId('join'),
          type: 'join',
          label: '匯合',
          join: { completion: 'ALL', result: 'ALL_SUCCESS' },
        }),
      },
    ],
  },
  {
    key: 'human',
    title: '人工',
    items: [
      {
        type: 'human_approval',
        label: '人工簽核',
        description: '指派給人員核准或退回',
        icon: 'how_to_reg',
        accent: 'blue',
        defaults: () => ({
          id: nextNodeId('approval'),
          type: 'human_approval',
          label: '簽核',
          participant: 'internal',
          resolver: { type: 'manager_of', of: 'initiator' },
        }),
      },
      {
        type: 'human_task',
        label: '人工處理',
        description: '指派工作但不需核准決定',
        icon: 'assignment_ind',
        accent: 'blue',
        defaults: () => ({
          id: nextNodeId('task'),
          type: 'human_task',
          label: '處理',
          participant: 'internal',
          assignee: { type: 'initiator' },
        }),
      },
    ],
  },
  {
    key: 'system',
    title: '系統',
    items: [
      {
        type: 'action',
        label: '系統動作',
        description: '呼叫外部系統或執行運算',
        icon: 'bolt',
        accent: 'purple',
        defaults: () => ({
          id: nextNodeId('action'),
          type: 'action',
          label: '系統動作',
          action: '',
          retry: { max_attempts: 3, backoff: 'exponential' },
        }),
      },
      {
        type: 'notification',
        label: '發送通知',
        description: 'Email 或 LINE 通知相關人員',
        icon: 'notifications_active',
        accent: 'purple',
        defaults: () => ({
          id: nextNodeId('notify'),
          type: 'notification',
          label: '通知',
          channel: ['email'],
          to: ['initiator'],
        }),
      },
    ],
  },
  {
    key: 'terminal',
    title: '結束',
    items: [
      {
        type: 'end',
        label: '結束節點',
        description: '流程終點',
        icon: 'stop_circle',
        accent: 'dark',
        defaults: () => ({
          id: nextNodeId('end'),
          type: 'end',
          label: '結束',
          result: 'completed',
        }),
      },
    ],
  },
]

export const NODE_PALETTE_COUNT = NODE_PALETTE.reduce((n, g) => n + g.items.length, 0)

/** 節點型別的視覺樣式。畫布與節點庫共用。 */
export const NODE_STYLE: Record<
  NodeType,
  { icon: string; accent: string; bg: string; text: string }
> = {
  trigger: {
    icon: 'play_circle',
    accent: 'bg-[#15803D]',
    bg: 'bg-[#DCFCE7]',
    text: 'text-[#15803D]',
  },
  condition: {
    icon: 'alt_route',
    accent: 'bg-[#B45309]',
    bg: 'bg-[#FEF3C7]',
    text: 'text-[#B45309]',
  },
  human_approval: {
    icon: 'how_to_reg',
    accent: 'bg-primary-container',
    bg: 'bg-[#DBEAFE]',
    text: 'text-primary',
  },
  human_task: {
    icon: 'assignment_ind',
    accent: 'bg-primary-container',
    bg: 'bg-[#DBEAFE]',
    text: 'text-primary',
  },
  parallel: {
    icon: 'call_split',
    accent: 'bg-outline',
    bg: 'bg-surface-container-high',
    text: 'text-secondary',
  },
  join: {
    icon: 'call_merge',
    accent: 'bg-outline',
    bg: 'bg-surface-container-high',
    text: 'text-secondary',
  },
  action: {
    icon: 'bolt',
    accent: 'bg-[#7C3AED]',
    bg: 'bg-[#EDE9FE]',
    text: 'text-[#7C3AED]',
  },
  notification: {
    icon: 'notifications_active',
    accent: 'bg-[#7C3AED]',
    bg: 'bg-[#EDE9FE]',
    text: 'text-[#7C3AED]',
  },
  end: {
    icon: 'stop_circle',
    accent: 'bg-inverse-surface',
    bg: 'bg-surface-container-high',
    text: 'text-on-surface',
  },
}

export function nodeTypeLabel(type: NodeType): string {
  const map: Record<NodeType, string> = {
    trigger: '開始',
    condition: '條件分支',
    human_approval: '人工簽核',
    human_task: '人工處理',
    parallel: '並行',
    join: '匯合',
    action: '系統動作',
    notification: '發送通知',
    end: '結束',
  }
  return map[type] ?? type
}

/** 簽核人解析方式的顯示名稱 */
export function resolverLabel(r?: { type: string; value?: string; of?: unknown }): string {
  if (!r) return '未設定'

  switch (r.type) {
    case 'initiator':
      return '申請人本人'
    case 'manager_of':
      return typeof r.of === 'string' && r.of === 'initiator' ? '申請人主管' : '指定人員主管'
    case 'department_manager':
      return '部門主管'
    case 'role':
      return `角色：${r.value ?? '未指定'}`
    case 'user':
      return '指定人員'
    case 'external_contacts':
      return '外部聯絡人'
    case 'expression':
      return '進階條件'
    case 'composite': {
      const n = Array.isArray(r.of) ? r.of.length : 0
      return `組合（${n} 項）`
    }
    default:
      return r.type
  }
}
