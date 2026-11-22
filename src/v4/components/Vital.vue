<script setup lang="ts">
/* One vital in the Today strip: value, change vs your usual, and a 7-day mini chart. */
import { computed } from 'vue';
import { RouterLink } from 'vue-router';

const props = defineProps<{
  label: string;
  value: string | null;
  unit?: string;
  color: string;
  /** Last 7 values, oldest first; null = no data that day. */
  week: (number | null)[];
  /** Signed change vs the person's usual, already formatted ("+3 bpm"). */
  change?: string | null;
  /** Whether the change is good, bad or neither. */
  tone?: 'good' | 'bad' | 'flat';
  to?: string;
}>();

const max = computed(() => Math.max(1, ...props.week.map((v) => v ?? 0)));
const min = computed(() => {
  const v = props.week.filter((x): x is number => x != null);
  return v.length ? Math.min(...v) * 0.85 : 0;
});
const h = (v: number) => `${Math.max(8, ((v - min.value) / (max.value - min.value || 1)) * 100)}%`;
</script>

<template>
  <component :is="to ? RouterLink : 'div'" :to="to" class="vital">
    <div class="v-label"><span class="v4-dot" :style="{ background: color }" />{{ label }}</div>
    <div class="v-value">
      <span class="v4-num">{{ value ?? '–' }}</span><span v-if="unit && value" class="v-unit">{{ unit }}</span>
    </div>
    <div :class="['v-change', tone ?? 'flat']">{{ change ?? 'No baseline yet' }}</div>
    <div class="v-week" aria-hidden="true">
      <span v-for="(v, i) in week" :key="i" :class="{ last: i === week.length - 1 }">
        <i v-if="v != null" :style="{ height: h(v), background: color }" />
      </span>
    </div>
  </component>
</template>

<style scoped>
.vital {
  display: grid; gap: 6px; padding: 16px 18px; min-width: 0;
  background: var(--v-surface); border: 1px solid var(--v-line); border-radius: var(--v-radius);
}
a.vital:hover { border-color: var(--v-line-strong); }
.v-label { color: var(--v-muted); font-size: 12px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.v-label .v4-dot { width: 6px; height: 6px; margin-right: 6px; }
.v-value { display: flex; align-items: baseline; }
.v-value .v4-num { font-size: 32px; }
.v-unit { color: var(--v-muted); font-size: 12px; font-weight: 600; margin-left: 4px; }
.v-change { font-size: 12px; font-weight: 600; color: var(--v-faint); }
.v-change.good { color: #4fd18b; }
.v-change.bad { color: var(--v-move); }
.v-week { display: flex; gap: 3px; height: 28px; align-items: flex-end; margin-top: 4px; }
.v-week span { flex: 1; height: 100%; display: flex; align-items: flex-end; background: var(--v-surface-2); border-radius: 2px; overflow: hidden; }
.v-week i { display: block; width: 100%; opacity: 0.55; border-radius: 2px; }
.v-week span.last i { opacity: 1; }
</style>
