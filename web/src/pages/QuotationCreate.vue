<script setup lang="ts">
/**
 * 建立報價單
 *
 * 表單由表單設計器的定義驅動渲染，而非寫死欄位。設計器改了欄位、
 * 標籤或驗證規則，這個畫面跟著變——兩份清單各自維護遲早會不一致。
 *
 * 送出時依每個欄位的 data.path 組出巢狀的業務物件。這一點很關鍵：
 * 流程定義的條件式讀 `quotation.discount_rate`，客戶簽核的
 * resolver 讀 `quotation.customer_contact_ids`。放錯層級的話，
 * 條件式取不到值會靜默當成 false（少走一段簽核，沒有錯誤），
 * resolver 取不到則讓整個流程 FAILED。
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import * as formsApi from '@/api/forms'
import * as instancesApi from '@/api/instances'
import { ApiError } from '@/api/types'

const router = useRouter()

/** 報價單走的流程。目前一個業務物件只對應一條流程。 */
const WORKFLOW_KEY = 'quotation_approval'

interface LineColumn {
  key: string
  label: string
  component?: string
  align?: string
  width?: string
  precision?: number
}

interface FormField {
  key: string
  section?: string
  data?: {
    path?: string
    type?: string
    pattern?: string
    min?: number
    max?: number
    /** display 欄位的計算式，僅作為文件——實際計算在 computed 區 */
    computed?: string
  }
  ui?: {
    component?: string
    label?: string
    placeholder?: string
    width?: number
    suffix?: string
    precision?: number
    columns?: LineColumn[]
  }
  workflow?: { required_when?: string }
}

interface FormSection {
  key: string
  title: string
}

const sections = ref<FormSection[]>([])
const fields = ref<FormField[]>([])
const loading = ref(true)
const submitting = ref(false)
const errorMessage = ref('')

/**
 * 使用者輸入，key 是欄位 key
 *
 * 型別是 string | number 而非單純 string：`type="number"` 的 input
 * 搭 v-model 時，Vue 會自動把值轉成數字。假設它一定是字串而直接
 * 呼叫 .trim() 會拋 TypeError，而且是在驗證階段拋——
 * 例外往上冒，送出無聲中止，畫面上什麼都不會顯示。
 */
const values = ref<Record<string, string | number>>({})

/** 統一取出字串形式，供驗證與送出使用 */
function rawValue(key: string): string {
  const v = values.value[key]
  if (v === undefined || v === null) return ''
  return String(v).trim()
}

const businessKey = ref('')

/**
 * 客戶聯絡人
 *
 * 表單定義裡沒有這個欄位，但流程的客戶簽核節點需要它
 * （resolver path = quotation.customer_contact_ids）。
 * 缺了的話流程會走到客戶簽核才失敗，那時已經簽核過兩關了。
 *
 * 正確的解法是把它加進表單定義，但那會動到已發布的表單版本。
 * 先在這裡補上並標為必填，之後併入表單定義時移除。
 */
const customerContacts = ref('')

/**
 * 一般輸入欄位
 *
 * display 由系統計算、table 有自己的編輯介面，兩者都不走
 * 通用的 input 渲染，但**都要參與送出**——先前把 table 一併
 * 排除，結果是 lines 這個必填欄位在畫面與驗證裡同時消失，
 * 可以送出沒有明細的報價單。
 */
const NON_INPUT_COMPONENTS = new Set(['display', 'table'])

const editableFields = computed(() =>
  fields.value.filter(
    (f) => !NON_INPUT_COMPONENTS.has(f.ui?.component ?? 'input'),
  ),
)

/** 明細表格欄位定義。目前一張表單只有一個 table。 */
const linesField = computed(() =>
  fields.value.find((f) => f.ui?.component === 'table'),
)

const lineColumns = computed<LineColumn[]>(
  () => linesField.value?.ui?.columns ?? [],
)

/** 顯示用的計算欄位（毛利率、總計） */
const displayFields = computed(() =>
  fields.value.filter((f) => f.ui?.component === 'display'),
)

