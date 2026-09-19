<script setup lang="ts">
/**
 * 部門的建立與編輯
 *
 * 一個對話框兩種模式：`department` 為 null 時是建立。
 * 拆成兩個元件的話，欄位與驗證要寫兩遍，改一邊忘另一邊的風險
 * 比多一個 if 大。
 *
 * 編輯的欄位語意與 EmployeeEditDialog 相同：只送改過的欄位，
 * 已鎖定的欄位顯示「平台維護」與解鎖按鈕（需求 07 §3 規則 3）。
 *
 * `code` 只在建立時可填。它是 `unique (tenant_id, code)` 的一部分，
 * 也是同步匹配的備援鍵，改了會讓既有的對應關係斷掉——後端的
 * 白名單同樣擋著。
 */
import { computed, ref, watch } from 'vue'
import ManagedFieldMark from './ManagedFieldMark.vue'
import * as orgApi from '@/api/org'
import type { Department, Employee } from '@/api/org'
import { ApiError } from '@/api/types'

const props = defineProps<{
  /** null 代表建立新部門 */
  department: Department | null
  /** 供「上層部門」下拉使用。建立時是全部，編輯時要排除自己與下層 */
  departments: Department[]
  /** 可指派為部門主管的人 */
  employees: Employee[]
  /** 編輯時不可選的部門 id（自己與所有下層），避免成環 */
  excludeIds?: string[]
}>()

const emit = defineEmits<{ close: []; saved: [] }>()

type EditableField = 'name' | 'parent_id' | 'manager_user_id' | 'status'
type FormState = Record<EditableField, string> & { code: string }

function toForm(d: Department | null): FormState {
  return {
    code: d?.code ?? '',
    name: d?.name ?? '',
    parent_id: d?.parent_id ?? '',
    manager_user_id: d?.manager_user_id ?? '',
    status: d?.status ?? 'ACTIVE',
  }
}

const form = ref<FormState>(toForm(props.department))
const original = ref<FormState>(toForm(props.department))
const saving = ref(false)
const unlocking = ref('')
const errorMessage = ref('')

const isCreate = computed(() => props.department === null)

watch(
  () => props.department,
  (d) => {
    form.value = toForm(d)
    original.value = toForm(d)
    errorMessage.value = ''
    // 切到另一個部門時要收起確認，否則會對錯的目標按下刪除
    confirmingDelete.value = false
  },
)

/**
 * 上層部門的選項
 *
 * 排除自己與所有下層——把部門移到自己的下層會成環，
 * 後端也擋，但讓它連選都選不到比較好
 */
const parentOptions = computed(() => {
  const excluded = new Set(props.excludeIds ?? [])
  return props.departments.filter((d) => !excluded.has(d.id))
})

const changedFields = computed<EditableField[]>(() =>
  (['name', 'parent_id', 'manager_user_id', 'status'] as EditableField[]).filter(
    (key) => form.value[key] !== original.value[key],
  ),
)

/** 建立時代碼與名稱都要有，編輯時要有改動 */
const canSave = computed(() =>
  isCreate.value
    ? form.value.code.trim() !== '' && form.value.name.trim() !== ''
    : changedFields.value.length > 0,
)

const isManaged = (field: string): boolean =>
  props.department?.platform_managed_fields.includes(field) ?? false

async function save(): Promise<void> {
  if (!canSave.value) return

  saving.value = true
  errorMessage.value = ''

  try {
    if (isCreate.value) {
      await orgApi.createDepartment({
        code: form.value.code.trim(),
        name: form.value.name.trim(),
        parent_id: form.value.parent_id || null,
        manager_user_id: form.value.manager_user_id || null,
      })
    } else {
      // 只帶改過的欄位。空字串轉 null——「未指定上層部門」
      // 要存成 null 而非空字串
      const patch: orgApi.DepartmentPatch = { fields: changedFields.value }
      for (const field of changedFields.value) {
        const value = form.value[field]
        if (field === 'name' || field === 'status') {
          patch[field] = value
        } else {
          patch[field] = value === '' ? null : value
        }
      }
      await orgApi.updateDepartment(props.department!.id, patch)
    }
    emit('saved')
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '儲存失敗，請稍後再試'
  } finally {
    saving.value = false
  }
}

/**
 * 刪除
 *
 * 需要二次確認：刪除不可逆，而且按鈕就在儲存旁邊。
 * 用元件內的狀態而非 `confirm()`——後者在 headless 測試裡
 * 要另外掛 dialog handler，而且樣式不受控。
 */
const confirmingDelete = ref(false)
const deleting = ref(false)

async function remove(): Promise<void> {
  if (!props.department) return

  deleting.value = true
  errorMessage.value = ''
  try {
    await orgApi.deleteDepartment(props.department.id)
    // 只 emit saved。父層重新載入後找不到這個 id，會自己把
    // 對話框關掉——再 emit close 會讓網址的 query 被清兩次
    emit('saved')
  } catch (error) {
    // 有成員或有子部門時後端會擋下並說明原因
    errorMessage.value =
      error instanceof ApiError ? error.message : '刪除失敗，請稍後再試'
    confirmingDelete.value = false
  } finally {
    deleting.value = false
  }
}

