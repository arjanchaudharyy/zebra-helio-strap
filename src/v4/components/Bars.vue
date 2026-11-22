<script setup lang="ts">
/* Daily bars. Days with no data get no bar and a small dash, never a zero bar. */
import { computed, ref } from 'vue';

export interface Bar { key: string; label: string; value: number | null; tip?: string }

const props = withDefaults(defineProps<{
  bars: Bar[];
  color: string;
  height?: number;
  /** A dashed reference line, e.g. the person's usual value or goal. */
  reference?: number | null;
  referenceLabel?: string;
  max?: number;
  /** Show every n-th x label. */
  labelEvery?: number;
  format?: (value: number) => string;
}>(), { height: 160, reference: null, referenceLabel: '', labelEvery: 1, format: (v: number) => Math.round(v).toLocaleString('en-GB') });

const top = computed(() => {
  const values = props.bars.map((b) => b.value ?? 0);
  return Math.max(props.max ?? 0, props.reference ?? 0, ...values, 1) * 1.08;
});
const pct = (v: number) => `${(v / top.value) * 100}%`;
const hover = ref<number | null>(null);
const gap = computed(() => (props.bars.length > 90 ? 1 : props.bars.length > 40 ? 2 : 4));
const hovered = computed(() => (hover.value == null ? null : props.bars[hover.value]));
</script>

<template>
  <div class="bars" :style="{ height: `${height}px` }" @mouseleave="hover = null">
    <div class="bars-plot" :style="{ gap: `${gap}px` }">
      <div v-if="reference != null" class="ref" :style="{ bottom: pct(reference) }">
        <span v-if="referenceLabel">{{ referenceLabel }}</span>
      </div>
      <div
        v-for="(bar, i) in bars"
        :key="bar.key"
        class="col"
        :class="{ dim: hover != null && hover !== i }"
        @mouseenter="hover = i"
      >
        <div v-if="bar.value != null" class="bar" :style="{ height: pct(bar.value), background: color }" />
        <div v-else class="gap" />
      </div>
    </div>
    <div class="bars-axis" :style="{ gap: `${gap}px` }">
      <span v-for="(bar, i) in bars" :key="bar.key" class="tick">{{ i % labelEvery === 0 || i === bars.length - 1 ? bar.label : '' }}</span>
    </div>
    <div v-if="hovered" class="tip" role="status">
      <strong>{{ hovered.value == null ? 'No data' : format(hovered.value) }}</strong>
      <span>{{ hovered.tip ?? hovered.label }}</span>
    </div>
  </div>
</template>

<style scoped>
.bars { position: relative; display: grid; grid-template-rows: 1fr auto; }
.bars-plot { position: relative; display: flex; align-items: flex-end; gap: 4px; min-height: 0; border-bottom: 1px solid var(--v-line); }
.col { flex: 1; height: 100%; display: flex; align-items: flex-end; justify-content: center; cursor: default; transition: opacity 0.12s ease; }
.col.dim { opacity: 0.4; }
.bar { width: 100%; max-width: 22px; border-radius: 4px 4px 1px 1px; min-height: 2px; }
.gap { width: 8px; height: 2px; border-radius: 2px; background: var(--v-line-strong); margin-bottom: 4px; }
.ref { position: absolute; left: 0; right: 0; border-top: 1px dashed var(--v-faint); pointer-events: none; z-index: 1; }
.ref span { position: absolute; right: 0; top: -20px; font-size: 12px; color: var(--v-muted); background: var(--v-surface); padding: 0 0 0 6px; }
.bars-axis { display: flex; gap: 4px; margin-top: 8px; }
.tick { flex: 1; text-align: center; font-size: 11px; color: var(--v-faint); white-space: nowrap; overflow: visible; }
.tip {
  position: absolute; top: -6px; left: 0; display: flex; gap: 8px; align-items: baseline;
  font-size: 12px; color: var(--v-muted); pointer-events: none;
}
.tip strong { color: var(--v-text); font-size: 13px; }
</style>
