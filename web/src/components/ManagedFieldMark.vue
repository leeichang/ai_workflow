<script setup lang="ts">
/**
 * 平台維護欄位的標記
 *
 * 需求 07 §3 規則 3：UI 在該欄位旁顯示「平台維護，不隨同步更新」，
 * 並提供一鍵「改回跟隨 ERP」。
 *
 * 為什麼這個標記重要：管理員在平台補了 ERP 沒維護的資料（典型是
 * 300 個人沒有 manager_id），這些欄位之後同步時要跳過。沒有標記，
 * 管理員不知道哪些欄位是自己補的、哪些還跟著 ERP 走。
 *
 * 目前 source_system 全是 MANUAL（同步機制尚未實作），標記仍然顯示——
 * 日後某人轉為 ERP 同步來源時，他先前的編輯已經受保護。
 */

defineProps<{
  /** 這個欄位是否已被平台接管 */
  managed: boolean
  /** 解除鎖定進行中。避免連點送出兩次 */
  busy?: boolean
}>()

defineEmits<{ unlock: [] }>()
</script>

<template>
  <span v-if="managed" class="inline-flex items-center gap-1">
    <span
      class="inline-flex items-center gap-0.5 px-1.5 py-0.5 rounded bg-[#FEF3C7] text-[#92400E] font-label-caption text-label-caption whitespace-nowrap"
      title="此欄位由平台維護，同步時不會被來源系統覆蓋"
    >
      <span class="material-symbols-outlined text-[12px]">lock</span>
      平台維護
    </span>
    <button
      type="button"
      class="text-primary hover:underline font-label-caption text-label-caption disabled:opacity-50 disabled:cursor-not-allowed"
      :disabled="busy"
      data-testid="unlock-field"
      @click="$emit('unlock')"
    >
      改回跟隨來源
    </button>
  </span>
</template>