async function unlock(field: string): Promise<void> {
  if (!props.department) return

  unlocking.value = field
  errorMessage.value = ''
  try {
    await orgApi.unlockFields('departments', props.department.id, [field])
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
    data-testid="department-dialog"
    @click.self="emit('close')"
  >
    <div class="bg-surface-container-lowest rounded-2xl w-[480px] shadow-xl">
      <div
        class="px-space-lg py-space-md border-b border-outline-variant flex items-center justify-between"
      >
        <h2 class="font-title-sm text-title-sm text-on-surface">
          {{ isCreate ? '建立部門' : '編輯部門' }}
        </h2>
        <button
          type="button"
          class="text-secondary hover:text-on-surface"
          data-testid="department-dialog-close"
          @click="emit('close')"
        >
          <span class="material-symbols-outlined">close</span>
        </button>
      </div>

      <div class="px-space-lg py-space-md flex flex-col gap-space-md">
        <p
          v-if="errorMessage"
          role="alert"
          data-testid="department-dialog-error"
          class="px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
        >
          {{ errorMessage }}
        </p>

        <label class="flex flex-col gap-1">
          <span class="font-label-header text-label-header text-secondary">
            部門代碼
          </span>
          <input
            v-model="form.code"
            type="text"
            :disabled="!isCreate"
            data-testid="department-code"
            class="h-9 px-3 rounded-lg border border-outline-variant bg-surface-container-lowest font-data-mono text-[12px] text-on-surface focus:outline-none focus:border-primary disabled:bg-surface-container-low disabled:text-secondary"
          />
          <span
            v-if="!isCreate"
            class="font-label-caption text-label-caption text-secondary"
          >
            代碼建立後不可變更——它是同步比對的依據
          </span>
        </label>

        <label class="flex flex-col gap-1">
          <span
            class="font-label-header text-label-header text-secondary flex items-center gap-2"
          >
            部門名稱
            <ManagedFieldMark
              :managed="isManaged('name')"
              :busy="unlocking === 'name'"
              @unlock="unlock('name')"
            />
          </span>
          <input
            v-model="form.name"
            type="text"
            data-testid="department-name"
            class="h-9 px-3 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
          />
        </label>

        <label class="flex flex-col gap-1">
          <span
            class="font-label-header text-label-header text-secondary flex items-center gap-2"
          >
            上層部門
            <ManagedFieldMark
              :managed="isManaged('parent_id')"
              :busy="unlocking === 'parent_id'"
              @unlock="unlock('parent_id')"
            />
          </span>
          <select
            v-model="form.parent_id"
            data-testid="department-parent"
            class="h-9 px-2 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
          >
            <option value="">（最上層）</option>
            <option v-for="d in parentOptions" :key="d.id" :value="d.id">
              {{ d.name }}
            </option>
          </select>
        </label>

        <label class="flex flex-col gap-1">
          <span
            class="font-label-header text-label-header text-secondary flex items-center gap-2"
          >
            部門主管
            <ManagedFieldMark
              :managed="isManaged('manager_user_id')"
              :busy="unlocking === 'manager_user_id'"
              @unlock="unlock('manager_user_id')"
            />
          </span>
          <select
            v-model="form.manager_user_id"
            data-testid="department-manager"
            class="h-9 px-2 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
          >
            <option value="">（未指定）</option>
            <option v-for="e in employees" :key="e.id" :value="e.id">
              {{ e.name }}
            </option>
          </select>
          <!--
            department_manager_of 有 m.id <> u.id，主管本人隸屬於
            自己管的部門時，他送單會解析不到簽核人。組織健康檢查
            會把這種情況報成 DEPARTMENT_MANAGER_IS_SELF
          -->
          <span class="font-label-caption text-label-caption text-secondary">
            主管若隸屬於本部門，他自己送單時會找不到簽核人
          </span>
        </label>

        <label v-if="!isCreate" class="flex flex-col gap-1">
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
            data-testid="department-status"
            class="h-9 px-2 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
          >
            <option value="ACTIVE">啟用</option>
            <option value="INACTIVE">停用</option>
          </select>
        </label>
      </div>

      <div
        class="px-space-lg py-space-md border-t border-outline-variant flex items-center justify-between gap-space-sm"
      >
        <!-- 刪除放在左邊，與儲存分開——它不可逆，不該緊鄰主要動作 -->
        <div v-if="!isCreate" class="flex items-center gap-space-sm">
          <template v-if="confirmingDelete">
            <span class="font-label-caption text-label-caption text-error">
              確定刪除？
            </span>
            <button
              type="button"
              class="h-8 px-3 rounded-lg bg-error text-on-error font-label-header text-label-header hover:opacity-90 disabled:opacity-40"
              :disabled="deleting"
              data-testid="department-delete-confirm"
              @click="remove"
            >
              {{ deleting ? '刪除中…' : '確定' }}
            </button>
            <button
              type="button"
              class="h-8 px-2 rounded-lg text-secondary hover:bg-surface-container-low font-label-header text-label-header"
              @click="confirmingDelete = false"
            >
              取消
            </button>
          </template>
          <button
            v-else
            type="button"
            class="h-8 px-3 rounded-lg border border-outline-variant text-error hover:bg-error-container font-label-header text-label-header"
            data-testid="department-delete"
            @click="confirmingDelete = true"
          >
            刪除
          </button>
        </div>
        <div v-else />

        <div class="flex items-center gap-space-sm">
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
          :disabled="!canSave || saving"
          data-testid="department-save"
          @click="save"
        >
          {{ saving ? '儲存中…' : isCreate ? '建立' : '儲存' }}
        </button>
        </div>
      </div>
    </div>
  </div>
</template>
