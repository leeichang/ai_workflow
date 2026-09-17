/**
 * 逾時前提醒的設定測試
 *
 * 使用者可自己排程：到期前 2 天、1 天、12 小時、6 小時各發一次。
 * 設定在節點層，與 timeout 放在一起。
 */

import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import NodePropertyPanel from '@/workflow/NodePropertyPanel.vue'
import type { WorkflowNode } from '@/api/workflows'

function approvalNode(over: Partial<WorkflowNode> = {}): WorkflowNode {
  return {
    id: 'approve',
    type: 'human_approval',
    label: '主管簽核',
    timeout: { after: 'P3D', policy: 'AUTO_REJECT' },
    ...over,
  }
}

function render(node: WorkflowNode) {
  return mount(NodePropertyPanel, { props: { node, rejectTargets: [] } })
}

describe('提醒設定', () => {
  it('設了逾時才出現提醒欄位', () => {
    const withTimeout = render(approvalNode())
    expect(withTimeout.find('[data-testid="node-prop-remind-at"]').exists()).toBe(
      true,
    )

    const noTimeout = render(approvalNode({ timeout: undefined }))
    // 沒有期限就沒有「到期前」可言
    expect(noTimeout.find('[data-testid="node-prop-remind-at"]').exists()).toBe(
      false,
    )
  })

  it('顯示已設定的提醒時間點', () => {
    const w = render(
      approvalNode({
        timeout: { after: 'P3D', policy: 'AUTO_REJECT', remind_at: ['P1D', 'PT6H'] },
      }),
    )
    const input = w.find('[data-testid="node-prop-remind-at"]')
      .element as HTMLInputElement
    expect(input.value).toBe('P1D, PT6H')
  })

  it('逗號分隔輸入轉成陣列', async () => {
    const w = render(approvalNode())
    await w
      .find('[data-testid="node-prop-remind-at"]')
      .setValue('P2D, P1D, PT6H')

    const events = w.emitted('update')!
    const last = events[events.length - 1][0] as Partial<WorkflowNode>
    expect(last.timeout?.remind_at).toEqual(['P2D', 'P1D', 'PT6H'])
  })

  it('清空時移除 remind_at', async () => {
    const w = render(
      approvalNode({
        timeout: { after: 'P3D', policy: 'AUTO_REJECT', remind_at: ['P1D'] },
      }),
    )
    await w.find('[data-testid="node-prop-remind-at"]').setValue('')

    const events = w.emitted('update')!
    const last = events[events.length - 1][0] as Partial<WorkflowNode>
    expect(last.timeout?.remind_at).toBeUndefined()
  })

  it('忽略多餘的空白與空項目', async () => {
    const w = render(approvalNode())
    await w
      .find('[data-testid="node-prop-remind-at"]')
      .setValue('  P1D ,, PT6H , ')

    const events = w.emitted('update')!
    const last = events[events.length - 1][0] as Partial<WorkflowNode>
    expect(last.timeout?.remind_at).toEqual(['P1D', 'PT6H'])
  })

  it('join 也能設提醒', () => {
    const w = render({
      id: 'merge',
      type: 'join',
      label: '匯合',
      join: { completion: 'ALL', result: 'ALL_SUCCESS' },
      timeout: { after: 'P2D', policy: 'AUTO_REJECT' },
    })
    expect(w.find('[data-testid="node-prop-remind-at"]').exists()).toBe(true)
  })
})
