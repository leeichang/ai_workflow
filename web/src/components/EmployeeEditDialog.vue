<script setup lang="ts">
/**
 * 員工編輯
 *
 * 兩個設計重點：
 *
 * 1. **只送改過的欄位**。後端的 patch 帶 `fields` 清單，
 *    它同時是「要改什麼」與「要鎖哪些欄位」的依據
 *    （需求 07 §3 規則 2）。整包送出會讓沒碰過的欄位也被
 *    標成平台維護，日後同步就跳過它們——那不是使用者的意思。
 *
 * 2. **欄位鎖定要看得見**。已被平台接管的欄位旁顯示標記與
 *    「改回跟隨來源」，否則管理員不知道哪些是自己補的、
 *    哪些還跟著 ERP 走。
 *
 * email 與角色不在這裡改：email 是登入帳號，改它等於換人；
 * 角色是權限問題，不是組織資料。後端的白名單也擋著。
 */
import { computed, ref, watch } from 'vue'
import ManagedFieldMark from './ManagedFieldMark.vue'
import * as orgApi from '@/api/org'
import type { Department, Employee } from '@/api/org'
import { ApiError } from '@/api/types'

const props = defineProps<{
  employee: Employee
  departments: Department[]
  /** 可指派為主管的人。排除自己由本元件處理 */
  candidates: Employee[]
}>()

const emit = defineEmits<{ close: []; saved: [] }>()

/** 可編輯的欄位。與後端 EMPLOYEE_EDITABLE 一致 */
type EditableField =
  | 'name'
  | 'employee_no'
  | 'department_id'
  | 'manager_id'
  | 'job_title'
  | 'phone'
  | 'extension'
  | 'status'

/** 表單的值。null 與空字串在送出前統一處理 */
type FormState = Record<EditableField, string>

function toForm(e: Employee): FormState {
  return {
    name: e.name,
    employee_no: e.employee_no ?? '',
    department_id: e.department_id ?? '',
    manager_id: e.manager_id ?? '',
    job_title: e.job_title ?? '',
    phone: e.phone ?? '',
    extension: e.extension ?? '',
    status: e.status,
  }
}

const form = ref<FormState>(toForm(props.employee))
const original = ref<FormState>(toForm(props.employee))
const saving = ref(false)
const unlocking = ref('')
const errorMessage = ref('')

// 切換到另一個員工時重置。不重置的話會把上一個人的值帶過來
watch(
  () => props.employee,
  (e) => {
    form.value = toForm(e)
    original.value = toForm(e)
    errorMessage.value = ''
  },
)

/** 主管候選人要排除自己——後端也擋，但讓它連選都選不到比較好 */
const managerOptions = computed(() =>
  props.candidates.filter((c) => c.id !== props.employee.id),
)

/** 哪些欄位被改過。決定 fields 清單，也決定「儲存」能不能按 */
const changedFields = computed<EditableField[]>(() =>
  (Object.keys(form.value) as EditableField[]).filter(
    (key) => form.value[key] !== original.value[key],
  ),
)

const isManaged = (field: string): boolean =>
  props.employee.platform_managed_fields.includes(field)

async function save(): Promise<void> {
  if (changedFields.value.length === 0) return

  saving.value = true
  errorMessage.value = ''

  // 只帶改過的欄位。空字串轉 null——後端的 unique index
  // 會把兩個空字串的 employee_no 視為衝突
  const patch: orgApi.EmployeePatch = { fields: changedFields.value }
  for (const field of changedFields.value) {
    const value = form.value[field]
    if (field === 'name' || field === 'status') {
      patch[field] = value
    } else {
      patch[field] = value === '' ? null : value
    }
  }

  try {
    await orgApi.updateEmployee(props.employee.id, patch)
    emit('saved')
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '儲存失敗，請稍後再試'
  } finally {
    saving.value = false
  }
}

/**
 * 改回跟隨來源系統
 *
 * 解除後下一次同步就會覆蓋這個欄位。等同放棄平台上的人工修正，
 * 所以後端會留稽核。
 */
async function unlock(field: string): Promise<void> {
  unlocking.value = field
  errorMessage.value = ''
  try {
    await orgApi.unlockFields('employees', props.employee.id, [field])
    emit('saved')
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '解除鎖定失敗，請稍後再試'
  } finally {
    unlocking.value = ''
  }
}
</script>

