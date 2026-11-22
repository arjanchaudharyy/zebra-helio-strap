<script setup lang="ts">
/* A plain time line. Breaks the line wherever readings are further apart than `gapMs`. */
import { computed, ref } from 'vue';

export interface LinePoint { t: number; v: number }

const props = withDefaults(defineProps<{
  points: LinePoint[];
  color: string;
  height?: number;
  /** A dashed reference line (e.g. resting heart rate). */
  reference?: number | null;
  referenceLabel?: string;
  gapMs?: number;
  /** x-axis label formatter. */
  xLabel?: (t: number) => string;
  xTicks?: number;
  unit?: string;
  showDots?: boolean;
}>(), { height: 180, reference: null, referenceLabel: '', gapMs: 30 * 60 * 1000, xTicks: 5, unit: '', showDots: false, xLabel: undefined });

const W = 600;
const H = computed(() => props.height);
const PAD_T = 12;
const PAD_B = 4;

const domain = computed(() => {
  const ts = props.points.map((p) => p.t);
  const vs = props.points.map((p) => p.v).concat(props.reference != null ? [props.reference] : []);
  const t0 = Math.min(...ts);
  const t1 = Math.max(...ts);
  const lo = Math.min(...vs);
  const hi = Math.max(...vs);
  const pad = Math.max(2, (hi - lo) * 0.12);
  return { t0, t1: t1 === t0 ? t0 + 1 : t1, lo: Math.floor(lo - pad), hi: Math.ceil(hi + pad) };
});
const x = (t: number) => ((t - domain.value.t0) / (domain.value.t1 - domain.value.t0)) * W;
const y = (v: number) => PAD_T + (1 - (v - domain.value.lo) / (domain.value.hi - domain.value.lo)) * (H.value - PAD_T - PAD_B);

const path = computed(() => {
  let d = '';
  let prev: LinePoint | null = null;
  for (const p of props.points) {
    const move = !prev || p.t - prev.t > props.gapMs;
    d += `${move ? 'M' : 'L'}${x(p.t).toFixed(1)} ${y(p.v).toFixed(1)} `;
    prev = p;
  }
  return d;
});
const area = computed(() => {
  // One filled area per continuous run.
  const runs: LinePoint[][] = [];
  let run: LinePoint[] = [];
  let prev: LinePoint | null = null;
  for (const p of props.points) {
    if (prev && p.t - prev.t > props.gapMs) { runs.push(run); run = []; }
    run.push(p);
    prev = p;
  }
  if (run.length) runs.push(run);
  const base = H.value - PAD_B;
  return runs.filter((r) => r.length > 1).map((r) =>
    `M${x(r[0].t).toFixed(1)} ${base} ` + r.map((p) => `L${x(p.t).toFixed(1)} ${y(p.v).toFixed(1)}`).join(' ') + ` L${x(r[r.length - 1].t).toFixed(1)} ${base} Z`,
  ).join(' ');
});

const yTicks = computed(() => {
  const { lo, hi } = domain.value;
  const step = (hi - lo) / 3;
  return [0, 1, 2, 3].map((i) => Math.round(lo + step * i)).filter((v, i, a) => a.indexOf(v) === i);
});
const xTickList = computed(() => {
  if (!props.xLabel) return [];
  const { t0, t1 } = domain.value;
  return Array.from({ length: props.xTicks }, (_, i) => t0 + ((t1 - t0) * i) / (props.xTicks - 1));
});

const svg = ref<SVGSVGElement | null>(null);
const hover = ref<LinePoint | null>(null);
const onMove = (event: MouseEvent) => {
  const rect = svg.value?.getBoundingClientRect();
  if (!rect || !props.points.length) return;
  const t = domain.value.t0 + ((event.clientX - rect.left) / rect.width) * (domain.value.t1 - domain.value.t0);
  let best = props.points[0];
  for (const p of props.points) if (Math.abs(p.t - t) < Math.abs(best.t - t)) best = p;
  hover.value = best;
};
</script>

