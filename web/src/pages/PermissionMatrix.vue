<script setup lang="ts">
/**
 * 表單權限設定矩陣
 *
 * 依設計稿 docs/UI 設計/.../_7 實作。
 *
 * 核心是一張表：列為欄位、欄為流程節點，每格是三態權限。
 * 首欄凍結，欄位多時可橫向捲動。
 *
 * 修改暫存在前端，按下儲存才送出。這樣使用者可以連續調整多格，
 * 也能一次放棄全部修改。
 */
import { computed, onMounted, ref, watch } from 'vue'
import AppShell from '@/components/AppShell.vue'
import PermissionCell from '@/components/PermissionCell.vue'
import { getMatrix, type MatrixResponse, type Permission, type ViewMode } from '@/api/permissions'
import { listForms, type FormSummary } from '@/api/forms'
import { ApiError } from '@/api/types'

const forms = ref<FormSummary[]>([])
const matrix = ref<MatrixResponse | null>(null)
const loading = ref(false)
const error = ref<string | null>(null)

const selectedForm = ref<string>('')
const mode = ref<ViewMode>('by_role')
const selectedRole = ref('requester')
const selectedNode = ref('start')
const filter = ref('')

/** 未儲存的修改。key 為 `${rowKey}:${colKey}`。 */
const pending = ref<Map<string, Permission>>(new Map())

const roles = [
  { code: 'admin', label: '系統管理員' },
  { code: 'designer', label: '流程設計者' },
  { code: 'approver', label: '簽核人員' },
  { code: 'requester', label: '申請人' },
  { code: 'viewer', label: '檢視者' },
]

const modifiedCount = computed(() => pending.value.size)

/** 依搜尋字串過濾列。比對標籤與資料路徑。 */
const visibleRows = computed(() => {
  if (!matrix.value) return []
  const q = filter.value.trim().toLowerCase()
  if (!q) return matrix.value.rows

  return matrix.value.rows.filter(
    (r) =>
      r.label.toLowerCase().includes(q) ||
      r.key.toLowerCase().includes(q) ||
      (r.data_path ?? '').toLowerCase().includes(q),
  )
})

/** 依 section 分組，未分組的放最後 */
const groupedRows = computed(() => {
  if (!matrix.value) return []
  const sections = matrix.value.sections
  const rows = visibleRows.value

  const groups = sections
    .map((s) => ({
      key: s.key,
      title: s.title,
      rows: rows.filter((r) => r.section === s.key),
    }))
    .filter((g) => g.rows.length > 0)

  const ungrouped = rows.filter((r) => !r.section || !sections.some((s) => s.key === r.section))
  if (ungrouped.length) {
    groups.push({ key: '__ungrouped', title: '其他欄位', rows: ungrouped })
  }
  return groups
})

function cellKey(rowKey: string, colKey: string) {
  return `${rowKey}:${colKey}`
}

function effectivePermission(rowKey: string, colKey: string): Permission {
  const pendingValue = pending.value.get(cellKey(rowKey, colKey))
  if (pendingValue) return pendingValue
  return matrix.value?.rows.find((r) => r.key === rowKey)?.cells[colKey]?.permission ?? 'HIDDEN'
}

function isModified(rowKey: string, colKey: string) {
  return pending.value.has(cellKey(rowKey, colKey))
}

function onCellChange(rowKey: string, colKey: string, next: Permission) {
  const original = matrix.value?.rows.find((r) => r.key === rowKey)?.cells[colKey]?.permission
  const key = cellKey(rowKey, colKey)

  // 改回原值就從待儲存清單移除，避免誤以為有修改
  if (next === original) {
    pending.value.delete(key)
  } else {
    pending.value.set(key, next)
  }
  // Map 的變更不會觸發響應，需重新指派
  pending.value = new Map(pending.value)
}

/** 整欄批次設定 */
function applyColumn(colKey: string, permission: Permission) {
  if (!matrix.value) return
  for (const row of matrix.value.rows) {
    const cell = row.cells[colKey]
    if (!cell || cell.locked) continue // 鎖定的格子不動
    onCellChange(row.key, colKey, permission)
  }
}

function discardChanges() {
  pending.value = new Map()
}

async function load() {
  if (!selectedForm.value) return
  loading.value = true
  error.value = null
  try {
    matrix.value = await getMatrix(selectedForm.value, {
      mode: mode.value,
      role: mode.value === 'by_role' ? selectedRole.value : undefined,
      nodeId: mode.value === 'by_node' ? selectedNode.value : undefined,
    })
    pending.value = new Map()
  } catch (e) {
    error.value = e instanceof ApiError ? e.message : '載入失敗'
    matrix.value = null
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  try {
    forms.value = await listForms()
    if (forms.value.length > 0) {
      selectedForm.value = forms.value[0].form_key
    }
  } catch (e) {
    error.value = e instanceof ApiError ? e.message : '無法載入表單清單'
  }
})

