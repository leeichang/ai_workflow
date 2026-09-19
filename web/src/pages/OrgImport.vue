<script setup lang="ts">
/**
 * 組織資料匯入
 *
 * 依據 docs/需求規劃/202609/07_組織基本資料與客戶系統同步需求.md §8、§12.2。
 *
 * 四個步驟，刻意不能跳：
 *
 *   1. 上傳 CSV
 *   2. 確認欄位對應（AI 建議只是省打字，人工確認才是關鍵）
 *   3. **試跑**——§12.1 說「preview 是必備，不是加分項。
 *      沒有試跑，管理員第一次同步就是盲賭」
 *   4. 正式寫入
 *
 * 第 3 步不可跳過：正式寫入的按鈕要等試跑完成才會出現。
 *
 * ## 只收 CSV
 *
 * 需求寫的是「Excel / CSV」，本輪只做 CSV。說明文字要明講
 * 「請從 Excel 另存為 CSV」，而不是讓人上傳 xlsx 之後才拿到
 * 看不懂的錯誤。
 */
import { computed, onMounted, ref } from 'vue'
import AppShell from '@/components/AppShell.vue'
import MappingTable from '@/components/MappingTable.vue'
import * as integrationApi from '@/api/integration'
import type { AnalyzeResult, SyncOutcome } from '@/api/integration'
import { ApiError } from '@/api/types'
import { useSession } from '@/auth/useSession'

const { hasRole } = useSession()
const canImport = computed(() => hasRole('admin'))

/** canonical 欄位。與後端 domain::field_mapping::EMPLOYEE_FIELDS 一致 */
const EMPLOYEE_OPTIONS = [
  { value: 'employee_no', label: '員工編號' },
  { value: 'name', label: '姓名' },
  { value: 'email', label: '電子郵件' },
  { value: 'department_code', label: '部門代碼' },
  { value: 'manager_employee_no', label: '主管員工編號' },
  { value: 'job_title', label: '職稱' },
  { value: 'phone', label: '電話' },
  { value: 'extension', label: '分機' },
  { value: 'hired_at', label: '到職日' },
  { value: 'left_at', label: '離職日' },
  { value: 'external_id', label: '來源系統主鍵' },
]

const fileName = ref('')
const content = ref('')
const analysis = ref<AnalyzeResult | null>(null)
const mapping = ref<Record<string, string>>({})
const sourceId = ref<string | null>(null)
const previewResult = ref<SyncOutcome | null>(null)
const syncResult = ref<SyncOutcome | null>(null)

const busy = ref('')
const errorMessage = ref('')

/** 員工編號是匹配鍵。沒有它，同步兩次會產生兩份資料 */
const hasEmployeeNo = computed(() =>
  Object.values(mapping.value).includes('employee_no'),
)

/** 正式寫入要等試跑完成，且試跑沒有被完整性閘擋下 */
const canCommit = computed(
  () =>
    previewResult.value !== null &&
    previewResult.value.status !== 'ABORTED_INCOMPLETE' &&
    syncResult.value === null,
)

async function onFile(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return

  reset()
  fileName.value = file.name
  busy.value = 'analyze'
  errorMessage.value = ''

  try {
    content.value = await file.text()
    analysis.value = await integrationApi.analyze(content.value, 'employee')

    // 把建議填進對應表。猜不出來的留空，管理員自己選
    const initial: Record<string, string> = {}
    for (const s of analysis.value.suggestions) {
      initial[s.source_field] = s.canonical_field ?? ''
    }
    mapping.value = initial
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法讀取檔案，請確認是 CSV 格式'
    analysis.value = null
  } finally {
    busy.value = ''
  }
}

function reset(): void {
  analysis.value = null
  mapping.value = {}
  previewResult.value = null
  syncResult.value = null
  errorMessage.value = ''
}

