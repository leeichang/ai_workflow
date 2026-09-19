<script setup lang="ts">
/**
 * 組織管理
 *
 * 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §12.2。
 *
 * 左邊部門樹、右邊員工清單。選部門會連同下層部門的人一起顯示——
 * 只顯示直屬成員會讓上層部門看起來幾乎是空的。
 *
 * 組織健康檢查抓到的問題要在這裡修得掉，所以從健康檢查頁過來時
 * 帶 `?employee=<id>` 直接開啟該員工的編輯。
 */
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import DepartmentTree from '@/components/DepartmentTree.vue'
import EmployeeEditDialog from '@/components/EmployeeEditDialog.vue'
import * as orgApi from '@/api/org'
import type { Department, Employee } from '@/api/org'
import { buildTree, descendantIds } from '@/org/tree'
import { ApiError } from '@/api/types'
import { useSession } from '@/auth/useSession'

const route = useRoute()
const router = useRouter()
const { hasRole } = useSession()

const departments = ref<Department[]>([])
const employees = ref<Employee[]>([])
const loading = ref(true)
const errorMessage = ref('')

const selectedDept = ref<string | null>(null)
const keyword = ref('')
const includeInactive = ref(false)
const editing = ref<Employee | null>(null)

/**
 * 組織編輯要 admin。沒有的話只能看
 *
 * 包成 computed 而非直接呼叫 hasRole：後者是一般函式，
 * 登入狀態改變時畫面不會跟著更新
 */
const canEdit = computed(() => hasRole('admin'))

const tree = computed(() => buildTree(departments.value))

/**
 * 篩選後的員工
 *
 * 部門篩選在前端做：後端的 department_id 只收單一值，
 * 而選取父部門時要顯示整棵子樹的人。
 */
const visibleEmployees = computed<Employee[]>(() => {
  if (selectedDept.value === null) return employees.value

  const ids = new Set(descendantIds(tree.value, selectedDept.value))
  return employees.value.filter(
    (e) => e.department_id !== null && ids.has(e.department_id),
  )
})

async function load(): Promise<void> {
  loading.value = true
  errorMessage.value = ''
  try {
    // 兩支 API 互不相依，平行發出
    const [depts, emps] = await Promise.all([
      orgApi.listDepartments(),
      orgApi.listEmployees({
        q: keyword.value || undefined,
        include_inactive: includeInactive.value,
      }),
    ])
    departments.value = depts
    employees.value = emps
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法載入組織資料，請稍後再試'
  } finally {
    loading.value = false
  }
}

/** 關鍵字與「含離職」要重新查——篩選在後端做，前端沒有全部資料 */
watch([keyword, includeInactive], load)

/**
 * 從健康檢查頁帶 employee 參數進來時直接開編輯
 *
 * 報表指向的人必須點得到才修得了，不該讓使用者自己再找一次
 */
watch([() => route.query.employee, employees], () => {
  const id = route.query.employee
  if (typeof id !== 'string') return

  const found = employees.value.find((e) => e.id === id)
  if (found) editing.value = found
})

function closeDialog(): void {
  editing.value = null
  // 清掉 query，否則重新整理又會跳出對話框
  if (route.query.employee) {
    router.replace({ query: {} })
  }
}

