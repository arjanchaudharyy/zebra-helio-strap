<script setup lang="ts">
/* One night as proportional stage segments, plus a legend with minutes. */
import { computed } from 'vue';
import { duration } from '../data';

const props = defineProps<{ deep?: number | null; light?: number | null; rem?: number | null; awake?: number | null; compact?: boolean }>();

const stages = computed(() => [
  { key: 'deep', label: 'Deep', minutes: props.deep, color: 'var(--v-stage-deep)' },
  { key: 'light', label: 'Light', minutes: props.light, color: 'var(--v-stage-light)' },
  { key: 'rem', label: 'REM', minutes: props.rem, color: 'var(--v-stage-rem)' },
  { key: 'awake', label: 'Awake', minutes: props.awake, color: 'var(--v-stage-awake)' },
].filter((s) => s.minutes != null && s.minutes > 0) as { key: string; label: string; minutes: number; color: string }[]);
const total = computed(() => stages.value.reduce((sum, s) => sum + s.minutes, 0));
</script>

<template>
  <div v-if="stages.length" class="stages">
    <div class="track" role="img" :aria-label="stages.map((s) => `${s.label} ${duration(s.minutes)}`).join(', ')">
      <span v-for="s in stages" :key="s.key" :style="{ flexGrow: s.minutes, background: s.color }" />
    </div>
    <ul v-if="!compact" class="legend">
      <li v-for="s in stages" :key="s.key">
        <span class="v4-dot" :style="{ background: s.color }" />
        <span class="name">{{ s.label }}</span>
        <span class="mins">{{ duration(s.minutes) }}</span>
        <span class="pct">{{ Math.round((s.minutes / total) * 100) }}%</span>
      </li>
    </ul>
  </div>
</template>

<style scoped>
.track { display: flex; gap: 3px; height: 12px; }
.track span { border-radius: 3px; min-width: 3px; }
.legend { list-style: none; margin: 18px 0 0; padding: 0; display: grid; gap: 10px; }
.legend li { display: grid; grid-template-columns: auto 1fr auto 44px; align-items: center; font-size: 14px; }
.name { color: var(--v-muted); }
.mins { font-weight: 600; }
.pct { text-align: right; color: var(--v-faint); font-size: 13px; }
</style>
