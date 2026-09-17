<script setup lang="ts">
/**
 * 版面畫布
 *
 * A4 頁面等比縮放後以絕對定位擺放元素。所有座標在狀態裡都是 mm，
 * 只在這裡乘上縮放比例轉成像素——反過來（存像素）會讓縮放一改
 * 就得換算全部座標，且四捨五入誤差會累積。
 */
import { computed, ref } from 'vue'
import { PAGE, type TemplateElement } from './useTemplateDesigner'
import { fieldOf } from './useTemplateDesigner'

const props = defineProps<{
  elements: TemplateElement[]
  selectedName: string | null
  zoom: number
}>()

const emit = defineEmits<{
  select: [string | null]
  move: [string, number, number]
}>()

/** mm → px */
const scale = computed(() => (props.zoom / 100) * 3.78)

const mm = (v: number) => v * scale.value

const pageStyle = computed(() => ({
  width: `${mm(PAGE.width)}px`,
  height: `${mm(PAGE.height)}px`,
}))

const marginStyle = computed(() => ({
  top: `${mm(PAGE.padding[0])}px`,
  right: `${mm(PAGE.padding[1])}px`,
  bottom: `${mm(PAGE.padding[2])}px`,
  left: `${mm(PAGE.padding[3])}px`,
}))

function elementStyle(el: TemplateElement) {
  return {
    left: `${mm(el.position.x)}px`,
    top: `${mm(el.position.y)}px`,
    width: `${mm(el.width)}px`,
    minHeight: `${mm(Math.max(el.height, 3))}px`,
    fontSize: `${(el.fontSize ?? 9) * (props.zoom / 100) * 1.33}px`,
    textAlign: el.alignment ?? 'left',
  }
}

/** 標尺刻度，每 10mm 一格 */
const ticks = computed(() => {
  const h: number[] = []
  for (let i = 0; i <= PAGE.width; i += 10) h.push(i)
  const v: number[] = []
  for (let i = 0; i <= PAGE.height; i += 10) v.push(i)
  return { h, v }
})

// ── 拖曳 ────────────────────────────────────────────────

const dragging = ref<{ name: string; startX: number; startY: number } | null>(null)

function startDrag(event: MouseEvent, el: TemplateElement) {
  emit('select', el.name)
  dragging.value = { name: el.name, startX: event.clientX, startY: event.clientY }

  window.addEventListener('mousemove', onDrag)
  window.addEventListener('mouseup', endDrag, { once: true })
}

function onDrag(event: MouseEvent) {
  const d = dragging.value
  if (!d) return

  // 換算回 mm 再交給狀態，畫布不持有像素座標
  const dx = (event.clientX - d.startX) / scale.value
  const dy = (event.clientY - d.startY) / scale.value

  if (Math.abs(dx) < 0.1 && Math.abs(dy) < 0.1) return

  emit('move', d.name, dx, dy)
  dragging.value = { ...d, startX: event.clientX, startY: event.clientY }
}

function endDrag() {
  dragging.value = null
  window.removeEventListener('mousemove', onDrag)
}

/** 綁定欄位的元素顯示欄位名稱，純文字顯示實際內容 */
function labelOf(el: TemplateElement): string {
  if (!el.bound) return el.content || el.type
  return fieldOf(el)?.label ?? el.name
}
</script>

<template>
  <div
    class="flex-1 overflow-auto bg-[#374151] p-8"
    data-testid="template-canvas"
    @click="emit('select', null)"
  >
    <div class="relative mx-auto" :style="{ width: pageStyle.width }">
      <!-- 水平標尺 -->
      <div
        class="relative h-5 mb-1 bg-[#4B5563] rounded-sm overflow-hidden"
        data-testid="ruler-h"
      >
        <span
          v-for="t in ticks.h"
          :key="`h-${t}`"
          class="absolute top-0 h-full border-l border-white/30 text-[9px] text-white/70 pl-0.5 leading-5"
          :style="{ left: `${mm(t)}px` }"
        >
          {{ t % 30 === 0 ? t : '' }}
        </span>
      </div>

      <div class="flex gap-1">
        <!-- 垂直標尺 -->
        <div
          class="relative w-5 shrink-0 bg-[#4B5563] rounded-sm overflow-hidden"
          :style="{ height: pageStyle.height }"
          data-testid="ruler-v"
        >
          <span
            v-for="t in ticks.v"
            :key="`v-${t}`"
            class="absolute left-0 w-full border-t border-white/30 text-[9px] text-white/70 pl-0.5"
            :style="{ top: `${mm(t)}px` }"
          >
            {{ t % 30 === 0 ? t : '' }}
          </span>
        </div>

        <!-- A4 頁面 -->
        <div
          class="relative bg-white shadow-lg shrink-0"
          :style="pageStyle"
          data-testid="page"
          @click.stop
        >
          <!-- 安全邊距 -->
          <div
            class="absolute border border-dashed border-primary-container/60 pointer-events-none"
            :style="{
              top: marginStyle.top,
              left: marginStyle.left,
              right: marginStyle.right,
              bottom: marginStyle.bottom,
            }"
          />

          <div
            v-for="el in elements"
            :key="el.name"
            :data-testid="`el-${el.name}`"
            :data-selected="selectedName === el.name ? 'true' : 'false'"
            class="absolute cursor-move select-none px-0.5 leading-tight transition-shadow"
            :class="[
              el.bound
                ? 'bg-primary-container/10 border border-dashed border-primary-container/70 text-primary'
                : 'text-on-surface',
              selectedName === el.name
                ? 'ring-2 ring-primary-container shadow-[0_0_0_1px_rgba(37,99,235,0.3)]'
                : '',
            ]"
            :style="elementStyle(el)"
            @mousedown.stop="startDrag($event, el)"
            @click.stop="emit('select', el.name)"
          >
            <!-- 表格畫出欄位名稱，讓使用者看得出欄位組成 -->
            <table v-if="el.type === 'table'" class="w-full border-collapse">
              <thead>
                <tr>
                  <th
                    v-for="(h, i) in el.head ?? []"
                    :key="i"
                    class="border border-outline-variant bg-surface-container-high px-0.5 py-0.5 text-[0.85em] font-semibold text-on-surface"
                    :style="{ width: `${el.headWidthPercentages?.[i] ?? 20}%` }"
                  >
                    {{ h }}
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="r in 2" :key="r">
                  <td
                    v-for="(h, i) in el.head ?? []"
                    :key="i"
                    class="border border-outline-variant px-0.5 py-0.5 text-[0.8em] text-secondary"
                    :style="{ textAlign: el.columnAlignment?.[i] ?? 'left' }"
                  >
                    —
                  </td>
                </tr>
              </tbody>
            </table>

            <span v-else-if="el.type === 'line'" class="block w-full bg-current" style="height: 1px" />

            <span v-else class="block truncate">{{ labelOf(el) }}</span>
          </div>
        </div>
      </div>

      <p class="text-center font-label-caption text-label-caption text-white/60 mt-2">
        A4 直式 210 × 297 mm，虛線為 15mm 安全邊距
      </p>
    </div>
  </div>
</template>
