<script setup lang="ts">
/**
 * 欄位屬性面板
 *
 * 依設計稿 _5 的右側面板，三個頁籤對應 schema 的三個 key：
 *   基本 → ui、資料 → data、規則 → workflow
 *
 * 這個對應不是巧合。設計稿把「長什麼樣」「資料在哪」「什麼時候能改」
 * 分成三類，正是 form.schema.json 的分層方式。
 */
import { computed, ref } from 'vue'
import type { FormField } from '@/api/types'
import RoleListEditor from './RoleListEditor.vue'
import { ROLES } from './roles'

const props = defineProps<{ field: FormField | null }>()
const emit = defineEmits<{
  update: [Partial<FormField>]
  remove: []
}>()

type Tab = 'basic' | 'data' | 'rules'
const tab = ref<Tab>('basic')

const tabs: { key: Tab; label: string }[] = [
  { key: 'basic', label: '基本' },
  { key: 'data', label: '資料' },
  { key: 'rules', label: '規則' },
]

const componentLabel = computed(() => {
  const map: Record<string, string> = {
    input: '單行文字', textarea: '多行文字', number: '數字', date: '日期',
    datetime: '日期時間', select: '下拉選單', multi_select: '多選',
    checkbox: '核取方塊', radio: '單選', switch: '開關', upload: '檔案上傳',
    table: '明細表格', reference: '關聯查詢', user_picker: '人員選擇',
    department_picker: '部門選擇', display: '唯讀顯示',
  }
  return props.field ? map[props.field.ui.component] ?? props.field.ui.component : ''
})

/** 資料路徑格式檢查。空字串代表尚未綁定。 */
const pathValid = computed(() => {
  const p = props.field?.data.path
  if (!p) return null
  return /^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$/.test(p)
})

/**
 * 可綁定的基本資料來源
 *
 * 與後端 lookup.rs 的 SOURCES 白名單一致。寫死而非查 API，
 * 因為這是設計時的選項——設計器開啟時多打一次 API 不划算，
 * 而來源本身是 API 契約的一部分，不會動態增減。
 */
const LOOKUP_SOURCES = [
  { value: 'employee', label: '員工' },
  { value: 'department', label: '部門' },
  { value: 'role', label: '角色' },
] as const

/** 只有這幾種元件有選項的概念 */
const OPTION_COMPONENTS = new Set(['select', 'multi_select', 'radio'])

const isOptionField = computed(() =>
  props.field ? OPTION_COMPONENTS.has(props.field.ui.component) : false,
)

/**
 * 切換選項來源
 *
 * 選空字串時移除 option_source 而非設成空物件——
 * schema 要求 option_source 必填 source，空物件驗證不過。
 */
function patchOptionSource(source: string) {
  if (source === '') {
    const ui = { ...props.field!.ui }
    delete (ui as Record<string, unknown>).option_source
    emit('update', { ui: ui as FormField['ui'] })
    return
  }
  patchUi('option_source', { source })
}

function patchUi(key: string, value: unknown) {
  emit('update', { ui: { ...props.field!.ui, [key]: value } as FormField['ui'] })
}

function patchData(key: string, value: unknown) {
  emit('update', { data: { ...props.field!.data, [key]: value } as FormField['data'] })
}

function patchWorkflow(key: string, value: unknown) {
  const next = { ...(props.field!.workflow ?? {}), [key]: value }
  // 空字串等同未設定，移除以免產生 "" 這種無意義的表達式
  if (value === '' || value === undefined) delete (next as Record<string, unknown>)[key]
  emit('update', { workflow: next })
}

/** 12 欄寬度選擇器 */
const widthOptions = Array.from({ length: 12 }, (_, i) => i + 1)

function widthLabel(w: number) {
  if (w === 12) return '整行'
  if (w === 6) return '1/2 寬'
  if (w === 4) return '1/3 寬'
  if (w === 3) return '1/4 寬'
  return `${w}/12`
}
</script>