/**
 * 取得員工資料的來源，沒有就建立
 *
 * **必須重用既有的來源**，不能每次上傳都建新的：
 *
 *   - 資料庫有 `unique (tenant_id, connection_id, dataset)`，
 *     第二次會撞唯一約束
 *   - 更危險的是，就算擋得過，新來源沒有 `last_success_count`，
 *     完整性閘會被當成「首次匯入」而永遠不生效。
 *     那是最糟的失效方式——它安靜，管理員不會發現保護沒了
 *
 * 重用時更新 mapping：客戶下次匯出的欄位可能不一樣。
 */
async function ensureSource(): Promise<string> {
  // 只保留有對應的欄位。空字串代表「不匯入這一欄」
  const effective = Object.fromEntries(
    Object.entries(mapping.value).filter(([, target]) => target !== ''),
  )

  const sources = await integrationApi.listSources()
  const existing = sources.find((s) => s.dataset === 'employee')

  if (existing) {
    await integrationApi.updateSource(existing.id, { mapping: effective })
    return existing.id
  }

  const connections = await integrationApi.listConnections()
  const connectionId =
    connections.find((c) => c.kind === 'FILE')?.id ??
    (await integrationApi.createConnection('檔案匯入')).id

  const created = await integrationApi.createSource({
    connection_id: connectionId,
    dataset: 'employee',
    name: '員工主檔',
    mapping: effective,
  })
  return created.id
}

/**
 * 確認對應並試跑
 *
 * 來源（integration_source）在這一步建立或重用。mapping 固化在
 * 它上面，之後的同步不必再對一次欄位。
 */
async function runPreview(): Promise<void> {
  busy.value = 'preview'
  errorMessage.value = ''

  try {
    if (sourceId.value === null) {
      sourceId.value = await ensureSource()
    }

    previewResult.value = await integrationApi.preview(
      sourceId.value,
      content.value,
    )
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '試跑失敗，請稍後再試'
  } finally {
    busy.value = ''
  }
}

async function commit(): Promise<void> {
  if (!sourceId.value) return

  busy.value = 'sync'
  errorMessage.value = ''

  try {
    syncResult.value = await integrationApi.sync(sourceId.value, content.value)
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '同步失敗，請稍後再試'
  } finally {
    busy.value = ''
  }
}

/** 顯示用的計數項。順序即閱讀順序：先看寫了什麼，再看沒寫什麼 */
function countItems(outcome: SyncOutcome) {
  return [
    { label: '讀到', value: outcome.counts.received_count, warn: false },
    { label: '通過驗證', value: outcome.counts.validated_count, warn: false },
    { label: '新增', value: outcome.counts.inserted_count, warn: false },
    { label: '更新', value: outcome.counts.updated_count, warn: false },
    { label: '未變動', value: outcome.counts.unchanged_count, warn: false },
    {
      label: '被擋下',
      value: outcome.counts.rejected_count,
      warn: outcome.counts.rejected_count > 0,
    },
    {
      label: '平台維護跳過',
      value: outcome.counts.skipped_by_ownership_count,
      warn: false,
    },
    {
      label: '來源消失',
      value: outcome.counts.missing_in_source_count,
      warn: outcome.counts.missing_in_source_count > 0,
    },
  ]
}

const STATUS_LABEL: Record<string, { text: string; class: string }> = {
  SUCCESS: { text: '成功', class: 'bg-[#DCFCE7] text-[#15803D]' },
  PARTIAL_SUCCESS: { text: '部分成功', class: 'bg-[#FEF3C7] text-[#92400E]' },
  ABORTED_INCOMPLETE: {
    text: '整批中止',
    class: 'bg-error-container text-on-error-container',
  },
  FAILED: { text: '失敗', class: 'bg-error-container text-on-error-container' },
}

