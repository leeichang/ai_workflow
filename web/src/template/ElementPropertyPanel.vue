<script setup lang="ts">
/**
 * 元素屬性面板
 *
 * 依設計稿 _9 右側。表格選取時顯示欄位設定、表頭樣式、
 * 分頁與跨頁控制、合計列呈現——那是這個設計器最重要的一組設定，
 * 也是報價單能不能印對的關鍵。
 */
import { computed } from 'vue'
import { fieldOf, type TemplateElement } from './useTemplateDesigner'

const props = defineProps<{ element: TemplateElement | null }>()
const emit = defineEmits<{
  update: [Partial<TemplateElement>]
  remove: []
}>()

const field = computed(() => fieldOf(props.element))
const isTable = computed(() => props.element?.type === 'table')

const INPUT =
  'w-full h-8 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary-container'

function patch(key: keyof TemplateElement, value: unknown) {
  emit('update', { [key]: value } as Partial<TemplateElement>)
}

function patchPosition(axis: 'x' | 'y', value: string) {
  const n = Number(value)
  if (!Number.isFinite(n)) return
  emit('update', { position: { [axis]: n } as never })
}

/** 調整某一欄的對齊 */
function setColumnAlign(index: number, align: 'left' | 'center' | 'right') {
  emit('update', {
    columnAlignment: { ...(props.element?.columnAlignment ?? {}), [index]: align },
  })
}

/** 調整欄寬。百分比總和需維持 100，否則表格寬度會跑掉。 */
function setColumnWidth(index: number, value: string) {
  const n = Number(value)
  if (!Number.isFinite(n) || n <= 0) return

  const widths = [...(props.element?.headWidthPercentages ?? [])]
  widths[index] = n
  emit('update', { headWidthPercentages: widths })
}

const widthTotal = computed(() =>
  (props.element?.headWidthPercentages ?? []).reduce((a, b) => a + b, 0),
)

const ALIGNMENTS = [
  { value: 'left' as const, icon: 'format_align_left', label: '靠左' },
  { value: 'center' as const, icon: 'format_align_center', label: '置中' },
  { value: 'right' as const, icon: 'format_align_right', label: '靠右' },
]
</script>

