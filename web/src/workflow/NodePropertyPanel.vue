<script setup lang="ts">
/**
 * 節點屬性面板
 *
 * 不同節點型別的設定項差異很大，用 v-if 分段而非動態表單：
 * 每種節點的欄位組合是固定且有限的，寫成設定驅動反而讓
 * 型別檢查失效，也難以為個別欄位加說明文字。
 */
import { computed } from 'vue'
import type {
  Resolver,
  ResolverType,
  WorkflowNode,
} from '@/api/workflows'
import { nodeTypeLabel, resolverLabel } from './palette'

const props = defineProps<{
  node: WorkflowNode | null
  /** 可作為退回目標的節點（上游節點），由畫面計算後傳入 */
  rejectTargets: { id: string; label: string }[]
}>()

const emit = defineEmits<{
  update: [Partial<WorkflowNode>]
  remove: []
}>()

const isHuman = computed(
  () => props.node?.type === 'human_approval' || props.node?.type === 'human_task',
)

/** human_approval 用 resolver，human_task 用 assignee，欄位名不同但語意一致 */
const resolverKey = computed<'resolver' | 'assignee'>(() =>
  props.node?.type === 'human_task' ? 'assignee' : 'resolver',
)

const currentResolver = computed<Resolver | undefined>(
  () => props.node?.[resolverKey.value],
)

const RESOLVER_OPTIONS: { value: ResolverType; label: string }[] = [
  { value: 'manager_of', label: '申請人主管' },
  { value: 'department_manager', label: '部門主管' },
  { value: 'role', label: '指定角色' },
  { value: 'user', label: '指定人員' },
  { value: 'initiator', label: '申請人本人' },
  { value: 'external_contacts', label: '外部聯絡人' },
  { value: 'expression', label: '進階條件' },
]

const TIMEOUT_POLICIES: { value: string; label: string; hint: string }[] = [
  { value: 'WAIT', label: '持續等待', hint: '不做任何事，繼續等人處理' },
  { value: 'ESCALATE', label: '逾時加簽', hint: '加簽給指定的人，原簽核人仍可處理' },
  { value: 'AUTO_APPROVE', label: '自動核准', hint: '視為同意並繼續' },
  { value: 'AUTO_REJECT', label: '自動退回', hint: '視為退回並走退回路徑' },
]

/**
 * 此節點可用的逾時策略
 *
 * join 不支援 ESCALATE——分支各有自己的簽核人，加簽給誰沒有明確語意。
 * 列出來只會讓使用者設了才在驗證時被 WF-E009 擋下。
 */
const timeoutPolicies = computed(() =>
  props.node?.type === 'join'
    ? TIMEOUT_POLICIES.filter((p) => p.value !== 'ESCALATE')
    : TIMEOUT_POLICIES,
)

/** 帶逾時設定的節點型別 */
const SUPPORTS_TIMEOUT = new Set(['human_approval', 'human_task', 'join'])

const showTimeout = computed(() =>
  SUPPORTS_TIMEOUT.has(props.node?.type ?? ''),
)

function patch(key: keyof WorkflowNode, value: unknown) {
  emit('update', { [key]: value } as Partial<WorkflowNode>)
}

/**
 * 切換簽核人型別
 *
 * 整組取代而非合併。不同 type 的附帶欄位不相容，
 * 留著舊欄位會在後端 schema 驗證被擋下。
 */
function changeResolverType(type: ResolverType) {
  const next: Resolver =
    type === 'manager_of' ? { type, of: 'initiator' } : { type }
  patch(resolverKey.value, next)
}

function patchResolverField(key: 'value' | 'user_id' | 'cel', value: string) {
  const current = currentResolver.value
  if (!current) return
  patch(resolverKey.value, { ...current, [key]: value })
}

/** 提醒時間點以逗號分隔顯示，比多個輸入框好編輯 */
const remindAtText = computed(() =>
  (props.node?.timeout?.remind_at ?? []).join(', '),
)

/**
 * 設定提醒時間點
 *
 * 逗號分隔轉陣列。空白與空項目忽略——使用者打「P1D, , PT6H」
 * 或手殘多打一個逗號，不該產生一個空字串的提醒。
 */
function patchRemindAt(raw: string) {
  const current = props.node?.timeout
  if (!current) return

  const list = raw
    .split(',')
    .map((s) => s.trim())
    .filter((s) => s !== '')

  const next = { ...current }
  if (list.length === 0) {
    delete next.remind_at
  } else {
    next.remind_at = list
  }
  patch('timeout', next)
}

function patchTimeout(key: 'after' | 'policy', value: string) {
  const current = props.node?.timeout
  if (key === 'after' && !value) {
    patch('timeout', undefined)
    return
  }
  patch('timeout', {
    after: current?.after ?? 'P3D',
    policy: current?.policy ?? 'WAIT',
    [key]: value,
  })
}