onMounted(() => {
  // 進來就重置。上一次的結果留著會讓人以為是這次的
  reset()
})
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-on-surface font-semibold">組織資料匯入</span>
    </template>

    <div class="max-w-[900px]">
      <div class="mb-space-lg">
        <h1 class="font-title-md text-title-md text-on-surface">組織資料匯入</h1>
        <p class="font-body-dense text-body-dense text-secondary mt-1">
          從人事系統匯出的檔案批次更新員工資料
        </p>
      </div>

      <p
        v-if="!canImport"
        data-testid="import-readonly-hint"
        class="mb-space-md px-3 py-2 rounded-lg bg-surface-container-low text-secondary font-body-dense text-body-dense"
      >
        匯入需要管理員權限——它會改動整份組織資料，而組織決定簽核路徑。
      </p>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="import-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <!-- 步驟 1：上傳 -->
      <section
        class="mb-space-lg p-space-md bg-surface-container-lowest border border-outline-variant rounded-xl"
      >
        <h2 class="font-label-header text-label-header text-secondary mb-space-sm">
          步驟 1　選擇檔案
        </h2>
        <input
          type="file"
          accept=".csv,text/csv"
          :disabled="!canImport || busy !== ''"
          data-testid="import-file"
          class="font-body-dense text-body-dense text-on-surface"
          @change="onFile"
        />
        <!-- 本輪只做 CSV。說明要明講，不要讓人上傳 xlsx 才拿到錯誤 -->
        <p class="font-label-caption text-label-caption text-secondary mt-space-sm">
          目前支援 CSV。Excel 請用「另存新檔」選擇 CSV UTF-8 格式。
          檔案第一列必須是欄位名稱。
        </p>
      </section>

      <!-- 步驟 2：欄位對應 -->
      <section
        v-if="analysis"
        class="mb-space-lg"
        data-testid="import-mapping-section"
      >
        <div class="flex items-baseline justify-between mb-space-sm">
          <h2 class="font-label-header text-label-header text-secondary">
            步驟 2　確認欄位對應
          </h2>
          <p class="font-label-caption text-label-caption text-secondary">
            讀到 {{ analysis.row_count }} 筆資料
          </p>
        </div>

        <MappingTable
          v-model="mapping"
          :headers="analysis.headers"
          :suggestions="analysis.suggestions"
          :sample-rows="analysis.sample_rows"
          :options="EMPLOYEE_OPTIONS"
        />

        <p
          v-if="!hasEmployeeNo"
          data-testid="import-no-employee-no"
          class="mt-space-sm px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
        >
          必須對應員工編號。沒有它，系統無法判斷每一列是新人還是既有員工，
          匯入兩次會產生兩份資料。
        </p>

        <button
          type="button"
          class="mt-space-md h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 disabled:opacity-40 disabled:cursor-not-allowed"
          :disabled="!hasEmployeeNo || busy !== '' || !canImport"
          data-testid="import-preview"
          @click="runPreview"
        >
          {{ busy === 'preview' ? '試跑中…' : '試跑（不會寫入）' }}
        </button>
      </section>

      <!-- 步驟 3：試跑結果 -->
      <section
        v-if="previewResult"
        class="mb-space-lg p-space-md bg-surface-container-lowest border border-outline-variant rounded-xl"
        data-testid="import-preview-result"
      >
        <div class="flex items-center gap-space-sm mb-space-md">
          <h2 class="font-label-header text-label-header text-secondary">
            步驟 3　試跑結果
          </h2>
          <span
            class="px-2 py-0.5 rounded font-label-caption text-label-caption"
            :class="STATUS_LABEL[previewResult.status].class"
            data-testid="import-preview-status"
          >
            {{ STATUS_LABEL[previewResult.status].text }}
          </span>
        </div>

        <p
          v-if="previewResult.message"
          class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
          data-testid="import-preview-message"
        >
          {{ previewResult.message }}
        </p>

        <div class="grid grid-cols-4 gap-space-sm mb-space-md">
          <div
            v-for="item in countItems(previewResult)"
            :key="item.label"
            class="px-space-sm py-1.5 rounded-lg bg-surface-container-low"
          >
            <p class="font-label-caption text-label-caption text-secondary">
              {{ item.label }}
            </p>
            <p
              class="font-title-sm text-title-sm mt-0.5"
              :class="item.warn ? 'text-error' : 'text-on-surface'"
            >
              {{ item.value }}
            </p>
          </div>
        </div>

        <details v-if="previewResult.issues.length > 0" class="mb-space-md">
          <summary
            class="cursor-pointer font-body-dense text-body-dense text-on-surface-variant"
            data-testid="import-issues-toggle"
          >
            {{ previewResult.issues.length }} 個問題
          </summary>
          <ul class="mt-space-sm flex flex-col gap-1">
            <li
              v-for="(issue, index) in previewResult.issues.slice(0, 50)"
              :key="index"
              class="font-body-dense text-body-dense text-secondary"
            >
              <span v-if="issue.row_number" class="font-data-mono text-[11px]">
                第 {{ issue.row_number }} 列：
              </span>
              {{ issue.message }}
            </li>
          </ul>
        </details>

        <button
          type="button"
          class="h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 disabled:opacity-40 disabled:cursor-not-allowed"
          :disabled="!canCommit || busy !== ''"
          data-testid="import-commit"
          @click="commit"
        >
          {{ busy === 'sync' ? '寫入中…' : '確認寫入' }}
        </button>
        <p
          v-if="previewResult.status === 'ABORTED_INCOMPLETE'"
          class="mt-space-sm font-label-caption text-label-caption text-error"
        >
          完整性檢查未通過，不能寫入。請確認匯出的檔案是完整的。
        </p>
      </section>

      <!-- 步驟 4：寫入結果 -->
      <section
        v-if="syncResult"
        class="p-space-md bg-surface-container-lowest border border-outline-variant rounded-xl"
        data-testid="import-sync-result"
      >
        <div class="flex items-center gap-space-sm mb-space-md">
          <h2 class="font-label-header text-label-header text-secondary">
            步驟 4　已寫入
          </h2>
          <span
            class="px-2 py-0.5 rounded font-label-caption text-label-caption"
            :class="STATUS_LABEL[syncResult.status].class"
            data-testid="import-sync-status"
          >
            {{ STATUS_LABEL[syncResult.status].text }}
          </span>
        </div>

        <div class="grid grid-cols-4 gap-space-sm">
          <div
            v-for="item in countItems(syncResult)"
            :key="item.label"
            class="px-space-sm py-1.5 rounded-lg bg-surface-container-low"
          >
            <p class="font-label-caption text-label-caption text-secondary">
              {{ item.label }}
            </p>
            <p
              class="font-title-sm text-title-sm mt-0.5"
              :class="item.warn ? 'text-error' : 'text-on-surface'"
            >
              {{ item.value }}
            </p>
          </div>
        </div>

        <!-- 來源消失的人沒有被停用，管理員要知道下一步 -->
        <p
          v-if="syncResult.counts.missing_in_source_count > 0"
          class="mt-space-md px-3 py-2 rounded-lg bg-[#FEF3C7] text-[#92400E] font-body-dense text-body-dense"
          data-testid="import-missing-hint"
        >
          有 {{ syncResult.counts.missing_in_source_count }} 人在這次的檔案裡找不到。
          他們已標記但尚未停用——來源缺漏與離職是兩回事。
          連續多次消失後才會進入待停用清單，由您確認。
          <RouterLink
            to="/designer/sync-center"
            class="underline"
            data-testid="import-goto-sync-center"
          >
            前往同步中心
          </RouterLink>
        </p>

        <div class="mt-space-md flex items-center gap-space-sm">
          <RouterLink
            to="/designer/organization"
            class="h-8 px-3 inline-flex items-center rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header"
            data-testid="import-goto-org"
          >
            查看組織
          </RouterLink>
          <RouterLink
            to="/designer/org-health"
            class="h-8 px-3 inline-flex items-center rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header"
            data-testid="import-goto-health"
          >
            執行健康檢查
          </RouterLink>
        </div>
      </section>
    </div>
  </AppShell>
</template>