watch([selectedForm, mode, selectedRole, selectedNode], load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="hover:text-on-surface cursor-pointer">設計器</span>
      <span class="material-symbols-outlined text-[14px]">chevron_right</span>
      <span class="text-on-surface font-semibold">表單權限設定</span>
    </template>

    <div class="flex flex-col w-full">
      <!-- 標題 -->
      <div class="flex items-start justify-between mb-4">
        <div>
          <div class="flex items-center gap-2">
            <span class="inline-flex items-center justify-center w-7 h-7 rounded-lg bg-surface-container-high text-primary">
              <span class="material-symbols-outlined text-[18px]">admin_panel_settings</span>
            </span>
            <h1 class="font-headline-xl text-headline-xl text-on-surface" data-testid="page-title">
              表單權限設定
            </h1>
            <span class="inline-flex items-center px-2 py-0.5 rounded-full font-label-caption text-label-caption bg-secondary-container text-on-secondary-fixed font-semibold">
              矩陣模式
            </span>
          </div>
          <p class="font-body-dense text-body-dense text-secondary mt-1">
            配置表單各欄位於工作流程不同簽核節點的檢視與編輯權限
          </p>
        </div>
      </div>

      <!-- 篩選列 -->
      <div class="sticky top-0 z-30 bg-surface-container-lowest rounded-xl shadow-sm mb-4 p-3">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex flex-wrap items-center gap-3">
            <label class="flex items-center gap-1.5">
              <span class="font-label-header text-label-header text-secondary">表單</span>
              <select
                v-model="selectedForm"
                data-testid="form-picker"
                class="h-8 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
              >
                <option v-for="f in forms" :key="f.form_key" :value="f.form_key">
                  {{ f.name }}
                </option>
              </select>
            </label>

            <label class="flex items-center gap-1.5">
              <span class="font-label-header text-label-header text-secondary">檢視維度</span>
              <div class="inline-flex rounded-lg border border-outline-variant overflow-hidden">
                <button
                  type="button"
                  data-testid="mode-by-role"
                  class="px-3 h-8 font-body-dense text-body-dense transition-colors"
                  :class="mode === 'by_role' ? 'bg-primary-container text-on-primary' : 'bg-surface-container-lowest text-secondary'"
                  @click="mode = 'by_role'"
                >
                  依角色
                </button>
                <button
                  type="button"
                  data-testid="mode-by-node"
                  class="px-3 h-8 font-body-dense text-body-dense transition-colors"
                  :class="mode === 'by_node' ? 'bg-primary-container text-on-primary' : 'bg-surface-container-lowest text-secondary'"
                  @click="mode = 'by_node'"
                >
                  依節點
                </button>
              </div>
            </label>

            <label v-if="mode === 'by_role'" class="flex items-center gap-1.5">
              <span class="font-label-header text-label-header text-secondary">當前角色</span>
              <select
                v-model="selectedRole"
                data-testid="role-picker"
                class="h-8 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
              >
                <option v-for="r in roles" :key="r.code" :value="r.code">{{ r.label }}</option>
              </select>
            </label>
          </div>

          <!-- 圖例 -->
          <div class="flex items-center gap-3 font-label-caption text-label-caption">
            <span class="flex items-center gap-1">
              <span class="w-2 h-2 rounded-full bg-[#15803D]" />可編輯
            </span>
            <span class="flex items-center gap-1">
              <span class="w-2 h-2 rounded-full bg-primary" />唯讀
            </span>
            <span class="flex items-center gap-1">
              <span class="w-2 h-2 rounded-full bg-outline" />隱藏
            </span>
            <span class="flex items-center gap-1 text-secondary">
              <span class="material-symbols-outlined text-[12px]">lock</span>系統計算鎖定
            </span>
          </div>
        </div>
      </div>

      <!-- 矩陣 -->
      <div class="bg-surface-container-lowest rounded-xl shadow-sm overflow-hidden">
        <div class="flex items-center justify-between px-4 py-3 border-b border-outline-variant">
          <h2 class="font-title-md text-title-md text-on-surface">
            欄位權限清單
            <span v-if="matrix" class="font-body-dense text-body-dense text-secondary ml-2">
              共 {{ matrix.sections.length }} 個分組 · {{ matrix.rows.length }} 個欄位
            </span>
          </h2>
          <div class="relative flex items-center">
            <span class="material-symbols-outlined absolute left-2 text-[16px] text-outline">search</span>
            <input
              v-model="filter"
              type="text"
              placeholder="過濾欄位名稱或路徑"
              data-testid="field-filter"
              class="h-8 w-64 pl-7 pr-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
            />
          </div>
        </div>

        <div v-if="loading" class="p-8 text-center text-secondary font-body-dense" data-testid="loading">
          載入中
        </div>

        <div v-else-if="error" class="p-8 text-center text-error font-body-dense" data-testid="error">
          {{ error }}
        </div>

        <div v-else-if="!matrix || matrix.rows.length === 0" class="p-8 text-center text-secondary font-body-dense" data-testid="empty">
          沒有可設定的欄位
        </div>

        <div v-else class="overflow-x-auto">
          <table class="w-full border-collapse" data-testid="permission-matrix">
            <thead>
              <tr class="bg-surface-container-low">
                <th class="sticky left-0 z-10 bg-surface-container-low text-left px-4 py-2 w-[280px] font-label-header text-label-header text-secondary">
                  表單結構與欄位名稱
                </th>
                <th
                  v-for="col in matrix.columns"
                  :key="col.key"
                  class="px-3 py-2 text-left font-label-header text-label-header text-secondary min-w-[140px] group"
                  :data-testid="`col-${col.key}`"
                >
                  <div class="flex items-center gap-1">
                    <span class="text-on-surface">{{ col.label }}</span>
                    <span
                      v-if="col.participant_kind === 'external'"
                      class="px-1 rounded bg-[#FED7AA] text-[#9A3412] text-[10px] font-semibold"
                    >外部</span>
                    <button
                      type="button"
                      :data-testid="`col-menu-${col.key}`"
                      class="ml-auto opacity-0 group-hover:opacity-100 transition-opacity text-secondary hover:text-on-surface"
                      title="整欄批次設定"
                      @click="applyColumn(col.key, 'READONLY')"
                    >
                      <span class="material-symbols-outlined text-[16px]">more_vert</span>
                    </button>
                  </div>
                </th>
              </tr>
            </thead>

            <tbody>
              <template v-for="group in groupedRows" :key="group.key">
                <tr class="bg-surface-container">
                  <td
                    :colspan="matrix.columns.length + 1"
                    class="px-4 py-1.5 font-label-header text-label-header text-secondary"
                  >
                    <span class="material-symbols-outlined text-[14px] align-middle mr-1">folder</span>
                    {{ group.title }}
                    <span class="text-outline">（{{ group.rows.length }} 個欄位）</span>
                  </td>
                </tr>

                <tr
                  v-for="row in group.rows"
                  :key="row.key"
                  class="border-b border-outline-variant hover:bg-surface-container-low"
                  :data-testid="`row-${row.key}`"
                >
                  <td class="sticky left-0 z-10 bg-surface-container-lowest px-4 py-2">
                    <div class="flex items-center gap-1.5">
                      <span class="font-body-dense text-body-dense text-on-surface">
                        {{ row.label }}
                      </span>
                      <span
                        v-if="row.external_visible === false"
                        class="px-1 rounded bg-[#FED7AA] text-[#9A3412] text-[10px] font-semibold"
                        data-testid="external-hidden-badge"
                      >客戶看不到</span>
                    </div>
                    <div v-if="row.data_path" class="font-data-mono text-[11px] text-outline mt-0.5">
                      {{ row.data_path }}
                    </div>
                  </td>

                  <td v-for="col in matrix.columns" :key="col.key" class="px-3 py-2">
                    <PermissionCell
                      v-if="row.cells[col.key]"
                      :cell="{ ...row.cells[col.key], permission: effectivePermission(row.key, col.key) }"
                      :modified="isModified(row.key, col.key)"
                      @change="(p) => onCellChange(row.key, col.key, p)"
                    />
                  </td>
                </tr>
              </template>
            </tbody>
          </table>
        </div>
      </div>

      <!-- 底部儲存列 -->
      <div
        v-if="modifiedCount > 0"
        class="sticky bottom-0 mt-4 bg-surface-container-lowest rounded-xl shadow-lg border border-outline-variant p-3 flex items-center justify-between"
        data-testid="save-bar"
      >
        <span class="flex items-center gap-2 font-body-dense text-body-dense text-on-surface">
          <span class="w-2 h-2 rounded-full bg-primary-container" />
          已暫存修改：{{ modifiedCount }} 處權限設定
        </span>
        <div class="flex items-center gap-2">
          <button
            type="button"
            data-testid="discard-changes"
            class="h-8 px-3 rounded-lg font-body-dense text-body-dense text-secondary hover:bg-surface-container-low"
            @click="discardChanges"
          >
            放棄所有修改
          </button>
          <button
            type="button"
            data-testid="save-changes"
            disabled
            title="儲存功能待表單定義寫回 API 完成"
            class="h-8 px-4 rounded-lg bg-primary-container text-on-primary font-body-dense text-body-dense opacity-50 cursor-not-allowed"
          >
            儲存權限矩陣
          </button>
        </div>
      </div>
    </div>
  </AppShell>
</template>
