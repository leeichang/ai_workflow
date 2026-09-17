/**
 * 節點庫測試
 */

import { describe, expect, it } from 'vitest'
import {
  NODE_PALETTE,
  NODE_PALETTE_COUNT,
  NODE_STYLE,
  nodeTypeLabel,
  resolverLabel,
} from '@/workflow/palette'
import type { NodeType } from '@/api/workflows'

const ALL_TYPES: NodeType[] = [
  'trigger',
  'condition',
  'human_approval',
  'human_task',
  'parallel',
  'join',
  'action',
  'notification',
  'end',
]

describe('節點庫', () => {
  it('分四組，共 8 種可新增的節點', () => {
    // trigger 不在節點庫：每個流程只有一個開始節點，
    // 由流程建立時產生，使用者不該再新增。
    expect(NODE_PALETTE.map((g) => g.key)).toEqual([
      'control',
      'human',
      'system',
      'terminal',
    ])
    expect(NODE_PALETTE_COUNT).toBe(8)
  })

  it('節點庫不含 trigger', () => {
    const types = NODE_PALETTE.flatMap((g) => g.items.map((i) => i.type))
    expect(types).not.toContain('trigger')
  })

  it('每種節點都能產生帶 id 與 type 的骨架', () => {
    for (const g of NODE_PALETTE) {
      for (const item of g.items) {
        const n = item.defaults()
        expect(n.id, `${item.label} 缺 id`).toBeTruthy()
        expect(n.type, `${item.label} 缺 type`).toBe(item.type)
      }
    }
  })

  it('連續新增的 id 不重複', () => {
    const item = NODE_PALETTE[1].items[0]
    const a = item.defaults().id
    const b = item.defaults().id
    expect(a).not.toBe(b)
  })

  it('每種節點型別都有視覺樣式', () => {
    for (const t of ALL_TYPES) {
      expect(NODE_STYLE[t], `${t} 缺樣式`).toBeDefined()
      expect(NODE_STYLE[t].icon).toBeTruthy()
    }
  })

  it('每種節點型別都有中文名稱', () => {
    for (const t of ALL_TYPES) {
      expect(nodeTypeLabel(t)).not.toBe(t)
    }
  })
})

describe('簽核人顯示', () => {
  it('manager_of initiator 顯示為申請人主管', () => {
    expect(resolverLabel({ type: 'manager_of', of: 'initiator' })).toBe('申請人主管')
  })

  it('role 帶出角色代碼', () => {
    expect(resolverLabel({ type: 'role', value: 'finance' })).toContain('finance')
  })

  it('未設定時明確標示', () => {
    expect(resolverLabel(undefined)).toBe('未設定')
  })

  it('composite 顯示子項數量', () => {
    const r = { type: 'composite', of: [{}, {}] as unknown[] }
    expect(resolverLabel(r as never)).toContain('2')
  })
})
