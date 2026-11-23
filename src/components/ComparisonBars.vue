<script setup lang="ts">
import { computed } from 'vue';
const props = defineProps<{
  current: number; baseline: number; currentLabel: string; baselineLabel: string;
  currentText: string; baselineText: string; tone: 'good' | 'bad' | 'neutral' | 'flat' | 'up' | 'down';
  color?: string | null;
  uncertain?: boolean;
}>();
const maximum = computed(() => Math.max(props.current, props.baseline, 1));
const percent = (value: number) => `${Math.max(0, value) / maximum.value * 100}%`;
</script>

<template>
  <div :class="['comparison-bars', { tinted: !!color }]" :style="color ? { '--bar': color } : undefined">
    <div class="comparison-row">
      <span>{{ currentLabel }}</span><span class="comparison-number">{{ currentText }}</span>
      <span :class="['comparison-track', 'track-' + tone]"><i :class="tone" :style="{ width: percent(current) }"></i></span>
    </div>
    <div class="comparison-row">
      <span>{{ baselineLabel }}</span><span class="comparison-number">{{ baselineText }}</span>
      <span :class="['comparison-track', 'track-' + tone]"><i :class="['baseline', { 'sfumato-end': uncertain }]" :style="{ width: percent(baseline) }"></i></span>
    </div>
  </div>
</template>

<style scoped>
.comparison-bars { display: grid; gap: 10px; margin-block: 10px 6px; }
.comparison-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 5px 10px; color: var(--subtle); font-size: var(--fs-2xs); }
.comparison-number { text-align: right; white-space: nowrap; font-variant-numeric: tabular-nums; }
.comparison-track { grid-column: 1 / -1; height: 6px; border-radius: 6px; overflow: hidden; background: rgba(232,238,244,.08); }
.comparison-track i { display: block; height: 100%; border-radius: inherit; background: var(--muted); }
.comparison-track .good, .comparison-track .bad { position: relative; overflow: hidden; background-color: var(--accent); }
.comparison-track .good::before, .comparison-track .bad::before { content: ""; position: absolute; top: 0; bottom: 0; left: -24px; width: calc(100% + 24px); background-image: repeating-linear-gradient(-45deg, transparent 0 6px, rgba(255,255,255,.28) 6px 10px, transparent 10px 16px); background-size: 24px 24px; }
.comparison-track .bad { background-color: var(--danger); }
.comparison-track .neutral { background: color-mix(in srgb, var(--ink) 55%, transparent); }
.comparison-track .baseline { background: rgba(232,238,244,.24); }
.comparison-track .up { background: color-mix(in srgb, var(--accent) 62%, transparent); }
.comparison-track .down { background: color-mix(in srgb, var(--danger) 62%, transparent); }
.track-good, .track-up { background: color-mix(in srgb, var(--accent) 10%, transparent); }
.track-bad, .track-down { background: color-mix(in srgb, var(--danger) 10%, transparent); }
.track-good .baseline, .track-up .baseline { background: color-mix(in srgb, var(--accent) 34%, transparent); }
.track-bad .baseline, .track-down .baseline { background: color-mix(in srgb, var(--danger) 34%, transparent); }
.tinted .comparison-track, .tinted .comparison-track[class*='track-'] { background: color-mix(in srgb, var(--bar) 10%, transparent); }
.tinted .comparison-track i:not(.baseline) { background-color: var(--bar); }
.tinted .comparison-track .baseline { background: color-mix(in srgb, var(--bar) 36%, transparent); }
</style>
