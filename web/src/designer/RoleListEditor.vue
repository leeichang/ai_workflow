<script setup lang="ts">
/**
 * 角色清單編輯器
 *
 * 三態必須在介面上分得開。用單一輸入框時「拒絕所有角色」與
 * 「不限制」都是空的，使用者無從表達前者。
 */
import { computed } from 'vue'
import {
  readRoleList,
  ROLE_LIST_MODES,
  toggleRole,
  writeRoleList,
  type RoleListMode,
} from './roleList'
import type { RoleOption } from './roles'

const props = defineProps<{
  label: string
  /** DSL 中的原值。undefined 代表鍵不存在。 */
  value: string[] | undefined
  roles: RoleOption[]
  testid: string
}>()

const emit = defineEmits<{ change: [string[] | undefined] }>()

const state = computed(() => readRoleList(props.value))

const hint = computed(
  () => ROLE_LIST_MODES.find((m) => m.value === state.value.mode)?.hint ?? '',
)

function setMode(mode: RoleListMode) {
  if (mode === state.value.mode) return

  // 從清單切到其他模式時保留已勾選的角色沒有意義，
  // 因為那兩種模式都不看清單內容。
  emit('change', writeRoleList({ mode, roles: mode === 'allow_list' ? props.roles.slice(0, 1).map((r) => r.code) : [] }))
}

function toggle(role: string) {
  emit('change', writeRoleList(toggleRole(state.value, role)))
}
</script>

<template>
  <div :data-testid="testid">
    <span class="block font-label-header text-label-header text-secondary mb-1">
      {{ label }}
    </span>

    <div class="flex gap-0.5 p-0.5 bg-surface-container-low rounded-lg" role="radiogroup">
      <button
        v-for="m in ROLE_LIST_MODES"
        :key="m.value"
        type="button"
        role="radio"
        :aria-checked="state.mode === m.value"
        :data-testid="`${testid}-mode-${m.value}`"
        class="flex-1 h-7 rounded font-label-caption text-label-caption transition-colors"
        :class="
          state.mode === m.value
            ? 'bg-surface-container-lowest text-primary font-semibold shadow-sm'
            : 'text-secondary hover:text-on-surface'
        "
        @click="setMode(m.value)"
      >
        {{ m.label }}
      </button>
    </div>

    <span class="block font-label-caption text-label-caption text-outline mt-1">
      {{ hint }}
    </span>

    <div
      v-if="state.mode === 'allow_list'"
      class="mt-1.5 space-y-0.5"
      :data-testid="`${testid}-roles`"
    >
      <label
        v-for="r in roles"
        :key="r.code"
        class="flex items-center gap-2 px-2 py-1 rounded-lg hover:bg-surface-container-low cursor-pointer"
      >
        <input
          type="checkbox"
          :checked="state.roles.includes(r.code)"
          :data-testid="`${testid}-role-${r.code}`"
          @change="toggle(r.code)"
        />
        <span class="font-body-dense text-body-dense text-on-surface">{{ r.label }}</span>
        <span class="font-data-mono text-[10px] text-outline ml-auto">{{ r.code }}</span>
      </label>
    </div>

    <!--
      全部拒絕是很強的設定，容易誤選，因此明講後果。
      admin 不受此限制，在後端 readonly_or_editable() 中優先回傳可編輯。
    -->
    <p
      v-if="state.mode === 'deny_all'"
      class="mt-1.5 px-2 py-1.5 rounded-lg bg-error-container font-label-caption text-label-caption text-on-error-container"
      :data-testid="`${testid}-deny-warning`"
    >
      除系統管理員外，所有角色都無法{{ label.includes('編輯') ? '編輯' : '檢視' }}此欄位
    </p>
  </div>
</template>
