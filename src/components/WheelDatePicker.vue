<script setup lang="ts">
import { computed } from 'vue';
import WheelColumn from './WheelColumn.vue';
import { clampDate, daysInMonth, wheelDateValue } from '../lib/wheel/momentum';
import { defineMessages, locale, useMessages } from '../i18n';

const props = withDefaults(defineProps<{
  modelValue: string | null;
  min?: string;
  max?: string;
  ariaLabel?: string;
}>(), { min: undefined, max: undefined, ariaLabel: undefined });
const emit = defineEmits<{ 'update:modelValue': [value: string] }>();

const t = useMessages(defineMessages(
  { year: 'Year', month: 'Month', day: 'Day', yearSuffix: '', daySuffix: '' },
));

const today = () => {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
};
const value = computed(() => wheelDateValue(props.modelValue, props.min, today()));
const parts = computed(() => {
  const [y, m, d] = value.value.split('-').map(Number);
  return { y: y!, m: m!, d: d! };
});

const thisYear = new Date().getFullYear();
const years = computed(() => {
  const low = Math.min(props.min ? Number(props.min.slice(0, 4)) : thisYear - 10, parts.value.y);
  const high = Math.max(props.max ? Number(props.max.slice(0, 4)) : thisYear + 1, parts.value.y);
  return Array.from({ length: high - low + 1 }, (_, i) => ({ value: low + i, label: `${low + i}${t.value.yearSuffix}` }));
});
const months = computed(() => {
  const fmt = new Intl.DateTimeFormat(locale.value, { month: 'short' });
  return Array.from({ length: 12 }, (_, i) => ({ value: i + 1, label: fmt.format(new Date(2026, i, 1)) }));
});
const days = computed(() => Array.from({ length: daysInMonth(parts.value.y, parts.value.m) }, (_, i) => ({
  value: i + 1, label: `${i + 1}${t.value.daySuffix}`,
})));

const set = (patch: Partial<{ y: number; m: number; d: number }>) => {
  const next = { ...parts.value, ...patch };
  const iso = clampDate(next.y, next.m, next.d, props.min, props.max);
  if (iso !== props.modelValue) emit('update:modelValue', iso);
};

const order = ['m', 'd', 'y'] as const;
</script>

<template>
  <div class="wheel-date" role="group" :aria-label="ariaLabel">
    <template v-for="key in order" :key="key">
      <WheelColumn v-if="key === 'y'" class="col-y" :items="years" :model-value="parts.y" :label="t.year" @update:model-value="set({ y: $event })" />
      <WheelColumn v-else-if="key === 'm'" class="col-m" :items="months" :model-value="parts.m" :label="t.month" @update:model-value="set({ m: $event })" />
      <WheelColumn v-else class="col-d" :items="days" :model-value="parts.d" :label="t.day" @update:model-value="set({ d: $event })" />
    </template>
  </div>
</template>

<style scoped>
.wheel-date {
  display: flex;
  gap: 2px;
  padding: 3px;
  border-radius: 22px;
  background: var(--cap-track);
  box-shadow: var(--cap-track-shadow);
}
.wheel-date > * { flex: 1 1 0; }
.wheel-date > .col-y { flex-grow: 1.3; }
.wheel-date > .col-m, .wheel-date > .col-d { min-width: 68px; }
.wheel-date > .col-y { min-width: 88px; }
</style>
