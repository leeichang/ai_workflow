/**
 * 表單設計器狀態
 *
 * 設計稿有 undo / redo，因此狀態需保留歷史。
 * 用「整份 content 快照」而非 diff：表單定義不大（通常數十個欄位），
 * 快照實作簡單且不會有 diff 套用錯誤的風險。
 */

import { computed, ref, shallowRef } from 'vue'
import type { FormContent, FormField } from '@/api/types'
import { getForm, publishDraft, saveDraft, validateDraft } from '@/api/forms'
import { ApiError } from '@/api/types'

const MAX_HISTORY = 50

export function useDesigner(formKey: string) {
  const content = ref<FormContent | null>(null)
  const selectedKey = ref<string | null>(null)

  const loading = ref(false)
  const saving = ref(false)
  const error = ref<string | null>(null)
  const validationErrors = ref<string[]>([])
  /**
   * 不影響發布的提醒
   *
   * 兩種：業務物件未定義、欄位路徑不在正式定義中。
   * 後者是 quotation.total_amount 那類舊名稱——表單本身能用，
   * 但流程的條件式引用正式路徑時會取不到值。
   */
  const warnings = ref<string[]>([])

  /** 最後儲存時間，設計稿顯示「已同步自動存檔 16:42」 */
  const lastSavedAt = ref<Date | null>(null)
  const publishedVersion = ref<number | null>(null)

  // 歷史：past 為過去狀態，future 為被 undo 的狀態
  const past = shallowRef<string[]>([])
  const future = shallowRef<string[]>([])
  const dirty = ref(false)

  const canUndo = computed(() => past.value.length > 0)
  const canRedo = computed(() => future.value.length > 0)

  const selectedField = computed(() =>
    content.value?.fields.find((f) => f.key === selectedKey.value) ?? null,
  )

  /** 在修改前呼叫，把當前狀態推入歷史 */
  function snapshot() {
    if (!content.value) return
    past.value = [...past.value, JSON.stringify(content.value)].slice(-MAX_HISTORY)
    future.value = []
    dirty.value = true
  }

  function undo() {
    if (!canUndo.value || !content.value) return
    const previous = past.value[past.value.length - 1]
    future.value = [JSON.stringify(content.value), ...future.value]
    past.value = past.value.slice(0, -1)
    content.value = JSON.parse(previous)
    dirty.value = true
  }

  function redo() {
    if (!canRedo.value || !content.value) return
    const next = future.value[0]
    past.value = [...past.value, JSON.stringify(content.value)]
    future.value = future.value.slice(1)
    content.value = JSON.parse(next)
    dirty.value = true
  }

  // ── 欄位操作 ──────────────────────────────────────────

  function addField(field: Partial<FormField>, afterKey?: string) {
    if (!content.value) return
    snapshot()

    const full = field as FormField
    const fields = [...content.value.fields]
    const at = afterKey ? fields.findIndex((f) => f.key === afterKey) + 1 : fields.length
    fields.splice(at, 0, full)

    content.value = { ...content.value, fields }
    selectedKey.value = full.key
  }

  function updateField(key: string, patch: Partial<FormField>) {
    if (!content.value) return
    snapshot()

    content.value = {
      ...content.value,
      fields: content.value.fields.map((f) =>
        f.key === key ? mergeField(f, patch) : f,
      ),
    }
  }

  function removeField(key: string) {
    if (!content.value) return
    snapshot()

    content.value = {
      ...content.value,
      fields: content.value.fields.filter((f) => f.key !== key),
    }
    if (selectedKey.value === key) selectedKey.value = null
  }

  function duplicateField(key: string) {
    if (!content.value) return
    const source = content.value.fields.find((f) => f.key === key)
    if (!source) return

    const copy: FormField = JSON.parse(JSON.stringify(source))
    copy.key = `${source.key}_copy`
    // 資料路徑也要改，否則兩個欄位綁同一路徑會在發布時被擋下
    if (copy.data.path) copy.data.path = `${copy.data.path}_copy`
    addField(copy, key)
  }

  /** 移動欄位。delta 為正往下，為負往上。 */
  function moveField(key: string, delta: number) {
    if (!content.value) return
    const fields = [...content.value.fields]
    const from = fields.findIndex((f) => f.key === key)
    const to = from + delta
    if (from < 0 || to < 0 || to >= fields.length) return

    snapshot()
    const [item] = fields.splice(from, 1)
    fields.splice(to, 0, item)
    content.value = { ...content.value, fields }
  }

  // ── 伺服器往返 ────────────────────────────────────────

  async function load() {
    loading.value = true
    error.value = null
    try {
      const detail = await getForm(formKey)
      // 有草稿用草稿，否則用已發布版本當編輯起點
      const source = detail.draft ?? detail.published
      if (!source) {
        error.value = '此表單尚無任何版本'
        return
      }
      content.value = source.content
      publishedVersion.value = detail.published?.version ?? null
      past.value = []
      future.value = []
      dirty.value = false
    } catch (e) {
      error.value = e instanceof ApiError ? e.message : '載入失敗'
    } finally {
      loading.value = false
    }
  }

  async function save() {
    if (!content.value || saving.value) return
    saving.value = true
    error.value = null
    try {
      await saveDraft(formKey, content.value)
      lastSavedAt.value = new Date()
      dirty.value = false
    } catch (e) {
      error.value = e instanceof ApiError ? e.message : '儲存失敗'
      throw e
    } finally {
      saving.value = false
    }
  }

  /** 驗證。發布前先讓使用者看到問題，而不是按了才失敗。 */
  async function validate(): Promise<boolean> {
    validationErrors.value = []
    try {
      // 先存再驗，否則驗的是舊草稿
      if (dirty.value) await save()
      const result = await validateDraft(formKey)
      validationErrors.value = result.errors ?? []
      warnings.value = result.warnings ?? []
      return result.valid
    } catch (e) {
      error.value = e instanceof ApiError ? e.message : '驗證失敗'
      return false
    }
  }

  async function publish(): Promise<boolean> {
    error.value = null
    validationErrors.value = []
    try {
      if (dirty.value) await save()
      const version = await publishDraft(formKey)
      // 發布可能帶著警告成功
      warnings.value = version.warnings ?? []
      publishedVersion.value = version.version
      dirty.value = false
      await load()
      return true
    } catch (e) {
      if (e instanceof ApiError) {
        error.value = e.message
        validationErrors.value = e.validationErrors
      } else {
        error.value = '發布失敗'
      }
      return false
    }
  }

  return {
    content,
    selectedKey,
    selectedField,
    loading,
    saving,
    error,
    validationErrors,
    warnings,
    lastSavedAt,
    publishedVersion,
    dirty,
    canUndo,
    canRedo,
    undo,
    redo,
    addField,
    updateField,
    removeField,
    duplicateField,
    moveField,
    load,
    save,
    validate,
    publish,
  }
}

/**
 * 深層合併欄位設定，避免 patch 只帶部分 ui 屬性時把其他屬性洗掉
 *
 * workflow 例外，是整組取代而非合併。這一區塊的鍵「存在與否」本身
 * 帶有語意：editable_roles 不存在代表不限制，空陣列代表拒絕所有角色。
 * 用展開合併時，呼叫端刪掉的鍵會從 base 補回來，
 * 於是「改為不限制」這個操作永遠沒有效果。
 *
 * 呼叫端（PropertyPanel.patchWorkflow）本來就會送出完整的 workflow，
 * 取代不會遺失其他規則。
 */
function mergeField(base: FormField, patch: Partial<FormField>): FormField {
  return {
    ...base,
    ...patch,
    ui: { ...base.ui, ...patch.ui },
    data: { ...base.data, ...patch.data },
    workflow: patch.workflow ?? base.workflow,
  }
}
