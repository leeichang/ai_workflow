<script setup lang="ts">
/**
 * 表單清單與新增
 *
 * 補的是一個實際存在的缺口：後端有 POST /forms，前端的 createForm()
 * 也寫好了，但先前**沒有任何畫面呼叫它**——設計器的路由是
 * /designer/forms/:formKey，只能開既有表單，/designer 的 redirect
 * 還寫死指向 quotation_form。
 *
 * 換句話說：使用者無法從零建立一張表單。
 * 這與產品定位「讓使用者自己是 Owner」直接衝突。
 */
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import AppShell from '@/components/AppShell.vue'
import * as formsApi from '@/api/forms'
import type { FormSummary } from '@/api/types'
import { ApiError } from '@/api/types'

const router = useRouter()

const forms = ref<FormSummary[]>([])
const loading = ref(true)
const errorMessage = ref('')

// ── 新增對話框 ────────────────────────────────────
const dialogOpen = ref(false)
const newKey = ref('')
const newName = ref('')
const newObject = ref('')
const createError = ref('')
const creating = ref(false)

/**
 * form_key 的格式限制
 *
 * 這個值會進 API 路徑（/forms/{form_key}）與資料表關聯，
 * 建立之後不能改。允許大小寫混用會讓 URL 難以預測，
 * 連字號則與既有慣例（quotation_form、pr_form）不一致。
 *
 * 擋在前端是為了給出可讀的訊息——後端也會擋，但它的錯誤
 * 對使用者來說是天書。
 */
const KEY_PATTERN = /^[a-z][a-z0-9_]*$/

function openDialog(): void {
  newKey.value = ''
  newName.value = ''
  newObject.value = ''
  createError.value = ''
  dialogOpen.value = true
}

async function load(): Promise<void> {
  loading.value = true
  errorMessage.value = ''
  try {
    forms.value = await formsApi.listForms()
  } catch (error) {
    errorMessage.value =
      error instanceof ApiError ? error.message : '無法載入表單，請稍後再試'
  } finally {
    loading.value = false
  }
}

/**
 * 新表單的初始內容
 *
 * 必須帶一個欄位——schemas/form.schema.json 對 fields 設了
 * minItems: 1，零欄位的表單會被擋下（「/fields：[] has less than 1 item」）。
 *
 * 這個限制反而對使用者有利：進設計器時看得到一個實際的欄位，
 * 知道欄位長什麼樣、屬性面板怎麼用，而不是對著空白畫布不知從何下手。
 *
 * 用「單號」當起始欄位，因為幾乎每張表單都需要它，
 * 而且它示範了最常見的組合：單行文字 + 資料路徑。
 *
 * 路徑用 `{object}.number` 而非 `business_key`：
 * `business_key` 是 workflow_instance 的欄位（流程實例的識別碼），
 * 不是業務資料。業務物件定義（schemas/business-objects/*.json）
 * 收錄的是 quotation.number 這類業務欄位——指向 business_key
 * 會建立一條永遠不在正式定義裡的路徑。
 */
function initialContent(formKey: string, name: string, object: string) {
  return {
    form_key: formKey,
    version: 1,
    business_object: object,
    name,
    layout: { columns: 2 },
    sections: [{ key: 'main', title: '基本資料' }],
    // 欄位的結構是 { key, section, ui, data }——
    // component 與 label 在 ui 底下，不是欄位的頂層屬性
    fields: [
      {
        key: 'number',
        section: 'main',
        ui: { component: 'input', label: '單號' },
        data: { path: `${object}.number`, type: 'string' },
      },
    ],
  }
}

