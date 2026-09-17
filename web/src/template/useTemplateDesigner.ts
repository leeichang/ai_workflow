/**
 * 單據套版設計器狀態
 *
 * 編輯的是 pdfme 模板（schema 陣列），不是表單定義。
 * 兩者都叫「設計器」但資料結構完全不同：表單是欄位清單，
 * 套版是帶座標的版面元素。
 *
 * 座標單位一律 mm，與 pdfme 一致。轉成畫面像素只在渲染時做，
 * 狀態裡不存像素——否則縮放比例一改，所有座標都得跟著換算。
 */

import { computed, ref, shallowRef } from 'vue'
import { findField, type DocField } from './fields'

/** A4 直式，與 pdfme 的 basePdf 對應 */
export const PAGE = { width: 210, height: 297, padding: [15, 15, 15, 15] as const }

export interface TemplateElement {
  /** 對應 pdfme schema 的 name。綁定欄位時等於欄位 path。 */
  name: string
  type: string
  position: { x: number; y: number }
  width: number
  height: number
  /** 靜態文字。綁定欄位的元素此值為空，由資料填入。 */
  content?: string
  fontName?: string
  fontSize?: number
  alignment?: 'left' | 'center' | 'right'
  fontColor?: string
  /** 表格專用 */
  head?: string[]
  headWidthPercentages?: number[]
  columnAlignment?: Record<number, 'left' | 'center' | 'right'>
  showHead?: boolean
  repeatHead?: boolean
  summaryRowCount?: number
  alternateBackground?: boolean
  /** 是否綁定資料欄位。false 為純版面元素。 */
  bound?: boolean
}

const MAX_HISTORY = 50

export function useTemplateDesigner() {
  const elements = ref<TemplateElement[]>([])
  const selectedName = ref<string | null>(null)

  const past = shallowRef<string[]>([])
  const future = shallowRef<string[]>([])
  const dirty = ref(false)

  const canUndo = computed(() => past.value.length > 0)
  const canRedo = computed(() => future.value.length > 0)

  const selected = computed(
    () => elements.value.find((e) => e.name === selectedName.value) ?? null,
  )

  function snapshot() {
    past.value = [...past.value, JSON.stringify(elements.value)].slice(-MAX_HISTORY)
    future.value = []
    dirty.value = true
  }

  function undo() {
    if (!canUndo.value) return
    const previous = past.value[past.value.length - 1]
    future.value = [JSON.stringify(elements.value), ...future.value]
    past.value = past.value.slice(0, -1)
    elements.value = JSON.parse(previous)
    dirty.value = true
  }

  function redo() {
    if (!canRedo.value) return
    const next = future.value[0]
    past.value = [...past.value, JSON.stringify(elements.value)]
    future.value = future.value.slice(1)
    elements.value = JSON.parse(next)
    dirty.value = true
  }

  /**
   * 產生不重複的元素名稱
   *
   * pdfme 以 name 對應輸入資料，重複會讓後者覆蓋前者。
   * 同一個欄位拖兩次時加序號。
   */
  function uniqueName(base: string): string {
    const taken = new Set(elements.value.map((e) => e.name))
    if (!taken.has(base)) return base

    let i = 2
    while (taken.has(`${base}_${i}`)) i += 1
    return `${base}_${i}`
  }

  /** 把欄位拖進版面 */
  function addField(field: DocField, at: { x: number; y: number }) {
    snapshot()

    const name = uniqueName(field.path)
    const element: TemplateElement =
      field.kind === 'table'
        ? {
            name,
            type: 'table',
            position: snapToPage(at),
            width: PAGE.width - PAGE.padding[1] - PAGE.padding[3],
            height: 40,
            showHead: true,
            repeatHead: true,
            // 合計三列（小計、稅額、總計）預設帶入，
            // 與渲染服務的 buildSummaryRows 一致。
            summaryRowCount: 3,
            alternateBackground: true,
            head: ['品名與規格', '數量', '單位', '單價(NT$)', '金額(NT$)'],
            headWidthPercentages: [46, 11, 9, 16, 18],
            columnAlignment: { 0: 'left', 1: 'right', 2: 'center', 3: 'right', 4: 'right' },
            bound: true,
          }
        : {
            name,
            type: field.kind === 'image' ? 'image' : 'text',
            position: snapToPage(at),
            width: 60,
            height: 8,
            fontSize: 9,
            alignment: field.kind === 'number' || field.kind === 'percent' ? 'right' : 'left',
            bound: true,
          }

    elements.value = [...elements.value, element]
    selectedName.value = name
  }

  /** 加入純版面元素（文字、線條等） */
  function addElement(type: string, at: { x: number; y: number }) {
    snapshot()

    const name = uniqueName(type)
    elements.value = [
      ...elements.value,
      {
        name,
        type,
        position: snapToPage(at),
        width: type === 'line' ? 60 : 40,
        height: type === 'line' ? 0.4 : 8,
        content: type === 'text' ? '文字' : '',
        fontSize: 9,
        bound: false,
      },
    ]
    selectedName.value = name
  }

  function update(name: string, patch: Partial<TemplateElement>) {
    snapshot()
    elements.value = elements.value.map((e) =>
      e.name === name ? { ...e, ...patch, position: { ...e.position, ...patch.position } } : e,
    )
  }

  /** 移動元素。dx/dy 為 mm。 */
  function move(name: string, dx: number, dy: number) {
    const element = elements.value.find((e) => e.name === name)
    if (!element) return

    update(name, {
      position: snapToPage({ x: element.position.x + dx, y: element.position.y + dy }),
    })
  }

  function remove(name: string) {
    snapshot()
    elements.value = elements.value.filter((e) => e.name !== name)
    if (selectedName.value === name) selectedName.value = null
  }

  function load(list: TemplateElement[]) {
    elements.value = list
    past.value = []
    future.value = []
    dirty.value = false
    selectedName.value = null
  }

  return {
    elements,
    selectedName,
    selected,
    dirty,
    canUndo,
    canRedo,
    undo,
    redo,
    addField,
    addElement,
    update,
    move,
    remove,
    load,
  }
}

/**
 * 限制座標在頁面內
 *
 * 超出上邊距會讓 pdfme 算出負的頁索引並崩潰
 * （見 pdfme/src/template/quotation.ts 的說明），
 * 因此在設計器就擋住，不讓使用者拖到那個位置。
 */
export function snapToPage(at: { x: number; y: number }): { x: number; y: number } {
  const [top, right, bottom, left] = PAGE.padding
  return {
    x: clamp(round1(at.x), left, PAGE.width - right),
    y: clamp(round1(at.y), top, PAGE.height - bottom),
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max)
}

/** 座標取到 0.1mm。再細使用者感覺不出來，但會讓數值欄位很難讀。 */
function round1(value: number): number {
  return Math.round(value * 10) / 10
}

/** 由模板元素反查欄位定義，供屬性面板顯示資料來源 */
export function fieldOf(element: TemplateElement | null): DocField | undefined {
  if (!element?.bound) return undefined
  // 名稱可能帶序號，取回原始 path
  return findField(element.name.replace(/_\d+$/, ''))
}
