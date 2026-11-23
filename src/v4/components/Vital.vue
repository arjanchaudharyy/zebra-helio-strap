<script setup lang="ts">
/* One vital, Apple Health style: icon and name in the metric colour, the value, one plain sentence,
   and the last 7 days as small bars (today highlighted). */
import { computed } from 'vue';
import { RouterLink } from 'vue-router';
import Symbol from './Symbol.vue';

const props = defineProps<{
  label: string;
  value: string | null;
  unit?: string;
  color: string;
  icon?: string;
  /** Last 7 values, oldest first; null = no data that day. */
  week: (number | null)[];
  /** One plain sentence about the value ("3 beats higher than usual"). */
  change?: string | null;
  tone?: 'good' | 'bad' | 'flat';
  to?: string;
}>();

const bars = computed(() => {
  const vals = props.week.filter((v): v is number => v != null);
  if (!vals.length) return props.week.map(() => null);
  const lo = Math.min(...vals);
  const hi = Math.max(...vals);
  return props.week.map((v) => (v == null ? null : 0.28 + 0.72 * (hi === lo ? 0.7 : (v - lo) / (hi - lo))));
});
const days = computed(() => {
  const out: string[] = [];
  for (let i = props.week.length - 1; i >= 0; i--) {
    const d = new Date(Date.now() - i * 86400000);
    out.push(d.toLocaleDateString('en-GB', { weekday: 'narrow' }));
  }
  return out;
});
</script>

<template>
  <component :is="to ? RouterLink : 'div'" :to="to" class="vital">
    <div class="left">
      <div class="label" :style="{ color }"><Symbol v-if="icon" :name="icon" :size="15" />{{ label }}</div>
      <div class="value"><span class="v4-num">{{ value ?? '–' }}</span><span v-if="unit && value" class="unit">{{ unit }}</span></div>
      <div :class="['change', tone ?? 'flat']">{{ change ?? 'No data yet' }}</div>
    </div>
    <div class="bars" aria-hidden="true">
      <div v-for="(b, i) in bars" :key="i" class="col">
        <div class="track"><i v-if="b != null" :style="{ height: `${b * 100}%`, background: color, opacity: i === bars.length - 1 ? 1 : 0.35 }" /></div>
        <span :class="{ today: i === bars.length - 1 }">{{ days[i] }}</span>
      </div>
    </div>
  </component>
</template>

<style scoped>
.vital {
  display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 16px;
  padding: 16px 18px; min-width: 0; background: var(--v-surface); border-radius: var(--v-radius);
}
a.vital:hover { background: var(--v-surface-2); }
.label { display: flex; align-items: center; gap: 6px; font-size: 13px; font-weight: 600; letter-spacing: -0.01em; }
.value { display: flex; align-items: baseline; margin-top: 6px; }
.value .v4-num { font-size: 28px; }
.unit { color: var(--v-muted); font-size: 13px; font-weight: 600; margin-left: 4px; }
.change { margin-top: 3px; font-size: 13px; color: var(--v-muted); }
.change.good { color: var(--v-good); }
.change.bad { color: var(--v-move); }
.bars { display: flex; gap: 5px; align-items: flex-end; }
.col { display: grid; justify-items: center; gap: 4px; }
.track { width: 9px; height: 44px; border-radius: 5px; background: var(--v-well); display: flex; align-items: flex-end; overflow: hidden; }
.track i { display: block; width: 100%; border-radius: 5px; }
.col span { font-size: 10px; color: var(--v-faint); }
.col span.today { color: var(--v-text); font-weight: 700; }
</style>
