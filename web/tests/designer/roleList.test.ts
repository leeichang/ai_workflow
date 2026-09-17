/**
 * 角色清單三態測試
 *
 * 這組語意曾經出過缺陷：寫入端把空陣列的鍵整個移除，
 * 讀取端把缺鍵視為「不限制」，於是「拒絕所有人」存檔後
 * 變成「所有人都可以」，剛好相反。
 *
 * 往返一致性因此是這裡最重要的測試。
 */

import { describe, expect, it } from 'vitest'
import {
  readRoleList,
  toggleRole,
  writeRoleList,
  type RoleListState,
} from '@/designer/roleList'

describe('讀取狀態', () => {
  it('鍵不存在為不限制', () => {
    expect(readRoleList(undefined)).toEqual({ mode: 'unrestricted', roles: [] })
  })

  it('空陣列為全部拒絕', () => {
    expect(readRoleList([])).toEqual({ mode: 'deny_all', roles: [] })
  })

  it('有內容為指定角色', () => {
    expect(readRoleList(['approver'])).toEqual({
      mode: 'allow_list',
      roles: ['approver'],
    })
  })

  it('不共用原陣列', () => {
    // 共用會讓編輯直接改到 DSL，繞過 undo 快照
    const source = ['approver']
    const state = readRoleList(source)
    state.roles.push('admin')

    expect(source).toEqual(['approver'])
  })
})

describe('寫回 DSL', () => {
  it('不限制回傳 undefined 代表移除鍵', () => {
    expect(writeRoleList({ mode: 'unrestricted', roles: [] })).toBeUndefined()
  })

  it('全部拒絕回傳空陣列而非 undefined', () => {
    // 這是整組語意的關鍵。回 undefined 會讓「拒絕所有人」
    // 被存成「不限制」，權限完全相反。
    expect(writeRoleList({ mode: 'deny_all', roles: [] })).toEqual([])
  })

  it('指定角色回傳清單', () => {
    expect(writeRoleList({ mode: 'allow_list', roles: ['a', 'b'] })).toEqual(['a', 'b'])
  })

  it('指定角色時忽略殘留的 roles 以外欄位', () => {
    const state: RoleListState = { mode: 'deny_all', roles: ['a'] }
    expect(writeRoleList(state), 'deny_all 不該受 roles 影響').toEqual([])
  })
})

describe('往返一致', () => {
  const cases: (string[] | undefined)[] = [undefined, [], ['approver'], ['a', 'b']]

  for (const value of cases) {
    it(`${JSON.stringify(value)} 讀出再寫回不變`, () => {
      expect(writeRoleList(readRoleList(value))).toEqual(value)
    })
  }
})

describe('切換單一角色', () => {
  it('從不限制勾選第一個角色轉為指定角色', () => {
    const next = toggleRole({ mode: 'unrestricted', roles: [] }, 'approver')
    expect(next).toEqual({ mode: 'allow_list', roles: ['approver'] })
  })

  it('取消最後一個角色轉為全部拒絕而非不限制', () => {
    // 使用者剛把所有角色都取消，意圖是拒絕。
    // 轉成「不限制」等於放行所有人，與操作意圖相反。
    const next = toggleRole({ mode: 'allow_list', roles: ['approver'] }, 'approver')
    expect(next).toEqual({ mode: 'deny_all', roles: [] })
  })

  it('從全部拒絕勾選角色轉為指定角色', () => {
    const next = toggleRole({ mode: 'deny_all', roles: [] }, 'admin')
    expect(next).toEqual({ mode: 'allow_list', roles: ['admin'] })
  })

  it('再次勾選同一角色會取消', () => {
    const a = toggleRole({ mode: 'allow_list', roles: ['a', 'b'] }, 'b')
    expect(a.roles).toEqual(['a'])
  })

  it('不修改傳入的狀態', () => {
    const state: RoleListState = { mode: 'allow_list', roles: ['a'] }
    toggleRole(state, 'b')
    expect(state.roles).toEqual(['a'])
  })
})
