/**
 * join 逾時設定測試
 *
 * 並簽時某人長期不簽會讓整個流程卡住，逾時是唯一的出口。
 *
 * join 不支援 ESCALATE——分支各有自己的簽核人，加簽給誰沒有明確語意。
 * 選項清單要排除它，否則使用者設了才在驗證時被 WF-E009 擋下。
 */

import { describe, expect, it } from 'vitest'
import { mount } from '@vue/test-utils'
import NodePropertyPanel from '@/workflow/NodePropertyPanel.vue'
import type { WorkflowNode } from '@/api/workflows'

function joinNode(over: Partial<WorkflowNode> = {}): WorkflowNode {
  return {
    id: 'merge',
    type: 'join',
    label: '匯合',
    join: { completion: 'ALL', result: 'ALL_SUCCESS' },
    ...over,
  }
}

function render(node: WorkflowNode) {
  return mount(NodePropertyPanel, {
    props: { node, rejectTargets: [] },
  })
}

describe('join 的逾時設定', () => {
  it('提供逾時欄位', () => {
    const w = render(joinNode())
    expect(w.find('[data-testid="node-prop-timeout-after"]').exists()).toBe(true)
  })

  it('填入等待時間後出現策略選單', async () => {
    const w = render(joinNode({ timeout: { after: 'P2D', policy: 'AUTO_REJECT' } }))
    expect(w.find('[data-testid="node-prop-timeout-policy"]').exists()).toBe(true)
  })

  it('策略選單不含 ESCALATE', () => {
    const w = render(joinNode({ timeout: { after: 'P2D', policy: 'AUTO_REJECT' } }))
    const options = w
      .find('[data-testid="node-prop-timeout-policy"]')
      .findAll('option')
      .map((o) => o.attributes('value'))

    // join 不支援 ESCALATE，列出來只會讓人設了才被驗證擋下
    expect(options).not.toContain('ESCALATE')
    expect(options).toContain('AUTO_APPROVE')
    expect(options).toContain('AUTO_REJECT')
    expect(options).toContain('WAIT')
  })

  it('簽核節點仍保留 ESCALATE', () => {
    const w = render({
      id: 'approve',
      type: 'human_approval',
      label: '簽核',
      timeout: { after: 'P2D', policy: 'ESCALATE' },
    })
    const options = w
      .find('[data-testid="node-prop-timeout-policy"]')
      .findAll('option')
      .map((o) => o.attributes('value'))

    expect(options).toContain('ESCALATE')
  })

  it('改變策略會發出更新事件', async () => {
    const w = render(joinNode({ timeout: { after: 'P2D', policy: 'WAIT' } }))
    await w
      .find('[data-testid="node-prop-timeout-policy"]')
      .setValue('AUTO_REJECT')

    const events = w.emitted('update')
    expect(events).toBeTruthy()
    const last = events![events!.length - 1][0] as Partial<WorkflowNode>
    expect(last.timeout?.policy).toBe('AUTO_REJECT')
  })
})