async function confirmCreate(): Promise<void> {
  const key = newKey.value.trim()
  const name = newName.value.trim()

  if (key === '') {
    createError.value = '請輸入表單代碼'
    return
  }
  if (!KEY_PATTERN.test(key)) {
    createError.value =
      '表單代碼只能用小寫英文、數字與底線，且需以英文開頭（例如 employee_form）'
    return
  }
  if (name === '') {
    createError.value = '請輸入表單名稱'
    return
  }

  // 業務物件留空時沿用 form_key。多數情況兩者相同，
  // 強迫使用者填兩次只是徒增困擾。
  const object = newObject.value.trim() === '' ? key : newObject.value.trim()

  creating.value = true
  createError.value = ''
  try {
    await formsApi.createForm({
      form_key: key,
      business_object: object,
      name,
      content: initialContent(key, name, object),
    })
    // 建完直接進設計器，不要讓使用者自己再點一次
    router.push(`/designer/forms/${key}`)
  } catch (error) {
    createError.value =
      error instanceof ApiError ? error.message : '建立失敗，請稍後再試'
  } finally {
    creating.value = false
  }
}

/** 表單的發布狀態。未發布與有草稿是兩件事，要分開顯示。 */
function statusOf(f: FormSummary): { text: string; class: string } {
  if (f.published_version === null) {
    return {
      text: '尚未發布',
      class: 'bg-surface-container-low text-secondary',
    }
  }
  if (f.has_draft) {
    return { text: `v${f.published_version} · 有草稿`, class: 'bg-[#FEF3C7] text-[#B45309]' }
  }
  return { text: `v${f.published_version}`, class: 'bg-[#DCFCE7] text-[#15803D]' }
}

const isEmpty = computed(() => !loading.value && forms.value.length === 0)

onMounted(load)
</script>

