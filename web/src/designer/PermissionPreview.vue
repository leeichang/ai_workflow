<script setup lang="ts">
/**
 * 權限預覽面板
 *
 * 在表單設計器底部展開，回答一個問題：
 * 「某角色在某節點上，這張表單實際長什麼樣？」
 *
 * 判定結果由後端計算（見 usePermissionPreview 的說明），
 * 這裡只負責挑情境與呈現。
 */
import { computed, ref, watch } from 'vue'
import type { FormField } from '@/api/types'
import { ROLES } from './roles'
import { PERMISSION_STYLE, type PreviewScenario } from './usePermissionPreview'
import type { FieldPermission } from '@/api/permissions'

const props = defineProps<{
  fields: FormField[]
  result: FieldPermission[]
  loading: boolean
  error: string | null
  stale: boolean
  /** 流程節點清單，空陣列代表尚未綁定流程 */
  nodes: { id: string; label: string }[]
}>()

const emit = defineEmits<{ run: [PreviewScenario] }>()

const role = ref('requester')
const nodeId = ref('')
const participantKind = ref<'internal' | 'external'>('internal')

const scenario = computed<PreviewScenario>(() => ({
  nodeId: nodeId.value || undefined,
  roles: [role.value],
  participantKind: participantKind.value,
}))

// 情境一改就重新查。這是唯讀查詢，沒有誤觸的代價。
watch(scenario, (s) => emit('run', s), { immediate: true, deep: true })

/** 依欄位在表單中的順序排列結果，並補上標籤 */
const rows = computed(() => {
  const byKey = new Map(props.result.map((r) => [r.key, r]))

  return props.fields.map((f) => {
    const r = byKey.get(f.key)
    return {
      key: f.key,
      label: f.ui.label || f.key,
      // 後端沒回這個欄位代表它被判定為隱藏後就不再列出，
      // 但目前 resolve_form 會回傳所有欄位，這裡只是防禦。
      permission: r?.permission ?? 'HIDDEN',
      required: r?.required ?? false,
      reason: r?.reason ?? '未知',
    }
  })
})

const summary = computed(() => {
  const count = { EDITABLE: 0, READONLY: 0, HIDDEN: 0 }
  for (const r of rows.value) {
    count[r.permission as keyof typeof count] += 1
  }
  return count
})

</script>

<template>
  <section
    class="border-t border-outline-variant bg-surface-container-lowest flex flex-col"
    data-testid="permission-preview"
  >
    <div class="flex items-center gap-3 px-space-xl py-2 border-b border-outline-variant">
      <span class="flex items-center gap-1.5 font-title-md text-title-md text-on-surface shrink-0">
        <span class="material-symbols-outlined text-[18px] text-primary">preview</span>
        權限預覽
      </span>

      <label class="flex items-center gap-1.5">
        <span class="font-label-caption text-label-caption text-secondary">角色</span>
        <select
          v-model="role"
          data-testid="preview-role"
          class="h-7 px-1.5 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
        >
          <option v-for="r in ROLES" :key="r.code" :value="r.code">{{ r.label }}</option>
        </select>
      </label>

      <label v-if="nodes.length > 0" class="flex items-center gap-1.5">
        <span class="font-label-caption text-label-caption text-secondary">節點</span>
        <select
          v-model="nodeId"
          data-testid="preview-node"
          class="h-7 px-1.5 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
        >
          <option value="">不指定</option>
          <option v-for="n in nodes" :key="n.id" :value="n.id">{{ n.label }}</option>
        </select>
      </label>

      <label class="flex items-center gap-1.5">
        <span class="font-label-caption text-label-caption text-secondary">參與者</span>
        <select
          v-model="participantKind"
          data-testid="preview-participant"
          class="h-7 px-1.5 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense"
        >
          <option value="internal">內部員工</option>
          <option value="external">外部客戶</option>
        </select>
      </label>

      <div class="flex items-center gap-2 ml-auto font-label-caption text-label-caption">
        <span
          v-for="(n, p) in summary"
          :key="p"
          class="px-1.5 py-0.5 rounded-lg"
          :class="[PERMISSION_STYLE[p].bg, PERMISSION_STYLE[p].text]"
          :data-testid="`preview-count-${p}`"
        >
          {{ PERMISSION_STYLE[p].label }} {{ n }}
        </span>
      </div>
    </div>

    <div
      v-if="stale"
      class="px-space-xl py-1.5 bg-[#FEF3C7] font-label-caption text-label-caption text-[#B45309]"
      data-testid="preview-stale"
    >
      草稿有未儲存的修改，預覽結果仍是上次儲存的版本。儲存後會自動更新。
    </div>

    <div
      v-if="error"
      class="px-space-xl py-2 bg-error-container font-body-dense text-body-dense text-on-error-container"
      data-testid="preview-error"
    >
      {{ error }}
    </div>

    <div v-else class="max-h-[220px] overflow-y-auto">
      <div
        v-if="loading && rows.length === 0"
        class="px-space-xl py-4 text-center font-body-dense text-body-dense text-secondary"
      >
        計算中
      </div>

      <table v-else class="w-full">
        <tbody>
          <tr
            v-for="r in rows"
            :key="r.key"
            class="border-b border-outline-variant last:border-0"
            :data-testid="`preview-row-${r.key}`"
          >
            <td class="px-space-xl py-1.5 font-body-dense text-body-dense text-on-surface w-[240px]">
              {{ r.label }}
              <span v-if="r.required" class="text-error ml-0.5">*</span>
            </td>
            <td class="py-1.5 w-[100px]">
              <span
                class="inline-flex items-center gap-1 px-1.5 py-0.5 rounded-lg font-label-caption text-label-caption"
                :class="[PERMISSION_STYLE[r.permission].bg, PERMISSION_STYLE[r.permission].text]"
                :data-testid="`preview-permission-${r.key}`"
              >
                <span class="material-symbols-outlined text-[12px]">
                  {{ PERMISSION_STYLE[r.permission].icon }}
                </span>
                {{ PERMISSION_STYLE[r.permission].label }}
              </span>
            </td>
            <td
              class="py-1.5 pr-space-xl font-label-caption text-label-caption text-secondary"
              :data-testid="`preview-reason-${r.key}`"
            >
              {{ r.reason }}
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  </section>
</template>