<template>
  <div class="line" :style="{ '--c': color }">
    <div class="line-y">
      <span v-for="v in [...yTicks].reverse()" :key="v" :style="{ top: `${(y(v) / H) * 100}%` }">{{ v }}</span>
    </div>
    <div class="line-main">
      <svg ref="svg" :viewBox="`0 0 ${W} ${H}`" preserveAspectRatio="none" :style="{ height: `${H}px` }" @mousemove="onMove" @mouseleave="hover = null">
        <line v-for="v in yTicks" :key="v" x1="0" :x2="W" :y1="y(v)" :y2="y(v)" class="grid" vector-effect="non-scaling-stroke" />
        <path :d="area" class="area" />
        <line v-if="reference != null" x1="0" :x2="W" :y1="y(reference)" :y2="y(reference)" class="ref" vector-effect="non-scaling-stroke" />
        <path :d="path" class="stroke" vector-effect="non-scaling-stroke" />
        <template v-if="showDots">
          <circle v-for="p in points" :key="p.t" :cx="x(p.t)" :cy="y(p.v)" r="3" class="dot" vector-effect="non-scaling-stroke" />
        </template>
        <line v-if="hover" :x1="x(hover.t)" :x2="x(hover.t)" y1="0" :y2="H" class="cursor" vector-effect="non-scaling-stroke" />
      </svg>
      <div v-if="hover" class="hover-dot" :style="{ left: `${(x(hover.t) / W) * 100}%`, top: `${y(hover.v)}px` }" />
      <div v-if="xLabel" class="line-x">
        <span v-for="t in xTickList" :key="t" :style="{ left: `${(x(t) / W) * 100}%` }">{{ xLabel(t) }}</span>
      </div>
      <div v-if="hover" class="tip"><strong>{{ Math.round(hover.v) }} {{ unit }}</strong> <span>{{ xLabel ? xLabel(hover.t) : '' }}</span></div>
      <div v-if="reference != null && referenceLabel" class="ref-label" :style="{ top: `${y(reference)}px` }">{{ referenceLabel }}</div>
    </div>
  </div>
</template>

<style scoped>
.line { display: grid; grid-template-columns: 32px 1fr; gap: 8px; }
.line-y { position: relative; }
.line-y span { position: absolute; right: 0; transform: translateY(-50%); font-size: 11px; color: var(--v-faint); }
.line-main { position: relative; padding-bottom: 22px; }
svg { width: 100%; display: block; overflow: visible; }
.grid { stroke: var(--v-line); stroke-width: 1; }
.area { fill: var(--c); opacity: 0.08; }
.stroke { fill: none; stroke: var(--c); stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; }
.ref { stroke: var(--v-faint); stroke-width: 1; stroke-dasharray: 4 4; }
.dot { fill: var(--v-surface); stroke: var(--c); stroke-width: 2; }
.cursor { stroke: var(--v-line-strong); stroke-width: 1; }
.hover-dot { position: absolute; width: 10px; height: 10px; border-radius: 50%; background: var(--c); border: 2px solid var(--v-surface); transform: translate(-50%, -50%); pointer-events: none; }
.line-x { position: absolute; left: 0; right: 0; bottom: 0; height: 16px; }
.line-x span { position: absolute; transform: translateX(-50%); font-size: 11px; color: var(--v-faint); white-space: nowrap; }
.line-x span:first-child { transform: none; }
.line-x span:last-child { transform: translateX(-100%); }
.tip { position: absolute; top: -26px; right: 0; font-size: 12px; color: var(--v-muted); }
.tip strong { color: var(--v-text); font-size: 13px; }
.ref-label { position: absolute; left: 6px; transform: translateY(-120%); font-size: 11px; color: var(--v-muted); pointer-events: none; }
</style>
