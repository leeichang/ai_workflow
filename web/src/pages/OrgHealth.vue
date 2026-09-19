<script setup lang="ts">
/**
 * 組織健康檢查
 *
 * 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §9。
 *
 * 這一頁回答的不是「組織資料完不完整」，而是
 * **哪些人現在送單會卡住**。導入期的價值在於問題上線前就浮出來，
 * 而不是使用者送單後才卡住。
 *
 * 因此版面刻意把 HIGH 放在最前面、用後果而非現象當說明文字。
 * 「沒有主管」不會讓人想修，「送單會卡住」才會。
 */
import { computed, onMounted, ref } from 'vue'
import AppShell from '@/components/AppShell.vue'
import * as orgApi from '@/api/org'
import type { HealthReport, OrgIssue, Severity } from '@/api/org'
import { ApiError } from '@/api/types'

const report = ref<HealthReport | null>(null)
const loading = ref(true)
const errorMessage = ref('')

/** 只看 HIGH。導入期通常先把會卡住的修完再管其他 */
const highOnly = ref(false)

/** 對象類型的中文。未知值原樣顯示，不要硬塞一個預設 */
const SUBJECT_LABEL: Record<string, string> = {
  employee: '員工',
  department: '部門',
  role: '角色',
}

const SEVERITY_STYLE: Record<Severity, { text: string; class: string }> = {
  HIGH: { text: '會卡住', class: 'bg-error-container text-on-error-container' },
  MEDIUM: { text: '功能受損', class: 'bg-[#FEF3C7] text-[#92400E]' },
}

const visibleIssues = computed<OrgIssue[]>(() => {
  const issues = report.value?.issues ?? []
  return highOnly.value ? issues.filter((i) => i.severity === 'HIGH') : issues
})

/** 全部健康。與「還沒載入」要分得出來，否則使用者會以為頁面壞了 */
const isHealthy = computed(
  () => report.value !== null && report.value.issues.length === 0,
)

/**
 * 「前往修正」的目的地
 *
 * 報表要修得掉才有用，不該讓使用者自己再去找那個人或那個部門。
 * 員工與部門各用一個參數，因為兩者開的是不同的對話框。
 */
function fixLink(issue: OrgIssue) {
  const key = issue.subject_type === 'department' ? 'department' : 'employee'
  return {
    path: '/designer/organization',
    query: { [key]: issue.subject_id },
  }
}

/**
 * 角色成員沒有管理畫面
 *
 * `ROLE_HAS_NO_MEMBERS` 的 subject 是角色，不是人或部門。
 * 組織管理頁沒有「編輯角色成員」，指過去只會開一個不存在的
 * 員工編輯然後靜默失敗——那比沒有連結更糟。
 *
 * 要修這一項得改 seed 或用 SQL 指派 user_role。等角色管理 UI
 * 做出來之前，先讓使用者看到說明而不是一個壞掉的按鈕。
 */
function hasFixLink(issue: OrgIssue): boolean {
  return issue.subject_type !== 'role'
}