/**
 * 判定方式的說明
 *
 * 三種策略的差別不看例子很難懂，尤其 ANY_REJECT 與 ALL_SUCCESS
 * 在「全部核准」的情況下結果相同，差別只在有人退回時。
 */
const joinResultHint = computed(() => {
  switch (props.node?.join?.result ?? 'ALL_SUCCESS') {
    case 'ANY_REJECT':
      return '任一人退回就立刻退回，不等其他人簽完'
    case 'MAJORITY':
      return '以全體分支數為分母計算過半'
    default:
      return '所有分支都核准才繼續，有人退回則整體退回'
  }
})

function patchJoin(key: 'completion' | 'n' | 'result', value: string | number) {
  const current = props.node?.join
  const next: Record<string, unknown> = {
    completion: current?.completion ?? 'ALL',
    ...current,
    [key]: value,
  }

  // 切成 N_OF_M 時給 n 一個預設值。不給的話卡片會顯示
  // 「? 人完成即繼續」，使用者可能就這樣存檔，等到驗證才發現
  // WF-E009（N_OF_M 缺 n）。
  if (next.completion === 'N_OF_M' && next.n === undefined) {
    next.n = 1
  }
  // 切回其他模式時把 n 拿掉，留著會讓 JSON 有無意義的欄位
  if (next.completion !== 'N_OF_M') {
    delete next.n
  }

  patch('join', next)
}

function patchChannel(ch: 'email' | 'line', on: boolean) {
  const current = new Set(props.node?.channel ?? [])
  on ? current.add(ch) : current.delete(ch)
  patch('channel', [...current])
}

function patchRejectTarget(value: string) {
  patch(
    'on_reject',
    value ? { action: 'goto', node: value } : { action: 'end', result: 'rejected' },
  )
}

const rejectValue = computed(() =>
  props.node?.on_reject?.action === 'goto' ? props.node.on_reject.node ?? '' : '',
)

const INPUT_CLASS =
  'w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-body-dense text-body-dense text-on-surface focus:outline-none focus:border-primary-container'
</script>