// ── 明細列 ──────────────────────────────────────────────

type LineRow = Record<string, string | number>

/** 一列明細。預設給一列，不必先按新增才能開始填。 */
const lines = ref<LineRow[]>([{}])

function addLine(): void {
  lines.value = [...lines.value, {}]
}

/**
 * 刪除指定列
 *
 * 至少保留一列：全刪光的話畫面上只剩一個「新增」按鈕，
 * 使用者得先按一下才能填，多一道無意義的手續。
 */
function removeLine(index: number): void {
  if (lines.value.length <= 1) return
  lines.value = lines.value.filter((_, i) => i !== index)
}

/** 一列是否完全空白 */
function isBlankLine(row: LineRow): boolean {
  return Object.values(row).every(
    (v) => v === undefined || v === null || String(v).trim() === '',
  )
}

const filledLines = computed(() =>
  lines.value.filter((row) => !isBlankLine(row)),
)

function num(v: string | number | undefined): number {
  if (v === undefined || v === null || String(v).trim() === '') return 0
  const n = Number(v)
  return Number.isNaN(n) ? 0 : n
}

// ── 金額計算 ────────────────────────────────────────────
//
// 表單定義的 data.computed 只寫了
//   gross_margin = (subtotal - total_cost) / subtotal
//   total_amount = subtotal + tax
// 但 subtotal / total_cost / tax 本身沒有定義在任何地方——
// 後端與 PDF 都只接收算好的值。這裡是唯一的實作處。

/** 營業稅率。台灣標準稅率 5%，之後應改為租戶設定。 */
const TAX_RATE = 0.05

const subtotal = computed(() =>
  filledLines.value.reduce(
    (sum, row) => sum + num(row.qty) * num(row.target_price),
    0,
  ),
)

/** 成本 = 各列（單位材料費 + 單位加工費）× 數量 */
const totalCost = computed(() =>
  filledLines.value.reduce(
    (sum, row) =>
      sum + (num(row.material_cost) + num(row.process_cost)) * num(row.qty),
    0,
  ),
)

const tax = computed(() => subtotal.value * TAX_RATE)

const totalAmount = computed(() => subtotal.value + tax.value)

/**
 * 毛利率
 *
 * 小計為 0 時除法會得到 NaN 或 Infinity，畫面上直接顯示那個字
 * 對使用者毫無意義，回傳 0。
 */
const grossMargin = computed(() => {
  if (subtotal.value === 0) return 0
  return (subtotal.value - totalCost.value) / subtotal.value
})

/** 千分位。金額欄位不顯示小數，比例另外處理。 */
function money(v: number): string {
  return v.toLocaleString('en-US', {
    minimumFractionDigits: 0,
    maximumFractionDigits: 2,
  })
}

/** display 欄位的顯示值 */
function displayValue(field: FormField): string {
  if (field.key === 'gross_margin') {
    return (grossMargin.value * 100).toFixed(field.ui?.precision ?? 2)
  }
  if (field.key === 'total_amount') {
    return money(totalAmount.value)
  }
  return '—'
}

/** 依 section 分組，維持定義中的順序 */
const grouped = computed(() =>
  sections.value
    .map((s) => ({
      section: s,
      fields: editableFields.value.filter((f) => f.section === s.key),
    }))
    .filter((g) => g.fields.length > 0),
)

/** 沒有 section 或 section 不在清單裡的欄位 */
const ungrouped = computed(() => {
  const known = new Set(sections.value.map((s) => s.key))
  return editableFields.value.filter(
    (f) => f.section === undefined || !known.has(f.section),
  )
})

async function load(): Promise<void> {
  loading.value = true
  errorMessage.value = ''
  try {
    const def = await formsApi.getForm('quotation_form')
    // 優先用已發布版本；還沒發布時用草稿，讓設計中的表單也能試填
    const version = def.published ?? def.draft
    if (version === null || version === undefined) {
      errorMessage.value = '表單尚未建立任何版本'
      return
    }
    sections.value = version.content.sections ?? []
    fields.value = version.content.fields ?? []
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法載入表單定義'
  } finally {
    loading.value = false
  }
}

