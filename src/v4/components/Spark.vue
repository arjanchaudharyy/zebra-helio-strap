<script setup lang="ts">
/* A small trend line. Days without data are skipped (the line joins the days that have data),
   and the last point is marked. */
import { computed } from 'vue';

const props = withDefaults(defineProps<{ values: (number | null)[]; color: string; height?: number }>(), { height: 34 });
const W = 120;
const pts = computed(() => {
  const n = props.values.length;
  const real = props.values.map((v, i) => ({ v, i })).filter((p): p is { v: number; i: number } => p.v != null);
  if (!real.length) return [];
  const lo = Math.min(...real.map((p) => p.v));
  const hi = Math.max(...real.map((p) => p.v));
  const span = hi - lo || 1;
  return real.map((p) => ({ x: n > 1 ? (p.i / (n - 1)) * W : W / 2, y: 4 + (1 - (p.v - lo) / span) * (props.height - 8) }));
});
const d = computed(() => pts.value.map((p, i) => `${i ? 'L' : 'M'}${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(' '));
const last = computed(() => pts.value[pts.value.length - 1]);
</script>

<template>
  <svg class="spark" :viewBox="`0 0 ${W} ${height}`" preserveAspectRatio="none" :style="{ height: `${height}px` }" aria-hidden="true">
    <path v-if="pts.length > 1" :d="d" :stroke="color" class="line" vector-effect="non-scaling-stroke" />
    <line v-else :x1="0" :x2="W" :y1="height / 2" :y2="height / 2" class="flat" vector-effect="non-scaling-stroke" />
  </svg>
  <span v-if="last" class="dot" :style="{ left: `calc(${(last.x / W) * 100}% - 3px)`, top: `${last.y - 3}px`, background: color }" />
</template>

<style scoped>
.spark { width: 100%; display: block; overflow: visible; }
.line { fill: none; stroke-width: 1.75; stroke-linejoin: round; stroke-linecap: round; opacity: 0.9; }
.flat { stroke: var(--v-line-strong); stroke-width: 1; stroke-dasharray: 3 3; }
.dot { position: absolute; width: 6px; height: 6px; border-radius: 50%; box-shadow: 0 0 0 2px var(--v-surface); }
</style>