<template>
  <aside
    class="w-[340px] shrink-0 bg-surface-container-lowest border-l border-outline-variant flex flex-col h-full overflow-hidden"
    data-testid="property-panel"
  >
    <div
      v-if="!field"
      class="flex-1 flex flex-col items-center justify-center text-secondary p-6 text-center"
      data-testid="property-empty"
    >
      <span class="material-symbols-outlined text-[32px] mb-2">ads_click</span>
      <p class="font-body-dense text-body-dense">點選畫布中的欄位以編輯屬性</p>
    </div>

    <template v-else>
      <!-- 標題 -->
      <div class="px-4 py-3 border-b border-outline-variant">
        <div class="flex items-center justify-between gap-2">
          <div class="flex items-center gap-1.5 min-w-0">
            <span class="material-symbols-outlined text-[18px] text-primary">settings_suggest</span>
            <span
              class="font-title-md text-title-md text-on-surface truncate"
              data-testid="property-title"
            >{{ field.ui.label || field.key }}</span>
          </div>
          <span class="px-2 py-0.5 rounded-lg bg-secondary-container text-on-secondary-fixed font-label-caption text-label-caption shrink-0">
            {{ componentLabel }}
          </span>
        </div>
        <div class="flex items-center justify-between mt-1">
          <span class="font-data-mono text-[11px] text-outline">#{{ field.key }}</span>
          <span
            v-if="pathValid !== null"
            class="flex items-center gap-0.5 font-label-caption text-label-caption"
            :class="pathValid ? 'text-[#15803D]' : 'text-error'"
          >
            <span class="material-symbols-outlined text-[12px]">
              {{ pathValid ? 'check_circle' : 'error' }}
            </span>
            {{ pathValid ? '綁定正常' : '路徑格式錯誤' }}
          </span>
        </div>
      </div>

      <!-- 頁籤 -->
      <div class="flex border-b border-outline-variant" role="tablist">
        <button
          v-for="t in tabs"
          :key="t.key"
          type="button"
          role="tab"
          :data-testid="`tab-${t.key}`"
          :aria-selected="tab === t.key"
          class="flex-1 h-10 font-body-dense text-body-dense transition-colors border-b-2"
          :class="tab === t.key
            ? 'border-primary-container text-primary font-semibold'
            : 'border-transparent text-secondary hover:text-on-surface'"
          @click="tab = t.key"
        >
          {{ t.label }}
        </button>
      </div>

      <div class="flex-1 overflow-y-auto p-4 space-y-4">
        <!-- 基本 -->
        <template v-if="tab === 'basic'">
          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              欄位標籤 (Label)
            </span>
            <input
              :value="field.ui.label"
              data-testid="prop-label"
              class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
              @input="patchUi('label', ($event.target as HTMLInputElement).value)"
            />
          </label>

          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              提示文字 (Placeholder)
            </span>
            <input
              :value="field.ui.placeholder ?? ''"
              data-testid="prop-placeholder"
              class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
              @input="patchUi('placeholder', ($event.target as HTMLInputElement).value)"
            />
          </label>

          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              輔助說明文字 (Help text)
            </span>
            <textarea
              :value="field.ui.help ?? ''"
              rows="3"
              data-testid="prop-help"
              class="w-full px-2 py-1.5 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense resize-none"
              @input="patchUi('help', ($event.target as HTMLTextAreaElement).value)"
            />
          </label>

          <!-- 12 欄寬度選擇器 -->
          <div>
            <div class="flex items-center justify-between mb-1">
              <span class="font-label-header text-label-header text-secondary">欄位寬度</span>
              <span class="font-data-mono text-[11px] text-primary">
                col-span-{{ field.ui.width ?? 12 }}（{{ widthLabel(field.ui.width ?? 12) }}）
              </span>
            </div>
            <div class="flex gap-0.5" data-testid="prop-width">
              <button
                v-for="w in widthOptions"
                :key="w"
                type="button"
                :title="widthLabel(w)"
                :data-width="w"
                class="flex-1 h-6 rounded transition-colors"
                :class="w <= (field.ui.width ?? 12)
                  ? 'bg-primary-container'
                  : 'bg-surface-container-high hover:bg-surface-dim'"
                @click="patchUi('width', w)"
              />
            </div>
          </div>

          <div v-if="field.ui.component === 'number'" class="flex items-center gap-3">
            <label class="flex-1">
              <span class="block font-label-header text-label-header text-secondary mb-1">
                小數位數
              </span>
              <input
                type="number"
                min="0"
                max="6"
                :value="field.ui.precision ?? 0"
                data-testid="prop-precision"
                class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
                @input="patchUi('precision', Number(($event.target as HTMLInputElement).value))"
              />
            </label>
            <label class="flex-1">
              <span class="block font-label-header text-label-header text-secondary mb-1">前綴</span>
              <input
                :value="field.ui.prefix ?? ''"
                data-testid="prop-prefix"
                class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
                @input="patchUi('prefix', ($event.target as HTMLInputElement).value)"
              />
            </label>
            <label class="flex-1">
              <span class="block font-label-header text-label-header text-secondary mb-1">後綴</span>
              <input
                :value="field.ui.suffix ?? ''"
                data-testid="prop-suffix"
                class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
                @input="patchUi('suffix', ($event.target as HTMLInputElement).value)"
              />
            </label>
          </div>

          <!--
            選項來源（select / multi_select / radio）

            寫死的 options 維護不了組織異動——有人到職、部門改組時
            要回頭改每一張表單。綁資料來源才能跟著變。
          -->
          <div
            v-if="isOptionField"
            class="pt-2 border-t border-outline-variant"
          >
            <span class="block font-label-header text-label-header text-secondary mb-1">
              選項來源
            </span>
            <select
              :value="field.ui.option_source?.source ?? ''"
              data-testid="prop-option-source"
              class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
              @change="patchOptionSource(($event.target as HTMLSelectElement).value)"
            >
              <option value="">固定選項（自行輸入）</option>
              <option v-for="s in LOOKUP_SOURCES" :key="s.value" :value="s.value">
                {{ s.label }}
              </option>
            </select>
            <span class="block font-label-caption text-label-caption text-outline mt-1">
              {{
                field.ui.option_source?.source
                  ? '選項由系統即時查詢，組織異動自動反映'
                  : '選項寫在表單定義裡，組織異動時要手動維護'
              }}
            </span>
          </div>

          <!-- 客戶 Portal 可見 -->
          <div class="flex items-start justify-between gap-2 pt-2 border-t border-outline-variant">
            <div class="min-w-0">
              <span class="block font-label-header text-label-header text-on-surface">
                客戶 Portal 可見
              </span>
              <span class="block font-label-caption text-label-caption text-secondary mt-0.5">
                關閉後客戶端檢視頁不呈現此欄位
              </span>
            </div>
            <button
              type="button"
              role="switch"
              :aria-checked="field.ui.external_visible !== false"
              data-testid="prop-external-visible"
              class="relative w-10 h-6 rounded-full transition-colors shrink-0"
              :class="field.ui.external_visible !== false ? 'bg-primary-container' : 'bg-surface-container-high'"
              @click="patchUi('external_visible', field.ui.external_visible === false)"
            >
              <span
                class="absolute top-1 w-4 h-4 rounded-full bg-surface-container-lowest transition-transform"
                :class="field.ui.external_visible !== false ? 'translate-x-5' : 'translate-x-1'"
              />
            </button>
          </div>
        </template>

        <!-- 資料 -->
        <template v-else-if="tab === 'data'">
          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              資料欄位路徑
            </span>
            <input
              :value="field.data.path"
              placeholder="quotation.discount_rate"
              data-testid="prop-data-path"
              class="w-full h-9 px-2 bg-surface-container-low border rounded-lg font-data-mono text-data-mono"
              :class="pathValid === false ? 'border-error' : 'border-outline-variant'"
              @input="patchData('path', ($event.target as HTMLInputElement).value)"
            />
            <span
              v-if="pathValid === false"
              class="block font-label-caption text-label-caption text-error mt-1"
              data-testid="prop-path-error"
            >
              格式須為 物件.欄位，例如 quotation.discount_rate
            </span>
          </label>

          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">型別</span>
            <select
              :value="field.data.type"
              data-testid="prop-data-type"
              class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
              @change="patchData('type', ($event.target as HTMLSelectElement).value)"
            >
              <option value="string">字串</option>
              <option value="text">長文字</option>
              <option value="integer">整數</option>
              <option value="decimal">小數</option>
              <option value="boolean">布林</option>
              <option value="date">日期</option>
              <option value="datetime">日期時間</option>
              <option value="uuid">識別碼</option>
              <option value="array">陣列</option>
              <option value="file">檔案</option>
            </select>
          </label>

          <div class="flex items-center gap-3">
            <label class="flex-1">
              <span class="block font-label-header text-label-header text-secondary mb-1">預設值</span>
              <input
                :value="String(field.data.default ?? '')"
                data-testid="prop-default"
                class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
                @input="patchData('default', ($event.target as HTMLInputElement).value)"
              />
            </label>
            <label class="flex-1">
              <span class="block font-label-header text-label-header text-secondary mb-1">最小值</span>
              <input
                type="number"
                :value="field.data.min ?? ''"
                data-testid="prop-min"
                class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
                @input="patchData('min', Number(($event.target as HTMLInputElement).value))"
              />
            </label>
            <label class="flex-1">
              <span class="block font-label-header text-label-header text-secondary mb-1">最大值</span>
              <input
                type="number"
                :value="field.data.max ?? ''"
                data-testid="prop-max"
                class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
                @input="patchData('max', Number(($event.target as HTMLInputElement).value))"
              />
            </label>
          </div>

          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              計算連動公式
            </span>
            <textarea
              :value="field.data.computed ?? ''"
              rows="3"
              placeholder="quotation.subtotal * (1 - quotation.discount_rate)"
              data-testid="prop-computed"
              class="w-full px-2 py-1.5 bg-inverse-surface text-inverse-on-surface border border-outline-variant rounded-lg font-data-mono text-data-mono resize-none"
              @input="patchData('computed', ($event.target as HTMLTextAreaElement).value)"
            />
            <span class="block font-label-caption text-label-caption text-secondary mt-1">
              設定後此欄位由系統計算，使用者不可編輯
            </span>
          </label>
        </template>

        <!-- 規則 -->
        <template v-else>
          <div>
            <span class="block font-label-header text-label-header text-secondary mb-1">顯示條件</span>
            <input
              :value="field.workflow?.visible_when ?? ''"
              placeholder="永遠顯示"
              data-testid="prop-visible-when"
              class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-data-mono"
              @input="patchWorkflow('visible_when', ($event.target as HTMLInputElement).value)"
            />
          </div>

          <div>
            <span class="block font-label-header text-label-header text-secondary mb-1">唯讀條件</span>
            <input
              :value="field.workflow?.readonly_when ?? ''"
              placeholder="node.id not in ['start','revise']"
              data-testid="prop-readonly-when"
              class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-data-mono"
              @input="patchWorkflow('readonly_when', ($event.target as HTMLInputElement).value)"
            />
          </div>

          <div>
            <span class="block font-label-header text-label-header text-secondary mb-1">必填條件</span>
            <input
              :value="field.workflow?.required_when ?? ''"
              placeholder="true 代表永遠必填"
              data-testid="prop-required-when"
              class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-data-mono"
              @input="patchWorkflow('required_when', ($event.target as HTMLInputElement).value)"
            />
          </div>

          <div class="pt-2 border-t border-outline-variant">
            <RoleListEditor
              label="可編輯角色"
              testid="prop-editable-roles"
              :value="field.workflow?.editable_roles"
              :roles="ROLES"
              @change="patchWorkflow('editable_roles', $event)"
            />
          </div>

          <RoleListEditor
            label="可檢視角色"
            testid="prop-readable-roles"
            :value="field.workflow?.readable_roles"
            :roles="ROLES"
            @change="patchWorkflow('readable_roles', $event)"
          />

          <!--
            判定順序會推翻個別設定，設計者常誤以為規則沒生效。
            直接把後端 compute() 的優先序列出來。
          -->
          <details class="pt-2 border-t border-outline-variant" data-testid="prop-precedence">
            <summary class="font-label-header text-label-header text-secondary cursor-pointer">
              判定順序
            </summary>
            <ol class="mt-1.5 space-y-1 font-label-caption text-label-caption text-outline">
              <li>1. 外部參與者遇到「客戶 Portal 不可見」的欄位 → 隱藏</li>
              <li>2. 有計算連動公式 → 唯讀（管理員也不例外）</li>
              <li>3. 角色不在可檢視清單 → 隱藏</li>
              <li>4. 不符合顯示條件 → 隱藏</li>
              <li>5. 管理員 → 可編輯（不受以下限制）</li>
              <li>6. 外部參與者 → 唯讀</li>
              <li>7. 角色不在可編輯清單，或符合唯讀條件 → 唯讀</li>
            </ol>
          </details>
        </template>
      </div>

      <div class="p-3 border-t border-outline-variant">
        <button
          type="button"
          data-testid="prop-delete"
          class="flex items-center gap-1 text-error font-body-dense text-body-dense hover:underline"
          @click="emit('remove')"
        >
          <span class="material-symbols-outlined text-[16px]">delete</span>
          刪除此欄位
        </button>
      </div>
    </template>
  </aside>
</template>