<template>
  <AppShell>
    <template #breadcrumb>
      <span class="text-secondary">設計器</span>
      <span class="text-outline mx-1">/</span>
      <span class="text-on-surface font-semibold">表單</span>
    </template>

    <div class="max-w-[1000px]">
      <div class="flex items-center justify-between mb-space-lg">
        <div>
          <h1 class="font-title-md text-title-md text-on-surface">表單</h1>
          <p class="font-body-dense text-body-dense text-secondary mt-1">
            建立與維護表單定義
          </p>
        </div>
        <div class="flex items-center gap-space-sm">
          <button
            type="button"
            class="flex items-center gap-1 h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
            data-testid="form-refresh"
            @click="load"
          >
            <span class="material-symbols-outlined text-[16px]">refresh</span>
            重新整理
          </button>
          <button
            type="button"
            class="flex items-center gap-1 h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 transition-opacity"
            data-testid="form-create"
            @click="openDialog"
          >
            <span class="material-symbols-outlined text-[16px]">add</span>
            建立表單
          </button>
        </div>
      </div>

      <p
        v-if="errorMessage"
        role="alert"
        data-testid="form-error"
        class="mb-space-md px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
      >
        {{ errorMessage }}
      </p>

      <div
        v-if="loading"
        data-testid="form-loading"
        class="py-space-xl text-center font-body-dense text-body-dense text-secondary"
      >
        載入中…
      </div>

      <!--
        空狀態不只說「沒有資料」。使用者第一次進來看到的就是這個畫面，
        要明確告訴他下一步做什麼。
      -->
      <div
        v-else-if="isEmpty"
        data-testid="form-empty"
        class="py-space-xl flex flex-col items-center gap-space-sm text-secondary"
      >
        <span class="material-symbols-outlined text-[40px] text-outline">description</span>
        <p class="font-body-dense text-body-dense">尚未建立任何表單</p>
        <p class="font-body-dense text-body-dense text-outline">
          按右上角的「建立表單」開始，或先參考操作手冊
        </p>
      </div>

      <table
        v-else
        class="w-full bg-surface-container-lowest border border-outline-variant rounded-xl overflow-hidden"
        data-testid="form-table"
      >
        <thead>
          <tr class="bg-surface-container-low border-b border-outline-variant">
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              名稱
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              代碼
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              業務物件
            </th>
            <th class="text-left px-space-md py-space-sm font-label-header text-label-header text-secondary">
              版本
            </th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="f in forms"
            :key="f.id"
            :data-testid="`form-row-${f.form_key}`"
            class="border-b border-outline-variant last:border-b-0 hover:bg-surface-container-low transition-colors cursor-pointer"
            @click="router.push(`/designer/forms/${f.form_key}`)"
          >
            <td class="px-space-md py-space-sm font-body-dense text-body-dense text-on-surface">
              {{ f.name }}
            </td>
            <td class="px-space-md py-space-sm font-data-mono text-[12px] text-secondary">
              {{ f.form_key }}
            </td>
            <td class="px-space-md py-space-sm font-data-mono text-[12px] text-secondary">
              {{ f.business_object }}
            </td>
            <td class="px-space-md py-space-sm">
              <span
                class="inline-block px-2 py-0.5 rounded font-label-caption text-label-caption"
                :class="statusOf(f).class"
              >
                {{ statusOf(f).text }}
              </span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- ── 新增對話框 ──────────────────────────────── -->
    <div
      v-if="dialogOpen"
      class="fixed inset-0 z-50 bg-black/50 flex items-center justify-center p-8"
      data-testid="create-dialog"
      @click.self="dialogOpen = false"
    >
      <div class="bg-surface-container-lowest rounded-xl shadow-2xl w-[520px] max-w-full">
        <div class="px-space-lg py-space-md border-b border-outline-variant">
          <h2 class="font-title-sm text-title-sm text-on-surface">建立表單</h2>
        </div>

        <div class="px-space-lg py-space-md flex flex-col gap-space-md">
          <label class="flex flex-col gap-1">
            <span class="font-label-header text-label-header text-secondary">
              表單名稱 <span class="text-error">*</span>
            </span>
            <input
              v-model="newName"
              type="text"
              placeholder="例如：員工基本資料"
              data-testid="new-form-name"
              class="h-9 px-3 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
            />
          </label>

          <label class="flex flex-col gap-1">
            <span class="font-label-header text-label-header text-secondary">
              表單代碼 <span class="text-error">*</span>
            </span>
            <input
              v-model="newKey"
              type="text"
              placeholder="例如：employee_form"
              data-testid="new-form-key"
              class="h-9 px-3 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-[12px]"
            />
            <!-- 事後不能改，所以要先講清楚 -->
            <span class="font-label-caption text-label-caption text-outline">
              小寫英文、數字與底線。建立後不能修改。
            </span>
          </label>

          <label class="flex flex-col gap-1">
            <span class="font-label-header text-label-header text-secondary">
              業務物件
            </span>
            <input
              v-model="newObject"
              type="text"
              placeholder="留空則與表單代碼相同"
              data-testid="new-business-object"
              class="h-9 px-3 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-[12px]"
            />
            <span class="font-label-caption text-label-caption text-outline">
              流程與報表用來辨識這張表單的資料。多數情況與表單代碼相同。
            </span>
          </label>

          <p
            v-if="createError"
            role="alert"
            data-testid="create-error"
            class="px-3 py-2 rounded-lg bg-error-container text-on-error-container font-body-dense text-body-dense"
          >
            {{ createError }}
          </p>
        </div>

        <div class="px-space-lg py-space-md border-t border-outline-variant flex justify-end gap-space-sm">
          <button
            type="button"
            data-testid="create-cancel"
            class="h-8 px-3 rounded-lg border border-outline-variant text-on-surface-variant hover:bg-surface-container-low font-label-header text-label-header transition-colors"
            @click="dialogOpen = false"
          >
            取消
          </button>
          <button
            type="button"
            data-testid="create-confirm"
            :disabled="creating"
            class="h-8 px-3 rounded-lg bg-primary text-on-primary font-label-header text-label-header hover:opacity-90 transition-opacity disabled:opacity-50"
            @click="confirmCreate"
          >
            {{ creating ? '建立中…' : '建立並開始設計' }}
          </button>
        </div>
      </div>
    </div>
  </AppShell>
</template>