async function onSaved(): Promise<void> {
  await load()
  // 重新載入後要用新的資料更新對話框，否則欄位鎖定標記還是舊的
  if (editing.value) {
    const refreshed = employees.value.find((e) => e.id === editing.value!.id)
    editing.value = refreshed ?? null
  }
}

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-on-surface font-semibold">組織管理</span>
    </template>

    <div class="max-w-[1200px]">
      <div class="flex items-center justify-between mb-space-lg">
        <div>
          <h1 class="font-title-md text-title-md text-on-surface">組織管理</h1>
          <p class="font-body-dense text-body-dense text-secondary mt-1">
            部門與人員。簽核人依這裡的主管關係解析
          </p>
        </div>
        <div class="flex items-center gap-space-sm">
          <RouterLink
            to="/designer/org-health"
            class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
            data-testid="org-goto-health"
          >
            <span class="material-symbols-outlined text-[16px]">
              health_and_safety
            </span>
            健康檢查
          </RouterLink>
          <button
            type="button"
            class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
            data-testid="org-refresh"
            @click="load"
          >
            <span class="material-symbols-outlined text-[16px]">refresh</span>
            重新整理
          </button>
        </div>
      </div>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="org-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <p
        v-if="!canEdit"
        data-testid="org-readonly-hint"
        class="mb-space-md px-3 py-2 rounded-lg bg-surface-container-low text-secondary font-body-dense text-body-dense"
      >
        目前為唯讀。組織異動需要管理員權限——改動主管關係會改變簽核路徑。
      </p>

      <div
        v-if="loading"
        data-testid="org-loading"
        class="py-space-xl text-center font-body-dense text-body-dense text-secondary"
      >
        載入中…
      </div>

      <div v-else class="flex gap-space-lg items-start">
        <!-- 部門樹 -->
        <aside
          class="w-[260px] shrink-0 bg-surface-container-lowest border border-outline-variant rounded-xl p-space-sm"
          data-testid="org-dept-tree"
        >
          <p
            class="px-space-sm py-1 font-label-header text-label-header text-secondary"
          >
            部門（{{ departments.length }}）
          </p>
          <button
            type="button"
            class="w-full text-left px-space-sm py-1.5 rounded-lg font-body-dense text-body-dense transition-colors"
            :class="
              selectedDept === null
                ? 'bg-primary-container text-on-primary font-semibold'
                : 'text-on-surface-variant hover:bg-surface-container-low'
            "
            data-testid="dept-node-all"
            @click="selectedDept = null"
          >
            全部
          </button>
          <DepartmentTree
            :nodes="tree"
            :selected-id="selectedDept"
            @select="selectedDept = $event"
          />
        </aside>

        <!-- 員工清單 -->
        <section class="flex-1 min-w-0">
          <div class="flex items-center gap-space-md mb-space-md">
            <input
              v-model="keyword"
              type="search"
              placeholder="搜尋姓名、email 或員工編號"
              data-testid="org-search"
              class="flex-1 h-9 px-3 rounded-lg border border-outline-variant bg-surface-container-lowest font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary"
            />
            <label
              class="flex items-center gap-2 font-body-dense text-body-dense text-on-surface-variant cursor-pointer whitespace-nowrap"
            >
              <input
                v-model="includeInactive"
                type="checkbox"
                data-testid="org-include-inactive"
                class="accent-primary"
              />
              含已離職
            </label>
          </div>

          <div
            v-if="visibleEmployees.length === 0"
            data-testid="org-employees-empty"
            class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
          >
            <span class="material-symbols-outlined text-[40px] text-outline">
              group_off
            </span>
            <p class="font-body-dense text-body-dense">這個範圍沒有員工</p>
          </div>

          <table
            v-else
            class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
            data-testid="org-employee-table"
          >
            <thead>
              <tr class="bg-surface-container-low border-b border-outline-variant">
                <th
                  class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
                >
                  員工
                </th>
                <th
                  class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
                >
                  部門 / 職稱
                </th>
                <th
                  class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
                >
                  直屬主管
                </th>
                <th
                  class="text-right px-space-md py-space-sm font-label-header text-label-header text-secondary w-[80px]"
                >
                  &nbsp;
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="emp in visibleEmployees"
                :key="emp.id"
                :data-testid="`org-employee-row-${emp.id}`"
                class="border-b border-outline-variant last:border-b-0 hover:bg-surface-container-low transition-colors"
              >
                <td class="px-space-md py-space-sm">
                  <p class="font-body-dense text-body-dense text-on-surface">
                    {{ emp.name }}
                    <span
                      v-if="emp.left_at"
                      class="ml-1 px-1 rounded bg-surface-container-low text-secondary font-label-caption text-label-caption"
                    >
                      已離職
                    </span>
                    <!-- 不可登入的人仍要在清單裡：他可能是別人的主管 -->
                    <span
                      v-else-if="!emp.can_login"
                      class="ml-1 px-1 rounded bg-surface-container-low text-secondary font-label-caption text-label-caption"
                    >
                      不可登入
                    </span>
                  </p>
                  <p
                    class="font-label-caption text-label-caption text-secondary mt-0.5"
                  >
                    {{ emp.employee_no || '—' }} · {{ emp.email }}
                  </p>
                </td>
                <td class="px-space-md py-space-sm">
                  <p class="font-body-dense text-body-dense text-on-surface">
                    {{ emp.department_name ?? '（未指定）' }}
                  </p>
                  <p
                    class="font-label-caption text-label-caption text-secondary mt-0.5"
                  >
                    {{ emp.job_title || '—' }}
                  </p>
                </td>
                <td class="px-space-md py-space-sm">
                  <span
                    v-if="emp.manager_name"
                    class="font-body-dense text-body-dense text-on-surface"
                  >
                    {{ emp.manager_name }}
                  </span>
                  <!-- 沒有主管代表 manager_of 會解析回空，流程卡住 -->
                  <span
                    v-else
                    class="font-body-dense text-body-dense text-error"
                    :data-testid="`org-no-manager-${emp.id}`"
                  >
                    未指定
                  </span>
                </td>
                <td class="px-space-md py-space-sm text-right">
                  <button
                    v-if="canEdit"
                    type="button"
                    class="h-7 px-2 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header"
                    :data-testid="`org-edit-${emp.id}`"
                    @click="editing = emp"
                  >
                    編輯
                  </button>
                </td>
              </tr>
            </tbody>
          </table>
        </section>
      </div>
    </div>

    <EmployeeEditDialog
      v-if="editing"
      :employee="editing"
      :departments="departments"
      :candidates="employees"
      @close="closeDialog"
      @saved="onSaved"
    />
  </AppShell>
</template>
