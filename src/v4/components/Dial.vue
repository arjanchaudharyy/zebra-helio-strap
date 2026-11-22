<script setup lang="ts">
/* A watch-bezel dial: a 270° track with minute ticks, the value as a filled arc,
   and (optionally) the person's usual range drawn as a faint band. */
import { computed } from 'vue';

const props = defineProps<{
  label: string;
  value: number | null;
  min: number;
  max: number;
  color: string;
  display: string;
  unit?: string;
  caption?: string;
  /** The usual range, drawn behind the value. */
  band?: [number, number] | null;
  /** Draw a marker at the value instead of a filled arc (for values where "more" is not "better"). */
  marker?: boolean;
  to?: string;
}>();

const SIZE = 220;
const C = SIZE / 2;
const R = 92;
const START = 135; // degrees, measured clockwise from 3 o'clock
const SWEEP = 270;

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
const frac = (v: number) => clamp01((v - props.min) / (props.max - props.min));
const point = (deg: number, r = R) => {
  const a = (deg * Math.PI) / 180;
  return [C + r * Math.cos(a), C + r * Math.sin(a)] as const;
};
const arc = (from: number, to: number, r = R) => {
  const a0 = START + SWEEP * from;
  const a1 = START + SWEEP * to;
  const [x0, y0] = point(a0, r);
  const [x1, y1] = point(a1, r);
  const large = a1 - a0 > 180 ? 1 : 0;
  return `M ${x0.toFixed(2)} ${y0.toFixed(2)} A ${r} ${r} 0 ${large} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
};

const track = arc(0, 1);
const fill = computed(() => (props.value == null || props.marker ? null : arc(0, Math.max(0.004, frac(props.value)))));
const bandPath = computed(() => (props.band ? arc(frac(props.band[0]), Math.max(frac(props.band[0]) + 0.01, frac(props.band[1]))) : null));
const markerAt = computed(() => {
  if (props.value == null || !props.marker) return null;
  const deg = START + SWEEP * frac(props.value);
  const [x0, y0] = point(deg, R - 12);
  const [x1, y1] = point(deg, R + 12);
  return { x0, y0, x1, y1 };
});
const ticks = Array.from({ length: 46 }, (_, i) => {
  const deg = START + (SWEEP * i) / 45;
  const major = i % 5 === 0;
  const [x0, y0] = point(deg, R + 16);
  const [x1, y1] = point(deg, R + (major ? 23 : 19));
  return { x0, y0, x1, y1, major };
});
</script>

<template>
  <component :is="to ? 'RouterLink' : 'div'" :to="to" class="dial" :aria-label="`${label}: ${value == null ? 'no data' : `${display} ${unit ?? ''}`}`">
    <svg :viewBox="`0 0 ${SIZE} ${SIZE}`" class="dial-svg" aria-hidden="true">
      <line v-for="(t, i) in ticks" :key="i" :x1="t.x0" :y1="t.y0" :x2="t.x1" :y2="t.y1" :class="['tick', { major: t.major }]" />
      <path :d="track" class="track" />
      <path v-if="bandPath" :d="bandPath" class="band" :style="{ stroke: color }" />
      <path v-if="fill" :d="fill" class="fill" :style="{ stroke: color }" />
      <line v-if="markerAt" :x1="markerAt.x0" :y1="markerAt.y0" :x2="markerAt.x1" :y2="markerAt.y1" class="marker" :style="{ stroke: color }" />
    </svg>
    <div class="dial-center">
      <div class="dial-label">{{ label }}</div>
      <div class="dial-value">
        <span class="v4-num">{{ value == null ? '–' : display }}</span><span v-if="unit && value != null" class="dial-unit">{{ unit }}</span>
      </div>
      <div class="dial-caption">{{ caption }}</div>
    </div>
  </component>
</template>

<style scoped>
.dial { position: relative; display: grid; place-items: center; aspect-ratio: 1; width: 100%; max-width: 240px; margin: 0 auto; }
.dial-svg { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; }
.tick { stroke: var(--v-line-strong); stroke-width: 1.4; stroke-linecap: round; }
.tick.major { stroke: var(--v-faint); stroke-width: 2; }
.track { fill: none; stroke: var(--v-surface-2); stroke-width: 14; stroke-linecap: round; }
.band { fill: none; stroke-width: 14; stroke-linecap: butt; opacity: 0.22; }
.fill { fill: none; stroke-width: 14; stroke-linecap: round; }
.marker { stroke-width: 5; stroke-linecap: round; }
.dial-center { position: relative; text-align: center; padding-top: 6px; }
.dial-label { color: var(--v-muted); font-size: 13px; font-weight: 600; }
.dial-value { margin-top: 4px; display: flex; align-items: baseline; justify-content: center; }
.dial-value .v4-num { font-size: 64px; }
.dial-unit { color: var(--v-muted); font-size: 15px; font-weight: 600; margin-left: 4px; }
.dial-caption { color: var(--v-muted); font-size: 13px; margin-top: 6px; min-height: 18px; }
a.dial:hover .track { stroke: var(--v-line); }
</style>
