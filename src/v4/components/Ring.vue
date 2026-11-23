<script setup lang="ts">
/* An Apple Activity-style ring: a full circle starting at 12 o'clock, a thick rounded stroke,
   and a dim track in the same colour. With `origin`, it fills from that value toward `value`
   (clockwise when higher, anticlockwise when lower), e.g. body age around real age. */
import { computed } from 'vue';
import { RouterLink } from 'vue-router';

const props = defineProps<{
  label: string;
  value: number | null;
  max: number;
  color: string;
  display: string;
  unit?: string;
  caption?: string;
  origin?: number | null;
  span?: number;      // with origin: the value range that one full turn represents
  to?: string;
}>();

const R = 44;
const C = 2 * Math.PI * R;
const progress = computed(() => {
  if (props.value == null) return 0;
  if (props.origin != null) return Math.max(-1, Math.min(1, (props.value - props.origin) / (props.span ?? 20)));
  return Math.max(0, Math.min(1, props.value / props.max));
});
const dash = computed(() => `${Math.max(0.0001, Math.abs(progress.value)) * C} ${C}`);
</script>

<template>
  <component :is="to ? RouterLink : 'div'" :to="to" class="ring">
    <div class="ring-art">
      <svg viewBox="0 0 120 120" aria-hidden="true">
        <circle cx="60" cy="60" :r="R" class="track" :style="{ stroke: color }" />
        <circle
          v-if="value != null && Math.abs(progress) > 0.002"
          cx="60" cy="60" :r="R" class="fill" :style="{ stroke: color, strokeDasharray: dash }"
          :transform="progress < 0 ? 'translate(120 0) scale(-1 1) rotate(-90 60 60)' : 'rotate(-90 60 60)'"
        />
      </svg>
      <div class="ring-center">
        <span class="v4-num ring-value">{{ value == null ? '–' : display }}</span>
        <span v-if="unit && value != null" class="ring-unit">{{ unit }}</span>
      </div>
    </div>
    <div class="ring-label">{{ label }}</div>
    <div class="ring-caption">{{ caption }}</div>
  </component>
</template>

<style scoped>
.ring { display: grid; justify-items: center; gap: 2px; text-align: center; padding: 6px 4px; border-radius: 16px; }
a.ring:hover { background: var(--v-surface-2); }
.ring-art { position: relative; width: min(150px, 100%); aspect-ratio: 1; }
svg { width: 100%; height: 100%; }
.track { fill: none; stroke-width: 15; opacity: 0.22; }
.fill { fill: none; stroke-width: 15; stroke-linecap: round; }
.ring-center { position: absolute; inset: 0; display: grid; place-items: center; align-content: center; }
.ring-value { font-size: 32px; letter-spacing: -0.03em; }
.ring-unit { color: var(--v-muted); font-size: 11px; font-weight: 600; margin-top: 2px; }
.ring-label { margin-top: 10px; font-size: 14px; font-weight: 600; letter-spacing: -0.01em; }
.ring-caption { color: var(--v-muted); font-size: 12px; min-height: 16px; }
</style>
