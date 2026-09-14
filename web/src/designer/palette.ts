/**
 * 元件庫定義
 *
 * 依設計稿 docs/UI 設計/.../_5 的三個分組、17 種元件。
 * 每種元件拖入畫布時產生一個 FormField 骨架。
 */

import type { FieldComponent, FormField } from '@/api/types'

export interface PaletteItem {
  component: FieldComponent
  label: string
  icon: string
  /** 拖入時的預設欄位設定 */
  defaults: () => Partial<FormField>
}

export interface PaletteGroup {
  key: string
  title: string
  items: PaletteItem[]
}

let counter = 0

/** 產生不重複的欄位 key。使用者可在屬性面板改。 */
export function nextFieldKey(prefix: string): string {
  counter += 1
  return `${prefix}_${counter}`
}

function basic(
  component: FieldComponent,
  label: string,
  icon: string,
  extra: Partial<FormField> = {},
): PaletteItem {
  return {
    component,
    label,
    icon,
    defaults: () => ({
      key: nextFieldKey(component),
      ui: { component, label, width: 6 },
      data: { path: '', type: 'string' },
      ...extra,
    }),
  }
}

export const PALETTE: PaletteGroup[] = [
  {
    key: 'basic',
    title: '基本欄位',
    items: [
      basic('input', '單行文字', 'text_fields'),
      basic('textarea', '多行文字', 'subject', {
        ui: { component: 'textarea', label: '多行文字', width: 12 },
        data: { path: '', type: 'text' },
      }),
      basic('number', '數字', 'tag', {
        ui: { component: 'number', label: '數字', width: 4, precision: 2 },
        data: { path: '', type: 'decimal' },
      }),
      basic('date', '日期', 'calendar_today', {
        ui: { component: 'date', label: '日期', width: 4 },
        data: { path: '', type: 'date' },
      }),
      basic('select', '下拉選單', 'arrow_drop_down_circle', {
        ui: { component: 'select', label: '下拉選單', width: 4, options: [] },
      }),
      basic('multi_select', '多選', 'checklist', {
        ui: { component: 'multi_select', label: '多選', width: 6, options: [] },
        data: { path: '', type: 'array' },
      }),
      basic('checkbox', '核取方塊', 'check_box', {
        ui: { component: 'checkbox', label: '核取方塊', width: 4 },
        data: { path: '', type: 'boolean' },
      }),
      basic('switch', '開關', 'toggle_on', {
        ui: { component: 'switch', label: '開關', width: 4 },
        data: { path: '', type: 'boolean' },
      }),
    ],
  },
  {
    key: 'advanced',
    title: '進階欄位',
    items: [
      {
        component: 'table',
        label: '明細表格 (Sub-table)',
        icon: 'table_chart',
        defaults: () => ({
          key: nextFieldKey('table'),
          ui: {
            component: 'table',
            label: '明細表格',
            width: 12,
            // 預設三欄，讓使用者拖入後立刻看得到結構
            columns: [
              { key: 'name', label: '品名', component: 'input', width: 'auto' },
              { key: 'qty', label: '數量', component: 'number', width: '90px', align: 'right', precision: 0 },
              { key: 'amount', label: '金額', component: 'number', width: '120px', align: 'right', precision: 2 },
            ],
          },
          data: { path: '', type: 'array' },
        }),
      },
      basic('reference', '關聯查詢', 'dataset_linked', {
        ui: {
          component: 'reference',
          label: '關聯查詢',
          width: 6,
          reference: { source: '', value_field: 'id', label_field: 'name' },
        },
        data: { path: '', type: 'uuid' },
      }),
      basic('user_picker', '人員選擇', 'person_search', {
        ui: { component: 'user_picker', label: '人員選擇', width: 6 },
        data: { path: '', type: 'uuid' },
      }),
      basic('department_picker', '部門選擇', 'corporate_fare', {
        ui: { component: 'department_picker', label: '部門選擇', width: 6 },
        data: { path: '', type: 'uuid' },
      }),
      basic('upload', '檔案上傳', 'attach_file', {
        ui: { component: 'upload', label: '檔案上傳', width: 12 },
        data: { path: '', type: 'array', item: 'file' },
      }),
      basic('display', '唯讀顯示 (Read-only)', 'visibility_off', {
        ui: { component: 'display', label: '唯讀顯示', width: 4 },
      }),
    ],
  },
  {
    key: 'layout',
    title: '版面配置',
    items: [
      // 版面元素也是 field，但不綁資料。用 display 元件加特殊標記。
      {
        component: 'display',
        label: '區塊標題',
        icon: 'title',
        defaults: () => ({
          key: nextFieldKey('heading'),
          ui: { component: 'display', label: '區塊標題', width: 12, help: '__layout_heading__' },
          data: { path: '', type: 'string' },
        }),
      },
      {
        component: 'display',
        label: '分隔線',
        icon: 'horizontal_rule',
        defaults: () => ({
          key: nextFieldKey('divider'),
          ui: { component: 'display', label: '', width: 12, help: '__layout_divider__' },
          data: { path: '', type: 'string' },
        }),
      },
      {
        component: 'display',
        label: '說明文字',
        icon: 'info',
        defaults: () => ({
          key: nextFieldKey('note'),
          ui: { component: 'display', label: '說明文字', width: 12, help: '__layout_note__' },
          data: { path: '', type: 'string' },
        }),
      },
    ],
  },
]

export const PALETTE_COUNT = PALETTE.reduce((n, g) => n + g.items.length, 0)

/** 依搜尋字串過濾元件 */
export function filterPalette(query: string): PaletteGroup[] {
  const q = query.trim().toLowerCase()
  if (!q) return PALETTE

  return PALETTE.map((g) => ({
    ...g,
    items: g.items.filter(
      (i) => i.label.toLowerCase().includes(q) || i.component.includes(q),
    ),
  })).filter((g) => g.items.length > 0)
}