/**
 * 依 path 寫入巢狀物件
 *
 * 'customer.name' → { customer: { name: ... } }
 */
function setByPath(
  target: Record<string, unknown>,
  path: string,
  value: unknown,
): void {
  const parts = path.split('.')
  let node = target
  for (const part of parts.slice(0, -1)) {
    if (typeof node[part] !== 'object' || node[part] === null) {
      node[part] = {}
    }
    node = node[part] as Record<string, unknown>
  }
  node[parts[parts.length - 1]] = value
}

/**
 * 依欄位型別轉換
 *
 * 數值必須送數字而非字串。流程的條件式會拿它做大小比較
 * （`quotation.discount_rate > 0.15`），字串比較的結果不可預期。
 *
 * 表單定義用的是 decimal／integer／number 三種寫法，
 * 只認其中一種的話另外兩種會靜默送出字串。
 */
const NUMERIC_TYPES = new Set(['number', 'decimal', 'integer', 'int', 'float'])

function coerce(field: FormField, raw: string): unknown {
  if (NUMERIC_TYPES.has(field.data?.type ?? '')) {
    const n = Number(raw)
    return Number.isNaN(n) ? raw : n
  }
  return raw
}

/**
 * 驗證
 *
 * 回傳第一個錯誤訊息，沒有錯誤時回傳空字串。
 * 只擋明確違規的，其餘交給後端——前端的驗證是為了少跑一趟，
 * 不是權威。
 */
function validate(): string {
  if (businessKey.value.trim() === '') {
    return '請填寫報價單號'
  }

  for (const field of editableFields.value) {
    const label = field.ui?.label ?? field.key
    const raw = rawValue(field.key)

    // required_when 是表達式，目前只處理常數 'true'。
    // 完整的條件式求值需要 node 上下文，建立時還沒有。
    if (field.workflow?.required_when === 'true' && raw === '') {
      return `請填寫${label}`
    }

    if (raw === '') continue

    if (field.data?.pattern !== undefined) {
      if (!new RegExp(field.data.pattern).test(raw)) {
        return `${label}格式不正確`
      }
    }

    if (NUMERIC_TYPES.has(field.data?.type ?? '')) {
      const n = Number(raw)
      if (Number.isNaN(n)) {
        return `${label}必須是數字`
      }
      if (field.data?.min !== undefined && n < field.data.min) {
        return `${label}不得小於 ${field.data.min}`
      }
      if (field.data?.max !== undefined && n > field.data.max) {
        return `${label}不得大於 ${field.data.max}`
      }
    }
  }

  // 明細。lines 的 required_when 是 true，但它不在 editableFields 裡，
  // 上面的迴圈看不到——必須單獨檢查。
  if (linesField.value !== undefined) {
    const required = linesField.value.workflow?.required_when === 'true'
    if (required && filledLines.value.length === 0) {
      return '請至少填寫一列報價明細'
    }

    for (const [i, row] of filledLines.value.entries()) {
      if (num(row.qty) <= 0) {
        return `第 ${i + 1} 列明細的數量必須大於 0`
      }
    }
  }

  if (customerContacts.value.trim() === '') {
    return '請填寫客戶聯絡人，否則流程會在客戶簽核時失敗'
  }

  return ''
}

