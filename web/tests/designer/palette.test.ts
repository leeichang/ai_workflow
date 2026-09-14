/**
 * 元件庫測試
 */

import { describe, expect, it } from 'vitest'
import { filterPalette, nextFieldKey, PALETTE, PALETTE_COUNT } from '@/designer/palette'

describe('元件庫', () => {
  it('共 17 種元件，分三組', () => {
    // 設計稿 _5 標示「共 17 種」
    expect(PALETTE_COUNT).toBe(17)
    expect(PALETTE.map((g) => g.key)).toEqual(['basic', 'advanced', 'layout'])
  })

  it('每種元件都能產生合法的欄位骨架', () => {
    for (const group of PALETTE) {
      for (const item of group.items) {
        const f = item.defaults()
        expect(f.key, `${item.label} 缺 key`).toBeTruthy()
        expect(f.ui?.component, `${item.label} 缺 component`).toBeTruthy()
        expect(f.data, `${item.label} 缺 data`).toBeDefined()
        // label 允許為空字串：分隔線本來就沒有文字，
        // 但必須有定義，否則渲染時會是 undefined。
        expect(f.ui?.label, `${item.label} 缺 label 定義`).toBeDefined()
      }
    }
  })

  it('版面元素以 help 欄位標記，畫布依此改變渲染方式', () => {
    const layout = PALETTE.find((g) => g.key === 'layout')!
    const markers = layout.items.map((i) => i.defaults().ui?.help)
    expect(markers).toEqual([
      '__layout_heading__',
      '__layout_divider__',
      '__layout_note__',
    ])
  })

  it('明細表格預設帶三個欄位', () => {
    const table = PALETTE.find((g) => g.key === 'advanced')!
      .items.find((i) => i.component === 'table')!
    const f = table.defaults()
    // 拖入後立刻看得到結構，不必先去設定欄位
    expect(f.ui?.columns).toHaveLength(3)
    expect(f.data?.type).toBe('array')
  })

  it('數字元件預設小數兩位', () => {
    const num = PALETTE[0].items.find((i) => i.component === 'number')!
    expect(num.defaults().ui?.precision).toBe(2)
    expect(num.defaults().data?.type).toBe('decimal')
  })

  it('產生的 key 不重複', () => {
    const input = PALETTE[0].items[0]
    const keys = new Set(Array.from({ length: 20 }, () => input.defaults().key))
    expect(keys.size).toBe(20)
  })

  it('nextFieldKey 帶前綴', () => {
    expect(nextFieldKey('number')).toMatch(/^number_\d+$/)
  })
})

describe('搜尋過濾', () => {
  it('空字串回傳全部', () => {
    expect(filterPalette('')).toEqual(PALETTE)
    expect(filterPalette('   ')).toEqual(PALETTE)
  })

  it('依標籤過濾', () => {
    const r = filterPalette('表格')
    const labels = r.flatMap((g) => g.items.map((i) => i.label))
    expect(labels).toContain('明細表格 (Sub-table)')
    expect(labels).not.toContain('單行文字')
  })

  it('依元件名稱過濾', () => {
    const r = filterPalette('number')
    expect(r.flatMap((g) => g.items).some((i) => i.component === 'number')).toBe(true)
  })

  it('無結果時不回傳空分組', () => {
    const r = filterPalette('不存在的元件')
    expect(r).toHaveLength(0)
  })
})