<template>
  <aside
    class="w-[320px] shrink-0 bg-surface-container-lowest border-l border-outline-variant flex flex-col"
    data-testid="element-property-panel"
  >
    <div
      v-if="!element"
      class="flex-1 flex flex-col items-center justify-center gap-2 p-6 text-center"
      data-testid="panel-empty"
    >
      <span class="material-symbols-outlined text-[32px] text-outline">ads_click</span>
      <p class="font-body-dense text-body-dense text-secondary">
        點選版面上的元素以編輯屬性
      </p>
    </div>

    <template v-else>
      <div class="px-4 py-3 border-b border-outline-variant">
        <div class="flex items-center justify-between">
          <span class="font-title-md text-title-md text-on-surface truncate">
            {{ field?.label ?? element.content ?? element.type }}
          </span>
          <span
            v-if="isTable"
            class="px-1.5 py-0.5 rounded bg-primary-container text-on-primary font-label-caption text-[10px] font-bold shrink-0"
          >
            TABLE
          </span>
        </div>
        <span class="font-data-mono text-[11px] text-outline">
          {{ field?.source ?? element.name }}
        </span>
      </div>

      <div class="flex-1 overflow-y-auto p-4 space-y-4">
        <!-- 位置與大小 -->
        <div>
          <span class="block font-label-header text-label-header text-secondary mb-1.5">
            位置與大小 (mm)
          </span>
          <div class="grid grid-cols-2 gap-2">
            <label class="block">
              <span class="block font-label-caption text-label-caption text-outline mb-1">X</span>
              <input
                type="number"
                step="0.5"
                :value="element.position.x"
                data-testid="prop-x"
                :class="INPUT"
                @input="patchPosition('x', ($event.target as HTMLInputElement).value)"
              />
            </label>
            <label class="block">
              <span class="block font-label-caption text-label-caption text-outline mb-1">Y</span>
              <input
                type="number"
                step="0.5"
                :value="element.position.y"
                data-testid="prop-y"
                :class="INPUT"
                @input="patchPosition('y', ($event.target as HTMLInputElement).value)"
              />
            </label>
            <label class="block">
              <span class="block font-label-caption text-label-caption text-outline mb-1">寬</span>
              <input
                type="number"
                step="1"
                :value="element.width"
                data-testid="prop-width"
                :class="INPUT"
                @input="patch('width', Number(($event.target as HTMLInputElement).value))"
              />
            </label>
            <label class="block">
              <span class="block font-label-caption text-label-caption text-outline mb-1">高</span>
              <input
                type="number"
                step="1"
                :value="element.height"
                data-testid="prop-height"
                :class="INPUT"
                @input="patch('height', Number(($event.target as HTMLInputElement).value))"
              />
            </label>
          </div>
        </div>

        <!-- 文字樣式 -->
        <template v-if="element.type === 'text'">
          <div class="grid grid-cols-2 gap-2">
            <label class="block">
              <span class="block font-label-header text-label-header text-secondary mb-1">
                字級 (pt)
              </span>
              <input
                type="number"
                min="5"
                max="48"
                :value="element.fontSize ?? 9"
                data-testid="prop-font-size"
                :class="INPUT"
                @input="patch('fontSize', Number(($event.target as HTMLInputElement).value))"
              />
            </label>
            <div>
              <span class="block font-label-header text-label-header text-secondary mb-1">
                對齊
              </span>
              <div class="flex gap-0.5 p-0.5 bg-surface-container-low rounded-lg">
                <button
                  v-for="a in ALIGNMENTS"
                  :key="a.value"
                  type="button"
                  :title="a.label"
                  :aria-pressed="(element.alignment ?? 'left') === a.value"
                  :data-testid="`prop-align-${a.value}`"
                  class="flex-1 h-7 rounded flex items-center justify-center transition-colors"
                  :class="
                    (element.alignment ?? 'left') === a.value
                      ? 'bg-surface-container-lowest text-primary shadow-sm'
                      : 'text-secondary hover:text-on-surface'
                  "
                  @click="patch('alignment', a.value)"
                >
                  <span class="material-symbols-outlined text-[16px]">{{ a.icon }}</span>
                </button>
              </div>
            </div>
          </div>

          <label v-if="!element.bound" class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              文字內容
            </span>
            <input
              :value="element.content ?? ''"
              data-testid="prop-content"
              :class="INPUT"
              @input="patch('content', ($event.target as HTMLInputElement).value)"
            />
          </label>
        </template>

        <!-- 表格設定 -->
        <template v-if="isTable">
          <div class="pt-3 border-t border-outline-variant">
            <div class="flex items-center justify-between mb-1.5">
              <span class="font-label-header text-label-header text-secondary">
                欄位設定 (Columns)
              </span>
              <span
                class="font-label-caption text-label-caption"
                :class="Math.round(widthTotal) === 100 ? 'text-secondary' : 'text-error'"
                data-testid="width-total"
              >
                {{ Math.round(widthTotal) }}%
              </span>
            </div>

            <!-- 總和不是 100% 時表格寬度會與設定不符，明講而非靜默修正 -->
            <p
              v-if="Math.round(widthTotal) !== 100"
              class="mb-1.5 px-2 py-1 rounded bg-error-container font-label-caption text-label-caption text-on-error-container"
            >
              欄寬總和需為 100%，否則表格寬度與設定不符
            </p>

            <div class="space-y-1">
              <div
                v-for="(h, i) in element.head ?? []"
                :key="i"
                class="flex items-center gap-1.5"
                :data-testid="`column-${i}`"
              >
                <span class="material-symbols-outlined text-[14px] text-outline shrink-0">
                  drag_indicator
                </span>
                <span class="flex-1 font-body-dense text-[12px] text-on-surface truncate">
                  {{ h }}
                </span>
                <input
                  type="number"
                  min="1"
                  max="100"
                  :value="element.headWidthPercentages?.[i] ?? 20"
                  :data-testid="`column-width-${i}`"
                  class="w-14 h-7 px-1 bg-surface-container-low border border-outline-variant rounded font-data-mono text-[11px] text-right"
                  @input="setColumnWidth(i, ($event.target as HTMLInputElement).value)"
                />
                <div class="flex gap-0.5 shrink-0">
                  <button
                    v-for="a in ALIGNMENTS"
                    :key="a.value"
                    type="button"
                    :title="a.label"
                    :data-testid="`column-align-${i}-${a.value}`"
                    class="w-6 h-7 rounded flex items-center justify-center transition-colors"
                    :class="
                      (element.columnAlignment?.[i] ?? 'left') === a.value
                        ? 'bg-surface-container-high text-primary'
                        : 'text-outline hover:text-on-surface'
                    "
                    @click="setColumnAlign(i, a.value)"
                  >
                    <span class="material-symbols-outlined text-[14px]">{{ a.icon }}</span>
                  </button>
                </div>
              </div>
            </div>
          </div>

          <!-- 分頁與跨頁控制 -->
          <div class="pt-3 border-t border-outline-variant space-y-2.5">
            <span class="block font-label-header text-label-header text-secondary">
              分頁與跨頁控制 (Pagination)
            </span>

            <label class="flex items-start justify-between gap-2">
              <span class="min-w-0">
                <span class="block font-body-dense text-body-dense text-on-surface">
                  換頁時重複表頭
                </span>
                <span class="block font-label-caption text-label-caption text-outline mt-0.5">
                  超過一頁時，每頁頂部自動顯示欄位標題
                </span>
              </span>
              <input
                type="checkbox"
                :checked="element.repeatHead !== false"
                data-testid="prop-repeat-head"
                class="mt-0.5 shrink-0"
                @change="patch('repeatHead', ($event.target as HTMLInputElement).checked)"
              />
            </label>

            <label class="flex items-start justify-between gap-2">
              <span class="min-w-0">
                <span class="block font-body-dense text-body-dense text-on-surface">
                  合計列不分頁拆散
                </span>
                <span class="block font-label-caption text-label-caption text-outline mt-0.5">
                  保留最後
                  <input
                    type="number"
                    min="0"
                    max="9"
                    :value="element.summaryRowCount ?? 3"
                    data-testid="prop-summary-count"
                    class="w-10 h-5 px-1 mx-0.5 bg-surface-container-low border border-outline-variant rounded font-data-mono text-[11px] text-center"
                    @input="
                      patch('summaryRowCount', Number(($event.target as HTMLInputElement).value))
                    "
                  />
                  列與總計同行，避免落入次頁
                </span>
              </span>
            </label>

            <label class="flex items-start justify-between gap-2">
              <span class="min-w-0">
                <span class="block font-body-dense text-body-dense text-on-surface">
                  隔行變色 (Zebra)
                </span>
                <span class="block font-label-caption text-label-caption text-outline mt-0.5">
                  偶數列使用微淺灰色
                </span>
              </span>
              <input
                type="checkbox"
                :checked="element.alternateBackground !== false"
                data-testid="prop-zebra"
                class="mt-0.5 shrink-0"
                @change="
                  patch('alternateBackground', ($event.target as HTMLInputElement).checked)
                "
              />
            </label>
          </div>
        </template>
      </div>

      <div class="p-4 border-t border-outline-variant">
        <button
          type="button"
          data-testid="prop-remove"
          class="w-full h-9 rounded-lg border border-error text-error font-body-dense text-body-dense hover:bg-error-container"
          @click="emit('remove')"
        >
          從版面移除
        </button>
      </div>
    </template>
  </aside>
</template>
