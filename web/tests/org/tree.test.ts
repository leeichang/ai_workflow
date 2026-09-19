/**
 * 部門樹組建測試
 *
 * 重點是孤兒節點：`parent_id` 指向不存在的部門時，那棵子樹
 * 不可以消失。組織健康檢查會把這種情況報成 DEPARTMENT_BROKEN_PARENT，
 * 但報表指向的部門必須在樹上點得到才修得了。
 */

import { describe, expect, it } from 'vitest'
import { buildTree, descendantIds } from '@/org/tree'
import type { Department } from '@/api/org'

function dept(
  id: string,
  name: string,
  parent_id: string | null = null,
): Department {
  return {
    id,
    code: id.toUpperCase(),
    name,
    parent_id,
    manager_user_id: null,
    manager_name: null,
    status: 'ACTIVE',
    sort_order: 0,
    source_system: 'MANUAL',
    platform_managed_fields: [],
    member_count: 0,
  }
}

describe('buildTree', () => {
  it('把扁平清單組成樹', () => {
    const tree = buildTree([
      dept('a', '總經理室'),
      dept('b', '業務部', 'a'),
      dept('c', '業務一課', 'b'),
    ])

    expect(tree).toHaveLength(1)
    expect(tree[0].name).toBe('總經理室')
    expect(tree[0].children[0].name).toBe('業務部')
    expect(tree[0].children[0].children[0].name).toBe('業務一課')
  })

  it('多個根節點都保留', () => {
    const tree = buildTree([dept('a', '業務部'), dept('b', '財務部')])
    expect(tree).toHaveLength(2)
  })

  it('parent_id 指向不存在的部門時當成根，不讓子樹消失', () => {
    const tree = buildTree([
      dept('a', '業務部'),
      // 上層部門 ghost 不在清單裡
      dept('b', '孤兒部門', 'ghost'),
      dept('c', '孤兒的下層', 'b'),
    ])

    expect(tree).toHaveLength(2)
    const orphan = tree.find((n) => n.name === '孤兒部門')
    expect(orphan).toBeDefined()
    // 孤兒的子樹也要跟著在
    expect(orphan!.children[0].name).toBe('孤兒的下層')
  })

  it('空清單回空陣列', () => {
    expect(buildTree([])).toEqual([])
  })

  it('不改動傳入的資料', () => {
    const input = [dept('a', '業務部'), dept('b', '業務一課', 'a')]
    buildTree(input)

    // 原始物件不該被塞進 children
    expect(input[0]).not.toHaveProperty('children')
  })
})

describe('descendantIds', () => {
  const tree = buildTree([
    dept('a', '總經理室'),
    dept('b', '業務部', 'a'),
    dept('c', '業務一課', 'b'),
    dept('d', '財務部', 'a'),
  ])

  it('含自己與所有下層', () => {
    const ids = descendantIds(tree, 'b')
    expect(ids.sort()).toEqual(['b', 'c'])
  })

  it('葉節點只回自己', () => {
    expect(descendantIds(tree, 'c')).toEqual(['c'])
  })

  it('整棵樹', () => {
    expect(descendantIds(tree, 'a').sort()).toEqual(['a', 'b', 'c', 'd'])
  })

  it('找不到的 id 回它自己，不回空陣列', () => {
    // 回空陣列會讓呼叫端把「找不到部門」誤判為「這個部門沒有人」
    expect(descendantIds(tree, 'ghost')).toEqual(['ghost'])
  })
})