<template>
  <div
    class="fixed inset-0 bg-black/40 flex items-center justify-center z-[100]"
    data-testid="employee-dialog"
    @click.self="emit('close')"
  >
    <div
      class="bg-surface-container-lowest rounded-2xl w-[560px] max-h-[85vh] overflow-y-auto shadow-xl"
    >
      <div
        class="px-space-lg py-space-md border-b border-outline-variant flex items-center justify-between"
      >
        <div>
          <h2 class="font-title-sm text-title-sm text-on-surface">
            編輯員工
          </h2>
          <p class="font-label-caption text-label-caption text-secondary mt-0.5">
            {{ employee.email }}
            <span v-if="!employee.can_login">（不可登入）</span>
          </p>
        </div>
        <button
          type="button"
          class="text-secondary hover:text-on-surface"
          data-testid="employee-dialog-close"
          @click="emit('close')"
        >
          <span class="material-symbols-outlined">close</span>
        </button>
      </div>

      <div class="px-space-lg py-space-md flex flex-col gap-space-md">
        <p
          v-if="errorMessage"
          role="alert"
          data-testid="employee-dialog-error"
          class="px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
        >
          {{ errorMessage }}
        </p>

        <div class="grid grid-cols-2 gap-space-md">
          <label class="flex flex-col gap-1">
            <span
              class="font-label-header text-label-header text-secondary flex items-center gap-2"
            >
              姓名
              <ManagedFieldMark
                :managed="isManaged('name')"
                :busy="unlocking === 'name'"
                @unlock="unlock('name')"
              />
            </span>
            <input
              v-model="form.name"
              type="text"
              data-testid="employee-name"
              class="h-9 px-3 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
            />
          </label>

          <label class="flex flex-col gap-1">
            <span
              class="font-label-header text-label-header text-secondary flex items-center gap-2"
            >
              員工編號
              <ManagedFieldMark
                :managed="isManaged('employee_no')"
                :busy="unlocking === 'employee_no'"
                @unlock="unlock('employee_no')"
              />
            </span>
            <input
              v-model="form.employee_no"
              type="text"
              data-testid="employee-no"
              class="h-9 px-3 rounded-lg border border-outline-variant bg-surface-container-lowest font-data-mono text-[12px] text-on-surface focus:outline-none focus:border-primary"
            />
          </label>

          <label class="flex flex-col gap-1">
            <span
              class="font-label-header text-label-header text-secondary flex items-center gap-2"
            >
              所屬部門
              <ManagedFieldMark
                :managed="isManaged('department_id')"
                :busy="unlocking === 'department_id'"
                @unlock="unlock('department_id')"
              />
            </span>
            <select
              v-model="form.department_id"
              data-testid="employee-department"
              class="h-9 px-2 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
            >
              <option value="">（未指定）</option>
              <option v-for="d in departments" :key="d.id" :value="d.id">
                {{ d.name }}
              </option>
            </select>
          </label>

          <label class="flex flex-col gap-1">
            <span
              class="font-label-header text-label-header text-secondary flex items-center gap-2"
            >
              直屬主管
              <ManagedFieldMark
                :managed="isManaged('manager_id')"
                :busy="unlocking === 'manager_id'"
                @unlock="unlock('manager_id')"
              />
            </span>
            <select
              v-model="form.manager_id"
              data-testid="employee-manager"
              class="h-9 px-2 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
            >
              <option value="">（未指定）</option>
              <option v-for="c in managerOptions" :key="c.id" :value="c.id">
                {{ c.name }}
              </option>
            </select>
          </label>

          <label class="flex flex-col gap-1">
            <span
              class="font-label-header text-label-header text-secondary flex items-center gap-2"
            >
              職稱
              <ManagedFieldMark
                :managed="isManaged('job_title')"
                :busy="unlocking === 'job_title'"
                @unlock="unlock('job_title')"
              />
            </span>
            <input
              v-model="form.job_title"
              type="text"
              data-testid="employee-job-title"
              class="h-9 px-3 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
            />
          </label>

          <label class="flex flex-col gap-1">
            <span
              class="font-label-header text-label-header text-secondary flex items-center gap-2"
            >
              狀態
              <ManagedFieldMark
                :managed="isManaged('status')"
                :busy="unlocking === 'status'"
                @unlock="unlock('status')"
              />
            </span>
            <select
              v-model="form.status"
              data-testid="employee-status"
              class="h-9 px-2 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
            >
              <option value="ACTIVE">啟用</option>
              <option value="DISABLED">停用</option>
            </select>
          </label>

          <label class="flex flex-col gap-1">
            <span
              class="font-label-header text-label-header text-secondary flex items-center gap-2"
            >
              電話
              <ManagedFieldMark
                :managed="isManaged('phone')"
                :busy="unlocking === 'phone'"
                @unlock="unlock('phone')"
              />
            </span>
            <input
              v-model="form.phone"
              type="text"
              data-testid="employee-phone"
              class="h-9 px-3 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
            />
          </label>

          <label class="flex flex-col gap-1">
            <span
              class="font-label-header text-label-header text-secondary flex items-center gap-2"
            >
              分機
              <ManagedFieldMark
                :managed="isManaged('extension')"
                :busy="unlocking === 'extension'"
                @unlock="unlock('extension')"
              />
            </span>
            <input
              v-model="form.extension"
              type="text"
              data-testid="employee-extension"
              class="h-9 px-3 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
            />
          </label>
        </div>

        <p class="font-label-caption text-label-caption text-secondary">
          編輯過的欄位會標記為「平台維護」，日後從 ERP 同步時不會被覆蓋。
        </p>
      </div>

      <div
        class="px-space-lg py-space-md border-t border-outline-variant flex items-center justify-end gap-space-sm"
      >
        <button
          type="button"
          class="h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header"
          @click="emit('close')"
        >
          取消
        </button>
        <button
          type="button"
          class="h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 disabled:opacity-40 disabled:cursor-not-allowed"
          :disabled="changedFields.length === 0 || saving"
          data-testid="employee-save"
          @click="save"
        >
          {{ saving ? '儲存中…' : '儲存' }}
        </button>
      </div>
    </div>
  </div>
</template>