async function load(): Promise<void> {
  loading.value = true
  errorMessage.value = ''
  try {
    report.value = await orgApi.health()
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法執行檢查，請稍後再試'
  } finally {
    loading.value = false
  }
}

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-on-surface font-semibold">組織健康檢查</span>
    </template>

    <div class="max-w-[1000px]">
      <div class="flex items-center justify-between mb-space-lg">
        <div>
          <h1 class="font-title-md text-title-md text-on-surface">組織健康檢查</h1>
          <p class="font-body-dense text-body-dense text-secondary mt-1">
            列出會讓簽核流程卡住的組織資料問題
          </p>
        </div>
        <button
          type="button"
          class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
          data-testid="org-health-refresh"
          @click="load"
        >
          <span class="material-symbols-outlined text-[16px]">refresh</span>
          重新檢查
        </button>
      </div>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="org-health-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <div
        v-if="loading"
        data-testid="org-health-loading"
        class="py-space-xl text-center font-body-dense text-body-dense text-secondary"
      >
        檢查中…
      </div>

      <template v-else-if="report">
        <!-- 統計。分母讓「3 個問題」有比例可言 -->
        <div class="grid grid-cols-4 gap-space-md mb-space-lg">
          <div
            class="px-space-md py-space-sm rounded-xl border border-outline-variant bg-surface-container-lowest"
            data-testid="org-health-high"
          >
            <p class="font-label-caption text-label-caption text-secondary">會卡住</p>
            <p
              class="font-title-md text-title-md mt-1"
              :class="report.high_count > 0 ? 'text-error' : 'text-on-surface'"
            >
              {{ report.high_count }}
            </p>
          </div>
          <div
            class="px-space-md py-space-sm rounded-xl border border-outline-variant bg-surface-container-lowest"
            data-testid="org-health-medium"
          >
            <p class="font-label-caption text-label-caption text-secondary">功能受損</p>
            <p class="font-title-md text-title-md text-on-surface mt-1">
              {{ report.medium_count }}
            </p>
          </div>
          <div
            class="px-space-md py-space-sm rounded-xl border border-outline-variant bg-surface-container-lowest"
            data-testid="org-health-employees"
          >
            <p class="font-label-caption text-label-caption text-secondary">在職員工</p>
            <p class="font-title-md text-title-md text-on-surface mt-1">
              {{ report.employee_count }}
            </p>
          </div>
          <div
            class="px-space-md py-space-sm rounded-xl border border-outline-variant bg-surface-container-lowest"
            data-testid="org-health-departments"
          >
            <p class="font-label-caption text-label-caption text-secondary">部門</p>
            <p class="font-title-md text-title-md text-on-surface mt-1">
              {{ report.department_count }}
            </p>
          </div>
        </div>

        <div
          v-if="isHealthy"
          data-testid="org-health-empty"
          class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
        >
          <span class="material-symbols-outlined text-[40px] text-[#15803D]">
            check_circle
          </span>
          <p class="font-body-dense text-body-dense">
            沒有發現問題。所有人的簽核人都解析得到
          </p>
        </div>

        <template v-else>
          <label
            class="flex items-center gap-2 mb-space-md font-body-dense text-body-dense text-on-surface-variant cursor-pointer w-fit"
          >
            <input
              v-model="highOnly"
              type="checkbox"
              data-testid="org-health-high-only"
              class="accent-primary"
            />
            只看會卡住的問題
          </label>

          <table
            class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
            data-testid="org-health-table"
          >
            <thead>
              <tr class="bg-surface-container-low border-b border-outline-variant">
                <th
                  class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary w-[90px]"
                >
                  嚴重度
                </th>
                <th
                  class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary w-[140px]"
                >
                  對象
                </th>
                <th
                  class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary"
                >
                  問題
                </th>
                <th
                  class="text-right px-space-md py-space-sm font-label-header text-label-header text-secondary w-[90px]"
                >
                  &nbsp;
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="issue in visibleIssues"
                :key="`${issue.code}-${issue.subject_id}`"
                :data-testid="`org-health-row-${issue.code}-${issue.subject_id}`"
                class="border-b border-outline-variant last:border-b-0 hover:bg-surface-container-low transition-colors"
              >
                <td class="px-space-md py-space-sm align-top">
                  <span
                    class="inline-block px-2 py-0.5 rounded font-label-caption text-label-caption whitespace-nowrap"
                    :class="SEVERITY_STYLE[issue.severity].class"
                  >
                    {{ SEVERITY_STYLE[issue.severity].text }}
                  </span>
                </td>
                <td class="px-space-md py-space-sm align-top">
                  <p class="font-body-dense text-body-dense text-on-surface">
                    {{ issue.subject_name }}
                  </p>
                  <p class="font-label-caption text-label-caption text-secondary mt-0.5">
                    {{ SUBJECT_LABEL[issue.subject_type] ?? issue.subject_type }}
                  </p>
                </td>
                <td class="px-space-md py-space-sm align-top">
                  <p class="font-body-dense text-body-dense text-on-surface">
                    {{ issue.message }}
                  </p>
                  <p
                    v-if="issue.detail"
                    class="font-label-caption text-label-caption text-secondary mt-0.5"
                  >
                    {{ issue.detail }}
                  </p>
                </td>
                <td class="px-space-md py-space-sm text-right align-top">
                  <!--
                    報表要修得掉才有用。員工的問題直接開該員工的編輯，
                    部門的問題先跳到組織管理讓使用者找得到它
                  -->
                  <RouterLink
                    v-if="hasFixLink(issue)"
                    :to="fixLink(issue)"
                    class="h-7 px-2 inline-flex items-center rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header whitespace-nowrap"
                    :data-testid="`org-health-fix-${issue.subject_id}`"
                  >
                    前往修正
                  </RouterLink>
                </td>
              </tr>
            </tbody>
          </table>
        </template>
      </template>
    </div>
  </AppShell>
</template>
