/**
 * 單據套版設計器狀態測試
 *
 * 座標邊界是重點。超出上邊距會讓 pdfme 算出負的頁索引，
 * 崩在 `pages[currentPageIndex].push`，錯誤訊息完全看不出與座標有關
 * （見 pdfme/src/template/quotation.ts）。設計器就該擋住。
 */

import { describe, expect, it } from 'vitest'
import {
  PAGE,
  fieldOf,
  snapToPage,
  useTemplateDesigner,
} from '@/template/useTemplateDesigner'
import { ITEMS_FIELD, findField } from '@/template/fields'

const customerName = findField('customer_name')!

describe('座標限制', () => {
  it('限制在頁面邊距內', () => {
    expect(snapToPage({ x: 0, y: 0 })).toEqual({ x: 15, y: 15 })
  })

  it('擋住超出下緣的座標', () => {
    const result = snapToPage({ x: 999, y: 999 })
    expect(result.x).toBe(PAGE.width - PAGE.padding[1])
    expect(result.y).toBe(PAGE.height - PAGE.padding[2])
  })

  it('負座標被拉回邊距', () => {
    // y 小於上邊距會讓 pdfme 崩潰，不能放行
    expect(snapToPage({ x: -50, y: -50 })).toEqual({ x: 15, y: 15 })
  })

  it('取到 0.1mm', () => {
    expect(snapToPage({ x: 50.06, y: 60.04 })).toEqual({ x: 50.1, y: 60 })
  })
})

describe('加入欄位', () => {
  it('一般欄位成為 text 元素', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })

    expect(d.elements.value).toHaveLength(1)
    expect(d.elements.value[0].type).toBe('text')
    expect(d.elements.value[0].name).toBe('customer_name')
    expect(d.elements.value[0].bound).toBe(true)
  })

  it('明細表成為 table 元素並帶預設欄位', () => {
    const d = useTemplateDesigner()
    d.addField(ITEMS_FIELD, { x: 15, y: 96 })

    const table = d.elements.value[0]
    expect(table.type).toBe('table')
    expect(table.head).toHaveLength(5)
    expect(table.headWidthPercentages).toHaveLength(5)
  })

  it('明細表預設帶三列合計', () => {
    // 與渲染服務的 buildSummaryRows 一致。不一致的話設計時看到三列、
    // 印出來變成別的數量。
    const d = useTemplateDesigner()
    d.addField(ITEMS_FIELD, { x: 15, y: 96 })

    expect(d.elements.value[0].summaryRowCount).toBe(3)
  })

  it('金額類欄位預設靠右', () => {
    const d = useTemplateDesigner()
    d.addField(findField('subtotal')!, { x: 20, y: 60 })

    expect(d.elements.value[0].alignment).toBe('right')
  })

  it('加入後自動選取', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })

    expect(d.selectedName.value).toBe('customer_name')
    expect(d.selected.value?.name).toBe('customer_name')
  })

  it('同一欄位拖兩次時名稱加序號', () => {
    // pdfme 以 name 對應輸入資料，重複會讓後者覆蓋前者
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })
    d.addField(customerName, { x: 20, y: 80 })

    const names = d.elements.value.map((e) => e.name)
    expect(names).toEqual(['customer_name', 'customer_name_2'])
    expect(new Set(names).size).toBe(2)
  })

  it('拖到頁面外時座標被修正', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 0, y: 0 })

    expect(d.elements.value[0].position).toEqual({ x: 15, y: 15 })
  })
})

describe('版面元素', () => {
  it('線條高度極細', () => {
    const d = useTemplateDesigner()
    d.addElement('line', { x: 15, y: 50 })

    expect(d.elements.value[0].height).toBeLessThan(1)
  })

  it('版面元素不綁欄位', () => {
    const d = useTemplateDesigner()
    d.addElement('text', { x: 20, y: 30 })

    expect(d.elements.value[0].bound).toBe(false)
    expect(fieldOf(d.elements.value[0])).toBeUndefined()
  })
})

describe('移動', () => {
  it('依位移調整座標', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 50, y: 60 })
    d.move('customer_name', 10, -5)

    expect(d.elements.value[0].position).toEqual({ x: 60, y: 55 })
  })

  it('移出頁面時被擋住', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 20 })
    d.move('customer_name', -100, -100)

    expect(d.elements.value[0].position).toEqual({ x: 15, y: 15 })
  })

  it('移動不存在的元素不崩潰', () => {
    const d = useTemplateDesigner()
    expect(() => d.move('nope', 10, 10)).not.toThrow()
  })
})

describe('修改與刪除', () => {
  it('部分更新不洗掉其他屬性', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })
    d.update('customer_name', { fontSize: 14 })

    const e = d.elements.value[0]
    expect(e.fontSize).toBe(14)
    expect(e.width, 'width 不該被洗掉').toBe(60)
    expect(e.position.x).toBe(20)
  })

  it('只改 position 的一個軸', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })
    d.update('customer_name', { position: { x: 30 } as never })

    expect(d.elements.value[0].position).toEqual({ x: 30, y: 60 })
  })

  it('刪除後清除選取', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })
    d.remove('customer_name')

    expect(d.elements.value).toHaveLength(0)
    expect(d.selectedName.value).toBeNull()
  })
})

describe('復原與重做', () => {
  it('復原還原加入前的狀態', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })
    expect(d.elements.value).toHaveLength(1)

    d.undo()
    expect(d.elements.value).toHaveLength(0)
    expect(d.canRedo.value).toBe(true)
  })

  it('重做再套用一次', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })
    d.undo()
    d.redo()

    expect(d.elements.value).toHaveLength(1)
  })

  it('新操作清除重做堆疊', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })
    d.undo()
    d.addElement('text', { x: 30, y: 30 })

    expect(d.canRedo.value, '分支後舊的重做路徑已無意義').toBe(false)
  })

  it('載入會清空歷史', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })
    d.load([])

    expect(d.canUndo.value).toBe(false)
    expect(d.dirty.value).toBe(false)
  })
})

describe('反查欄位定義', () => {
  it('帶序號的名稱仍查得到原欄位', () => {
    const d = useTemplateDesigner()
    d.addField(customerName, { x: 20, y: 60 })
    d.addField(customerName, { x: 20, y: 80 })

    expect(fieldOf(d.elements.value[1])?.label).toBe('客戶名稱')
  })
})