async function submit(): Promise<void> {
  if (submitting.value) return

  const error = validate()
  if (error !== '') {
    errorMessage.value = error
    return
  }

  errorMessage.value = ''
  submitting.value = true

  try {
    const input: Record<string, unknown> = {}

    for (const field of editableFields.value) {
      const raw = rawValue(field.key)
      if (raw === '') continue
      const path = field.data?.path
      if (path === undefined) continue
      setByPath(input, path, coerce(field, raw))
    }

    // 明細。數值欄位依 column 的 component 轉型——
    // 送字串的話後端與 PDF 都得自己 parse。
    if (linesField.value?.data?.path !== undefined) {
      const rows = filledLines.value.map((row) => {
        const out: Record<string, unknown> = {}
        for (const col of lineColumns.value) {
          const raw = row[col.key]
          if (raw === undefined || String(raw).trim() === '') continue
          out[col.key] =
            col.component === 'number' ? num(raw) : String(raw).trim()
        }
        return out
      })
      setByPath(input, linesField.value.data.path, rows)
    }

    // 計算結果一併送出。後端與 PDF 都沒有這段邏輯，
    // 不送的話單據上的金額會是空的。
    setByPath(input, 'quotation.subtotal', subtotal.value)
    setByPath(input, 'quotation.tax', tax.value)
    setByPath(input, 'quotation.total_amount', totalAmount.value)
    setByPath(input, 'quotation.gross_margin', grossMargin.value)

    // 逗號分隔轉陣列。resolver 型別是 external_contacts，要的是清單。
    setByPath(
      input,
      'quotation.customer_contact_ids',
      customerContacts.value
        .split(',')
        .map((s) => s.trim())
        .filter((s) => s !== ''),
    )

    await instancesApi.start({
      workflow_key: WORKFLOW_KEY,
      business_key: businessKey.value.trim(),
      input,
    })

    await router.push('/quotations')
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '建立失敗，請稍後再試'
  } finally {
    submitting.value = false
  }
}

/** Tailwind 的 grid 需要靜態 class，不能用字串拼接 */
const WIDTH_CLASS: Record<number, string> = {
  4: 'col-span-4',
  6: 'col-span-6',
  8: 'col-span-8',
  12: 'col-span-12',
}

function widthClass(field: FormField): string {
  return WIDTH_CLASS[field.ui?.width ?? 12] ?? 'col-span-12'
}

