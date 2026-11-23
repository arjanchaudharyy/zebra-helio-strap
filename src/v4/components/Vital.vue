<script setup lang="ts">
/* One vital in the Today strip: value, change vs your usual, and a 7-day mini chart. */

import { RouterLink } from 'vue-router';
import Spark from './Spark.vue';

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

</script>

<template>
  <component :is="to ? RouterLink : 'div'" :to="to" class="vital">
    <div class="v-label"><span class="v4-dot" :style="{ background: color }" />{{ label }}</div>
    <div class="v-value">
      <span class="v4-num">{{ value ?? '–' }}</span><span v-if="unit && value" class="v-unit">{{ unit }}</span>
    </div>
    <div :class="['v-change', tone ?? 'flat']">{{ change ?? 'No baseline yet' }}</div>
    <div class="v-trend"><Spark :values="week" :color="color" :height="30" /></div>
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
.v-value .v4-num { font-size: 30px; }
.v-unit { color: var(--v-muted); font-size: 12px; font-weight: 600; margin-left: 4px; }
.v-change { font-size: 12px; font-weight: 600; color: var(--v-faint); }
.v-change.good { color: var(--v-good); }
.v-change.bad { color: var(--v-move); }
.v-trend { position: relative; margin-top: 6px; }
</style>
