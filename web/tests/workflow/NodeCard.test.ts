/**
 * 節點卡片測試
 *
 * 卡片摘要是使用者不點開屬性面板時唯一的資訊來源，
 * 顯示錯了等於流程圖在說謊，因此逐型別驗證。
 */

import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import NodeCard from '@/workflow/NodeCard.vue'
import type { WorkflowNode } from '@/api/workflows'

function render(node: WorkflowNode, extra: Partial<Record<string, unknown>> = {}) {
  return mount(NodeCard, {
    props: {
      node,
      x: 100,
      y: 200,
      selected: false,
      hasError: false,
      ...extra,
    },
  })
}

describe('摘要文字', () => {
  it('人工簽核顯示簽核人', () => {
    const w = render({
      id: 'mgr',
      type: 'human_approval',
      label: '主管簽核',
      participant: 'internal',
      resolver: { type: 'manager_of', of: 'initiator' },
    })
    expect(w.get('[data-testid="node-summary-mgr"]').text()).toBe('申請人主管')
  })

  it('外部參與者加註外部', () => {
    const w = render({
      id: 'cust',
      type: 'human_approval',
      participant: 'external',
      resolver: { type: 'external_contacts' },
    })
    expect(w.get('[data-testid="node-summary-cust"]').text()).toContain('外部')
  })

  it('條件節點顯示表達式', () => {
    const w = render({ id: 'gate', type: 'condition', expression: 'amount > 100' })
    expect(w.get('[data-testid="node-summary-gate"]').text()).toBe('amount > 100')
  })

  it('未設定條件時明確提示而非空白', () => {
    const w = render({ id: 'gate', type: 'condition' })
    expect(w.get('[data-testid="node-summary-gate"]').text()).toContain('尚未設定')
  })

  it('通知顯示管道與對象', () => {
    const w = render({
      id: 'n1',
      type: 'notification',
      channel: ['email', 'line'],
      to: ['initiator'],
    })
    const text = w.get('[data-testid="node-summary-n1"]').text()
    expect(text).toContain('email')
    expect(text).toContain('initiator')
  })

  it('匯合顯示完成條件', () => {
    const w = render({
      id: 'j',
      type: 'join',
      join: { completion: 'N_OF_M', n: 2 },
    })
    expect(w.get('[data-testid="node-summary-j"]').text()).toContain('2 人')
  })

  it('結束節點區分結果狀態', () => {
    const w = render({ id: 'e', type: 'end', result: 'rejected' })
    expect(w.get('[data-testid="node-summary-e"]').text()).toContain('退回')
  })
})

describe('標記', () => {
  it('有逾時設定時顯示標記', () => {
    const w = render({
      id: 'a',
      type: 'human_approval',
      participant: 'internal',
      timeout: { after: 'P3D', policy: 'ESCALATE' },
    })
    expect(w.text()).toContain('P3D')
  })

  it('有退回目標時顯示標記', () => {
    const w = render({
      id: 'a',
      type: 'human_approval',
      participant: 'internal',
      on_reject: { action: 'goto', node: 'revise' },
    })
    expect(w.text()).toContain('退回至 revise')
  })

  it('退回設為結束時不顯示退回標記', () => {
    const w = render({
      id: 'a',
      type: 'human_approval',
      participant: 'internal',
      on_reject: { action: 'end', result: 'rejected' },
    })
    expect(w.text()).not.toContain('退回至')
  })
})

describe('狀態與互動', () => {
  it('定位使用傳入座標', () => {
    const w = render({ id: 'a', type: 'action', action: 'x' })
    const style = w.get('[data-testid="node-a"]').attributes('style')
    expect(style).toContain('left: 100px')
    expect(style).toContain('top: 200px')
  })

  it('錯誤狀態標記於 data 屬性', () => {
    const w = render({ id: 'a', type: 'action' }, { hasError: true })
    expect(w.get('[data-testid="node-a"]').attributes('data-error')).toBe('true')
  })

  it('點擊發出 select', async () => {
    const w = render({ id: 'a', type: 'action' })
    await w.get('[data-testid="node-a"]').trigger('click')
    expect(w.emitted('select')).toHaveLength(1)
  })

  it('trigger 節點不提供刪除按鈕', () => {
    // 流程沒有開始節點就無從啟動，刪掉只會讓驗證失敗，
    // 不如一開始就不給這個選項。
    const w = render({ id: 'start', type: 'trigger' })
    expect(w.find('[data-testid="node-remove-start"]').exists()).toBe(false)
  })

  it('其他節點的刪除按鈕發出 remove 且不觸發 select', async () => {
    const w = render({ id: 'a', type: 'action' })
    await w.get('[data-testid="node-remove-a"]').trigger('click')

    expect(w.emitted('remove')).toHaveLength(1)
    expect(w.emitted('select')).toBeUndefined()
  })

  it('無 label 時以型別名稱顯示', () => {
    const w = render({ id: 'a', type: 'action' })
    expect(w.text()).toContain('系統動作')
  })
})