<template>
  <aside
    class="w-[320px] shrink-0 bg-surface-container-lowest border-l border-outline-variant flex flex-col"
    data-testid="node-property-panel"
  >
    <div
      v-if="!node"
      class="flex-1 flex flex-col items-center justify-center gap-2 p-6 text-center"
      data-testid="panel-empty"
    >
      <span class="material-symbols-outlined text-[32px] text-outline">tune</span>
      <p class="font-body-dense text-body-dense text-secondary">
        點選畫布上的節點以編輯設定
      </p>
    </div>

    <template v-else>
      <div class="px-4 py-3 border-b border-outline-variant">
        <div class="flex items-center justify-between">
          <span class="font-title-md text-title-md text-on-surface">節點設定</span>
          <span
            class="px-1.5 py-0.5 rounded-lg bg-surface-container-high font-label-caption text-label-caption text-secondary"
          >
            {{ nodeTypeLabel(node.type) }}
          </span>
        </div>
        <span class="font-data-mono text-[11px] text-outline">#{{ node.id }}</span>
      </div>

      <div class="flex-1 overflow-y-auto p-4 space-y-4">
        <label class="block">
          <span class="block font-label-header text-label-header text-secondary mb-1">
            節點名稱
          </span>
          <input
            :value="node.label ?? ''"
            data-testid="node-prop-label"
            :class="INPUT_CLASS"
            @input="patch('label', ($event.target as HTMLInputElement).value)"
          />
        </label>

        <!-- 條件分支 -->
        <label v-if="node.type === 'condition'" class="block">
          <span class="block font-label-header text-label-header text-secondary mb-1">
            判斷條件
          </span>
          <input
            :value="node.expression ?? ''"
            placeholder="form.amount > 100000"
            data-testid="node-prop-expression"
            class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-[12px] text-on-surface focus:outline-none focus:border-primary-container"
            @input="patch('expression', ($event.target as HTMLInputElement).value)"
          />
          <span class="block mt-1 font-label-caption text-label-caption text-outline">
            條件成立走「是」分支，否則走「否」分支
          </span>
        </label>

        <!-- 人工節點 -->
        <template v-if="isHuman">
          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              參與者類型
            </span>
            <select
              :value="node.participant ?? 'internal'"
              data-testid="node-prop-participant"
              :class="INPUT_CLASS"
              @change="patch('participant', ($event.target as HTMLSelectElement).value)"
            >
              <option value="internal">內部員工</option>
              <option value="external">外部客戶</option>
            </select>
          </label>

          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              {{ node.type === 'human_task' ? '處理人' : '簽核人' }}
            </span>
            <select
              :value="currentResolver?.type ?? ''"
              data-testid="node-prop-resolver-type"
              :class="INPUT_CLASS"
              @change="changeResolverType(($event.target as HTMLSelectElement).value as ResolverType)"
            >
              <option value="" disabled>請選擇</option>
              <option v-for="o in RESOLVER_OPTIONS" :key="o.value" :value="o.value">
                {{ o.label }}
              </option>
            </select>
            <span
              class="block mt-1 font-label-caption text-label-caption text-outline"
              data-testid="node-prop-resolver-summary"
            >
              目前：{{ resolverLabel(currentResolver) }}
            </span>
          </label>

          <label v-if="currentResolver?.type === 'role'" class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              角色代碼
            </span>
            <input
              :value="currentResolver.value ?? ''"
              placeholder="finance_manager"
              data-testid="node-prop-resolver-value"
              :class="INPUT_CLASS"
              @input="patchResolverField('value', ($event.target as HTMLInputElement).value)"
            />
          </label>

          <label v-if="currentResolver?.type === 'user'" class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              使用者 ID
            </span>
            <input
              :value="currentResolver.user_id ?? ''"
              data-testid="node-prop-resolver-user"
              :class="INPUT_CLASS"
              @input="patchResolverField('user_id', ($event.target as HTMLInputElement).value)"
            />
          </label>

          <label v-if="currentResolver?.type === 'expression'" class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              CEL 表達式
            </span>
            <input
              :value="currentResolver.cel ?? ''"
              data-testid="node-prop-resolver-cel"
              class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-[12px]"
              @input="patchResolverField('cel', ($event.target as HTMLInputElement).value)"
            />
          </label>


        <!-- 退回 -->
          <div class="pt-3 border-t border-outline-variant">
            <label class="block">
              <span class="block font-label-header text-label-header text-secondary mb-1">
                退回時
              </span>
              <select
                :value="rejectValue"
                data-testid="node-prop-reject"
                :class="INPUT_CLASS"
                @change="patchRejectTarget(($event.target as HTMLSelectElement).value)"
              >
                <option value="">直接結束流程</option>
                <option v-for="t in rejectTargets" :key="t.id" :value="t.id">
                  退回至 {{ t.label }}
                </option>
              </select>
              <span class="block mt-1 font-label-caption text-label-caption text-outline">
                只能退回上游節點，避免產生走不完的迴圈
              </span>
            </label>
          </div>
        </template>

        <!-- 逾時。簽核節點與 join 都適用 -->
        <div
          v-if="showTimeout"
          class="pt-3 border-t border-outline-variant space-y-3"
        >
        <span class="block font-label-header text-label-header text-secondary">
          逾時處理
        </span>
        <label class="block">
          <span class="block font-label-caption text-label-caption text-outline mb-1">
            等待時間（ISO 8601，例如 P3D、PT8H）
          </span>
          <input
            :value="node.timeout?.after ?? ''"
            placeholder="留空表示不設逾時"
            data-testid="node-prop-timeout-after"
            class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-[12px]"
            @input="patchTimeout('after', ($event.target as HTMLInputElement).value)"
          />
        </label>
        <label v-if="node.timeout" class="block">
          <span class="block font-label-caption text-label-caption text-outline mb-1">
            逾時後的動作
          </span>
          <select
            :value="node.timeout.policy"
            data-testid="node-prop-timeout-policy"
            :class="INPUT_CLASS"
            @change="patchTimeout('policy', ($event.target as HTMLSelectElement).value)"
          >
            <option v-for="p in timeoutPolicies" :key="p.value" :value="p.value">
              {{ p.label }}
            </option>
          </select>
          <span class="block mt-1 font-label-caption text-label-caption text-outline">
            {{ timeoutPolicies.find((p) => p.value === node!.timeout!.policy)?.hint }}
          </span>
        </label>

        <!--
          逾時前提醒。設了期限才有意義——沒有期限就沒有「到期前」可言。
        -->
        <label v-if="node.timeout" class="block">
          <span class="block font-label-caption text-label-caption text-outline mb-1">
            到期前提醒（逗號分隔，例如 P2D, P1D, PT6H）
          </span>
          <input
            :value="remindAtText"
            placeholder="留空表示不提醒"
            data-testid="node-prop-remind-at"
            class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-[12px]"
            @input="patchRemindAt(($event.target as HTMLInputElement).value)"
          />
          <span class="block mt-1 font-label-caption text-label-caption text-outline">
            各時間點發一次給尚未處理的人，不影響逾時後的動作
          </span>
        </label>
        </div>

        <!-- 匯合 -->
        <template v-if="node.type === 'join'">
          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              完成條件
            </span>
            <select
              :value="node.join?.completion ?? 'ALL'"
              data-testid="node-prop-join-completion"
              :class="INPUT_CLASS"
              @change="patchJoin('completion', ($event.target as HTMLSelectElement).value)"
            >
              <option value="ALL">全部完成</option>
              <option value="ANY">任一完成</option>
              <option value="N_OF_M">N 人完成</option>
            </select>
          </label>

          <label v-if="node.join?.completion === 'N_OF_M'" class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              需要幾人完成
            </span>
            <input
              type="number"
              min="1"
              :value="node.join?.n ?? 1"
              data-testid="node-prop-join-n"
              :class="INPUT_CLASS"
              @input="patchJoin('n', Number(($event.target as HTMLInputElement).value))"
            />
          </label>

          <!--
            結果策略決定「分支都完成後，整體算通過還是退回」。
            與完成條件是兩件事：完成條件管「等幾個」，
            結果策略管「怎麼算」。
          -->
          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              判定方式
            </span>
            <select
              :value="node.join?.result ?? 'ALL_SUCCESS'"
              data-testid="node-prop-join-result"
              :class="INPUT_CLASS"
              @change="patchJoin('result', ($event.target as HTMLSelectElement).value)"
            >
              <option value="ALL_SUCCESS">全部核准才通過</option>
              <option value="ANY_REJECT">任一退回即退回</option>
              <option value="MAJORITY">過半核准即通過</option>
            </select>
            <span class="block font-label-caption text-label-caption text-secondary mt-1">
              {{ joinResultHint }}
            </span>
          </label>
        </template>

        <!-- 系統動作 -->
        <template v-if="node.type === 'action'">
          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              動作代碼
            </span>
            <input
              :value="node.action ?? ''"
              placeholder="erp.create_sale_order"
              data-testid="node-prop-action"
              class="w-full h-9 px-2 bg-surface-container-low border border-outline-variant rounded-lg font-data-mono text-[12px]"
              @input="patch('action', ($event.target as HTMLInputElement).value)"
            />
          </label>

          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              最多重試次數
            </span>
            <input
              type="number"
              min="1"
              :value="node.retry?.max_attempts ?? 3"
              data-testid="node-prop-retry"
              :class="INPUT_CLASS"
              @input="
                patch('retry', {
                  ...node.retry,
                  max_attempts: Number(($event.target as HTMLInputElement).value),
                })
              "
            />
            <span class="block mt-1 font-label-caption text-label-caption text-outline">
              呼叫外部系統可能暫時失敗，重試由 Temporal 保證不重複執行
            </span>
          </label>
        </template>

        <!-- 通知 -->
        <template v-if="node.type === 'notification'">
          <div>
            <span class="block font-label-header text-label-header text-secondary mb-1">
              發送管道
            </span>
            <div class="flex gap-3">
              <label
                v-for="ch in (['email', 'line'] as const)"
                :key="ch"
                class="flex items-center gap-1.5 font-body-dense text-body-dense text-on-surface"
              >
                <input
                  type="checkbox"
                  :checked="(node.channel ?? []).includes(ch)"
                  :data-testid="`node-prop-channel-${ch}`"
                  @change="patchChannel(ch, ($event.target as HTMLInputElement).checked)"
                />
                {{ ch === 'email' ? 'Email' : 'LINE' }}
              </label>
            </div>
          </div>

          <label class="block">
            <span class="block font-label-header text-label-header text-secondary mb-1">
              收件對象（逗號分隔）
            </span>
            <input
              :value="(node.to ?? []).join(',')"
              placeholder="initiator,approvers"
              data-testid="node-prop-to"
              :class="INPUT_CLASS"
              @input="
                patch(
                  'to',
                  ($event.target as HTMLInputElement).value
                    .split(',')
                    .map((s) => s.trim())
                    .filter(Boolean),
                )
              "
            />
          </label>
        </template>

        <!-- 結束 -->
        <label v-if="node.type === 'end'" class="block">
          <span class="block font-label-header text-label-header text-secondary mb-1">
            結束狀態
          </span>
          <select
            :value="node.result ?? 'completed'"
            data-testid="node-prop-result"
            :class="INPUT_CLASS"
            @change="patch('result', ($event.target as HTMLSelectElement).value)"
          >
            <option value="completed">正常完成</option>
            <option value="rejected">已退回</option>
            <option value="cancelled">已取消</option>
          </select>
        </label>
      </div>

      <div v-if="node.type !== 'trigger'" class="p-4 border-t border-outline-variant">
        <button
          type="button"
          data-testid="node-prop-remove"
          class="w-full h-9 rounded-lg border border-error text-error font-body-dense text-body-dense hover:bg-error-container"
          @click="emit('remove')"
        >
          刪除此節點
        </button>
      </div>
    </template>
  </aside>
</template>
