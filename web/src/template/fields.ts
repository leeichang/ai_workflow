/**
 * 單據可用欄位
 *
 * 依設計稿 _9 左側「可用欄位」樹。分組依據是使用者的心智模型
 * （這個值從哪來），不是技術上的資料表。
 *
 * path 對應 pdfme 模板的 schema name，也是渲染服務 buildInputs()
 * 產生的鍵。兩端必須一致，否則拖進版面的欄位印出來是空的。
 */

export type FieldKind =
  | 'text'
  | 'date'
  | 'number'
  | 'percent'
  | 'currency'
  | 'taxid'
  | 'address'
  | 'phone'
  | 'person'
  | 'image'
  | 'table'

export interface DocField {
  /** 對應模板 schema 的 name */
  path: string
  label: string
  kind: FieldKind
  /** 顯示於標籤下方的資料路徑，讓使用者知道值從哪來 */
  source: string
}

export interface FieldGroup {
  key: string
  title: string
  fields: DocField[]
}

/** 型別標籤。設計稿在每個欄位右側顯示。 */
export const KIND_LABEL: Record<FieldKind, string> = {
  text: '文字',
  date: '日期',
  number: '數值',
  percent: '百分比',
  currency: '幣別',
  taxid: '統編',
  address: '地址',
  phone: '電話',
  person: '姓名',
  image: '圖片',
  table: '表格',
}

/**
 * 明細表是特殊欄位
 *
 * 其他欄位拖進去是單一值，明細表拖進去會展開成多欄的動態表格，
 * 且欄位組成可再設定。因此在 UI 上獨立成一區，不混在一般欄位裡。
 */
export const ITEMS_FIELD: DocField = {
  path: 'items',
  label: '報價明細表',
  kind: 'table',
  source: 'quotation.items[]',
}

export const FIELD_GROUPS: FieldGroup[] = [
  {
    key: 'quotation',
    title: '報價單表頭',
    fields: [
      { path: 'doc_meta', label: '報價單號', kind: 'text', source: 'quotation.id' },
      { path: 'quotation_date', label: '報價日期', kind: 'date', source: 'quotation.date' },
      { path: 'valid_until', label: '有效期限', kind: 'date', source: 'quotation.valid_until' },
      { path: 'currency', label: '幣別', kind: 'currency', source: 'quotation.currency' },
      {
        path: 'payment_terms',
        label: '付款條件',
        kind: 'text',
        source: 'quotation.payment_terms',
      },
    ],
  },
  {
    key: 'customer',
    title: '客戶資訊',
    fields: [
      { path: 'customer_name', label: '客戶名稱', kind: 'text', source: 'customer.name' },
      { path: 'customer_tax_id', label: '統一編號', kind: 'taxid', source: 'customer.tax_id' },
      {
        path: 'customer_address',
        label: '通訊地址',
        kind: 'address',
        source: 'customer.address',
      },
      {
        path: 'customer_contact',
        label: '聯絡人 / 職稱',
        kind: 'person',
        source: 'customer.contact',
      },
      { path: 'customer_phone', label: '聯絡電話', kind: 'phone', source: 'customer.phone' },
    ],
  },
  {
    key: 'amount',
    title: '金額與稅率',
    fields: [
      { path: 'subtotal', label: '小計金額', kind: 'number', source: 'quotation.subtotal' },
      { path: 'tax', label: '營業稅額 (5%)', kind: 'number', source: 'quotation.tax' },
      { path: 'total', label: '總計合計', kind: 'number', source: 'quotation.total' },
      {
        path: 'discount_rate',
        label: '表頭折扣率',
        kind: 'percent',
        source: 'quotation.discount_rate',
      },
    ],
  },
  {
    key: 'company',
    title: '公司資訊',
    fields: [
      { path: 'company_name', label: '公司全名', kind: 'text', source: 'company.name' },
      { path: 'company_tax_id', label: '公司統編', kind: 'taxid', source: 'company.tax_id' },
      { path: 'company_info', label: '公司廠址', kind: 'address', source: 'company.address' },
      { path: 'company_logo', label: '公司標誌 Logo', kind: 'image', source: 'company.logo' },
    ],
  },
]

/** 版面元素。與資料無關，純粹是排版用的圖形。 */
export interface ElementSpec {
  type: 'text' | 'image' | 'line' | 'rectangle' | 'qrcode'
  label: string
  icon: string
}

export const ELEMENTS: ElementSpec[] = [
  { type: 'text', label: '文字', icon: 'title' },
  { type: 'image', label: '圖片', icon: 'image' },
  { type: 'line', label: '線條', icon: 'horizontal_rule' },
  { type: 'rectangle', label: '矩形', icon: 'crop_square' },
  { type: 'qrcode', label: 'QR Code', icon: 'qr_code_2' },
]

const ALL_FIELDS = [ITEMS_FIELD, ...FIELD_GROUPS.flatMap((g) => g.fields)]

export function findField(path: string): DocField | undefined {
  return ALL_FIELDS.find((f) => f.path === path)
}

/** 依關鍵字過濾。比對標籤與資料路徑，讓使用者用任一種都找得到。 */
export function filterGroups(keyword: string): FieldGroup[] {
  const q = keyword.trim().toLowerCase()
  if (!q) return FIELD_GROUPS

  return FIELD_GROUPS.map((g) => ({
    ...g,
    fields: g.fields.filter(
      (f) => f.label.toLowerCase().includes(q) || f.source.toLowerCase().includes(q),
    ),
  })).filter((g) => g.fields.length > 0)
}
