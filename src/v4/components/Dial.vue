<script setup lang="ts">
/* A ring in a recessed well with a gradient arc (Bevel-style). `marker` shows a position
   instead of a fill (for values where "more" is not "better"); `band` shades the usual range. */
import { computed } from 'vue';
import { RouterLink } from 'vue-router';

const props = defineProps<{
  label: string;
  value: number | null;
  min: number;
  max: number;
  color: string;
  color2?: string;
  display: string;
  unit?: string;
  caption?: string;
  band?: [number, number] | null;
  marker?: boolean;
  to?: string;
}>();

const uid = `g${Math.random().toString(36).slice(2, 8)}`;
const SIZE = 200;
const C = SIZE / 2;
const R = 82;
const START = 135;
const SWEEP = 270;
const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
const frac = (v: number) => clamp01((v - props.min) / (props.max - props.min));
const point = (deg: number, r = R) => {
  const a = (deg * Math.PI) / 180;
  return [C + r * Math.cos(a), C + r * Math.sin(a)] as const;
};
const arc = (from: number, to: number) => {
  const a0 = START + SWEEP * from;
  const a1 = START + SWEEP * to;
  const [x0, y0] = point(a0);
  const [x1, y1] = point(a1);
  return `M ${x0.toFixed(2)} ${y0.toFixed(2)} A ${R} ${R} 0 ${a1 - a0 > 180 ? 1 : 0} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
};
const track = arc(0, 1);
const fill = computed(() => (props.value == null || props.marker ? null : arc(0, Math.max(0.006, frac(props.value)))));
const bandPath = computed(() => (props.band ? arc(frac(props.band[0]), Math.max(frac(props.band[0]) + 0.012, frac(props.band[1]))) : null));
const knob = computed(() => {
  if (props.value == null) return null;
  const [x, y] = point(START + SWEEP * frac(props.value));
  return { x, y };
});
</script>

<template>
  <component :is="to ? RouterLink : 'div'" :to="to" class="dial" :aria-label="`${label}: ${value == null ? 'no data' : `${display} ${unit ?? ''}`}`">
    <svg :viewBox="`0 0 ${SIZE} ${SIZE}`" class="dial-svg" aria-hidden="true">
      <defs>
        <linearGradient :id="uid" x1="0" y1="1" x2="1" y2="0">
          <stop offset="0" :stop-color="color2 ?? color" />
          <stop offset="1" :stop-color="color" />
        </linearGradient>
        <filter :id="`${uid}-glow`" x="-30%" y="-30%" width="160%" height="160%">
          <feGaussianBlur stdDeviation="5" />
        </filter>
      </defs>
      <path :d="track" class="well" />
      <path v-if="bandPath" :d="bandPath" class="band" :style="{ stroke: color }" />
      <template v-if="fill">
        <path :d="fill" class="glow" :stroke="`url(#${uid})`" :filter="`url(#${uid}-glow)`" />
        <path :d="fill" class="fill" :stroke="`url(#${uid})`" />
      </template>
      <circle v-if="knob && marker" :cx="knob.x" :cy="knob.y" r="9" class="knob" :style="{ fill: color }" />
      <circle v-if="knob && marker" :cx="knob.x" :cy="knob.y" r="3.5" class="knob-dot" />
    </svg>
    <div class="dial-center">
      <div class="dial-value">
        <span class="v4-num">{{ value == null ? '–' : display }}</span><span v-if="unit && value != null" class="dial-unit">{{ unit }}</span>
      </div>
      <div class="dial-label">{{ label }}</div>
      <div class="dial-caption">{{ caption }}</div>
    </div>
  </component>
</template>

<style scoped>
.dial { position: relative; display: grid; place-items: center; aspect-ratio: 1; width: 100%; max-width: 210px; margin: 0 auto; }
.dial-svg { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; }
.well { fill: none; stroke: var(--v-well); stroke-width: 15; stroke-linecap: round; }
.band { fill: none; stroke-width: 15; stroke-linecap: butt; opacity: 0.22; }
.glow { fill: none; stroke-width: 15; stroke-linecap: round; opacity: 0.45; }
.fill { fill: none; stroke-width: 15; stroke-linecap: round; }
.knob { stroke: var(--v-bg); stroke-width: 3; }
.knob-dot { fill: var(--v-bg); }
.dial-center { position: relative; text-align: center; }
.dial-value { display: flex; align-items: baseline; justify-content: center; }
.dial-value .v4-num { font-size: clamp(36px, 3.9vw, 50px); }
.dial-unit { color: var(--v-muted); font-size: 13px; font-weight: 600; margin-left: 3px; }
.dial-label { color: var(--v-text); font-size: 13px; font-weight: 600; margin-top: 6px; }
.dial-caption { color: var(--v-muted); font-size: 12px; margin-top: 2px; min-height: 16px; }
</style>