const FIELD_CLASS =
  'w-full h-9 px-3 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense text-on-surface placeholder:text-outline focus:outline-none focus:border-primary-container transition-all'

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-secondary">報價單</span>
      <span class="mx-1 text-outline">/</span>
      <span class="text-on-surface font-semibold">建立</span>
    </template>

    <div class="max-w-[800px]">
      <div class="mb-space-lg">
        <h1 class="font-title-md text-title-md text-on-surface">建立報價單</h1>
        <p class="font-body-dense text-body-dense text-secondary mt-1">
          送出後會啟動報價單簽核流程
        </p>
      </div>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="create-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <div
        v-if="loading"
        data-testid="create-loading"
        class="py-space-xl text-center font-body-dense text-body-dense text-secondary"
      >
        載入表單定義…
      </div>

      <form v-else-if="fields.length > 0" @submit.prevent="submit">
        <!-- 單號不在表單定義裡：它是流程實例的識別，不是業務欄位 -->
        <div
          class="bg-surface-container-lowest border border-outline-variant rounded-xl p-space-lg mb-space-md"
        >
          <label class="block">
            <span class="font-label-header text-label-header text-secondary block mb-1">
              報價單號 <span class="text-error">*</span>
            </span>
            <input
              v-model="businessKey"
              type="text"
              placeholder="例如：QT-2026-0001"
              data-testid="field-business_key"
              :class="FIELD_CLASS"
            />
          </label>
        </div>

        <div
          v-for="group in grouped"
          :key="group.section.key"
          class="bg-surface-container-lowest border border-outline-variant rounded-xl p-space-lg mb-space-md"
        >
          <h2 class="font-title-md text-[15px] text-on-surface mb-space-md">
            {{ group.section.title }}
          </h2>

          <div class="grid grid-cols-12 gap-space-md">
            <label
              v-for="field in group.fields"
              :key="field.key"
              :class="widthClass(field)"
              class="block"
            >
              <span class="font-label-header text-label-header text-secondary block mb-1">
                {{ field.ui?.label ?? field.key }}
                <span
                  v-if="field.workflow?.required_when === 'true'"
                  class="text-error"
                >
                  *
                </span>
              </span>

              <!--
                type 必須是靜態字面值。v-model 搭配動態 :type 時，
                Vue 無法在編譯期決定用哪一種 v-model 實作，
                執行期綁定會失效——欄位看起來能打字，值卻進不到 model。
              -->
              <textarea
                v-if="field.ui?.component === 'textarea'"
                v-model="values[field.key]"
                rows="3"
                :placeholder="field.ui?.placeholder"
                :data-testid="`field-${field.key}`"
                class="w-full px-3 py-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense text-on-surface placeholder:text-outline focus:outline-none focus:border-primary-container transition-all"
              />
              <input
                v-else-if="field.ui?.component === 'number'"
                v-model="values[field.key]"
                type="number"
                step="any"
                :placeholder="field.ui?.placeholder"
                :data-testid="`field-${field.key}`"
                :class="FIELD_CLASS"
              />
              <input
                v-else
                v-model="values[field.key]"
                type="text"
                :placeholder="field.ui?.placeholder"
                :data-testid="`field-${field.key}`"
                :class="FIELD_CLASS"
              />
            </label>
          </div>
        </div>

        <!-- 明細表格。欄位依 ui.columns 產生，不寫死。 -->
        <div
          v-if="linesField !== undefined"
          class="bg-surface-container-lowest border border-outline-variant rounded-xl p-space-lg mb-space-md"
        >
          <div class="flex items-center justify-between mb-space-md">
            <h2 class="font-title-md text-[15px] text-on-surface">
              {{ linesField.ui?.label ?? '明細' }}
              <span
                v-if="linesField.workflow?.required_when === 'true'"
                class="text-error"
              >
                *
              </span>
            </h2>
            <button
              type="button"
              data-testid="lines-add"
              class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
              @click="addLine"
            >
              <span class="material-symbols-outlined text-[16px]">add</span>
              新增一列
            </button>
          </div>

          <div class="overflow-x-auto">
            <table
              data-testid="lines-table"
              class="w-full border border-outline-variant rounded-lg"
            >
              <thead>
                <tr class="bg-surface-container-low border-b border-outline-variant">
                  <th
                    v-for="col in lineColumns"
                    :key="col.key"
                    :style="{ minWidth: col.width }"
                    class="px-2 py-2 text-left font-label-header text-label-header text-secondary whitespace-nowrap"
                    :class="col.align === 'right' ? 'text-right' : ''"
                  >
                    {{ col.label }}
                  </th>
                  <th class="w-10" />
                </tr>
              </thead>
              <tbody>
                <tr
                  v-for="(row, rowIndex) in lines"
                  :key="rowIndex"
                  :data-testid="`line-row-${rowIndex}`"
                  class="border-b border-outline-variant last:border-b-0"
                >
                  <td
                    v-for="col in lineColumns"
                    :key="col.key"
                    class="px-1 py-1"
                  >
                    <input
                      v-if="col.component === 'number'"
                      v-model="row[col.key]"
                      type="number"
                      step="any"
                      :data-testid="`line-${rowIndex}-${col.key}`"
                      class="w-full h-8 px-2 bg-transparent border border-transparent rounded font-body-dense text-body-dense text-on-surface text-right hover:border-outline-variant focus:outline-none focus:border-primary-container focus:bg-surface-container-low transition-all"
                    />
                    <input
                      v-else
                      v-model="row[col.key]"
                      type="text"
                      :data-testid="`line-${rowIndex}-${col.key}`"
                      class="w-full h-8 px-2 bg-transparent border border-transparent rounded font-body-dense text-body-dense text-on-surface hover:border-outline-variant focus:outline-none focus:border-primary-container focus:bg-surface-container-low transition-all"
                    />
                  </td>
                  <td class="px-1 py-1 text-center">
                    <!-- 只剩一列時不給刪，避免刪到沒有任何可填的列 -->
                    <button
                      v-if="lines.length > 1"
                      type="button"
                      :data-testid="`line-remove-${rowIndex}`"
                      title="刪除這一列"
                      class="w-7 h-7 inline-flex items-center justify-center rounded text-secondary hover:bg-error-container hover:text-on-error-container transition-colors"
                      @click="removeLine(rowIndex)"
                    >
                      <span class="material-symbols-outlined text-[16px]">
                        delete
                      </span>
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>

          <!-- 金額。使用者填明細時就看得到結果，不必等送出。 -->
          <div class="mt-space-md flex justify-end">
            <dl class="w-[280px] font-body-dense text-body-dense">
              <div class="flex justify-between py-1">
                <dt class="text-secondary">小計</dt>
                <dd
                  data-testid="computed-subtotal"
                  class="font-data-mono text-on-surface"
                >
                  {{ money(subtotal) }}
                </dd>
              </div>
              <div class="flex justify-between py-1">
                <dt class="text-secondary">營業稅 5%</dt>
                <dd
                  data-testid="computed-tax"
                  class="font-data-mono text-on-surface"
                >
                  {{ money(tax) }}
                </dd>
              </div>
              <div
                v-for="field in displayFields"
                :key="field.key"
                class="flex justify-between py-1 border-t border-outline-variant mt-1 pt-2"
              >
                <dt class="text-secondary">{{ field.ui?.label }}</dt>
                <dd
                  :data-testid="`computed-${field.key}`"
                  class="font-data-mono font-semibold text-on-surface"
                >
                  {{ displayValue(field) }}{{ field.ui?.suffix ?? '' }}
                </dd>
              </div>
            </dl>
          </div>
        </div>

        <div
          v-if="ungrouped.length > 0"
          class="bg-surface-container-lowest border border-outline-variant rounded-xl p-space-lg mb-space-md"
        >
          <div class="grid grid-cols-12 gap-space-md">
            <label
              v-for="field in ungrouped"
              :key="field.key"
              :class="widthClass(field)"
              class="block"
            >
              <span class="font-label-header text-label-header text-secondary block mb-1">
                {{ field.ui?.label ?? field.key }}
              </span>
              <input
                v-if="field.ui?.component === 'number'"
                v-model="values[field.key]"
                type="number"
                step="any"
                :data-testid="`field-${field.key}`"
                :class="FIELD_CLASS"
              />
              <input
                v-else
                v-model="values[field.key]"
                type="text"
                :data-testid="`field-${field.key}`"
                :class="FIELD_CLASS"
              />
            </label>
          </div>
        </div>

        <!--
          客戶聯絡人不在表單定義裡，但流程的客戶簽核需要它。
          缺了的話會在簽核兩關之後才失敗。
        -->
        <div
          class="bg-surface-container-lowest border border-outline-variant rounded-xl p-space-lg mb-space-lg"
        >
          <h2 class="font-title-md text-[15px] text-on-surface mb-space-md">
            客戶簽核對象
          </h2>
          <label class="block">
            <span class="font-label-header text-label-header text-secondary block mb-1">
              客戶聯絡人 Email <span class="text-error">*</span>
            </span>
            <input
              v-model="customerContacts"
              type="text"
              placeholder="多筆以逗號分隔，例如：a@x.com, b@y.com"
              data-testid="field-customer_contacts"
              :class="FIELD_CLASS"
            />
            <span class="font-label-caption text-label-caption text-secondary mt-1 block">
              流程走到客戶簽核時會通知這些人
            </span>
          </label>
        </div>

        <div class="flex items-center gap-space-sm">
          <button
            type="submit"
            :disabled="submitting"
            data-testid="create-submit"
            class="h-9 px-5 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 disabled:opacity-50 disabled:cursor-not-allowed transition-opacity"
          >
            {{ submitting ? '建立中…' : '建立並送簽' }}
          </button>
          <button
            type="button"
            data-testid="create-cancel"
            class="h-9 px-5 rounded-lg border border-outline-variant text-on-surface-variant font-label-header text-label-header hover:bg-surface-container-low transition-colors"
            @click="router.push('/quotations')"
          >
            取消
          </button>
        </div>
      </form>
    </div>
  </AppShell>
</template>
