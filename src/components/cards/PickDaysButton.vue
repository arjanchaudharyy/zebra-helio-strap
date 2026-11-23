<script setup lang="ts">
import { computed, defineAsyncComponent, ref, shallowRef } from 'vue';
import Icon from '../Icon.vue';
import { useCardsText } from './cards.i18n';
import { useCardCollection } from '../../composables/useCardCollection';
import { useTrendRange } from '../../composables/useTrendRange';
import { askCategoryOf } from '../../lib/metricAsk';
import { deckRangeOf } from '../../lib/cards/deck';
import { categorySource, metricSource, PAIRED_METRICS, type DeckSource } from '../../lib/cards/sources';
import { metricColor } from '../../lib/metricTone';
import { formatPaceSeconds } from '../../lib/metricSeries';
import { paceUnitLabel } from '../../lib/units';
import type { AiTaskCategory } from '../../lib/bridge/types';

const CardTable = defineAsyncComponent(() => import('./CardTable.vue'));

const props = defineProps<{
  metric?: string | null;
  category?: AiTaskCategory;
  label: string;
  tint: string;
  format: ((value: number) => string) | null;
  unit?: string;
}>();
const t = useCardsText();
const box = useCardCollection();
const range = useTrendRange();
const open = ref(false);
const origin = shallowRef<DOMRect | null>(null);
const button = ref<HTMLElement | null>(null);
const table = ref<{ reopen: () => void; isClosing: () => boolean } | null>(null);
const key = computed(() => (props.category ? `cat:${props.category}` : props.metric ?? ''));
const pickedHere = computed(() => (key.value ? box.picks.value.filter((pick) => pick.key === key.value).length : 0));
const source = computed<DeckSource>(() => (props.category
  ? categorySource({ category: props.category, label: props.label, tint: props.tint, format: props.format, unit: props.unit })
  : metricSource({
    metric: props.metric ?? '',
    category: askCategoryOf(props.metric ?? ''),
    label: props.label,
    tint: props.tint,
    format: props.format ?? ((value: number) => String(value)),
    unit: props.unit,
    second: PAIRED_METRICS[props.metric ?? ''] ? { tint: metricColor(PAIRED_METRICS[props.metric ?? '']!), format: (seconds: number) => formatPaceSeconds(seconds), unit: paceUnitLabel() } : undefined,
  })));

const show = (event: MouseEvent) => {
  if (open.value && table.value?.isClosing()) { table.value.reopen(); return; }
  origin.value = (event.currentTarget as HTMLElement).getBoundingClientRect();
  open.value = true;
};
</script>

<template>
  <button v-if="key" ref="button" type="button" :class="['pick-days', { lit: pickedHere > 0 }]" :title="t.pickTitle(label)" :aria-label="t.pickTitle(label)" aria-haspopup="dialog" @click="show">
    <Icon name="cards" :size="16" /><span>{{ t.pick }}</span><b v-if="pickedHere">{{ pickedHere }}</b>
  </button>
  <CardTable v-if="open" ref="table" :source="source" :range="deckRangeOf(range)" :origin="origin" :anchor="button" @close="open = false" />
</template>

<style scoped>
.pick-days { display: inline-flex; align-items: center; justify-content: center; gap: 4px; height: 30px; padding: 0 11px 0 9px; white-space: nowrap; border: 0; border-radius: 999px;
  background: var(--mat-inset); box-shadow: var(--mat-inset-shadow); color: var(--muted); font: inherit; font-size: var(--fs-xs); font-weight: 650;
  font-variant-numeric: tabular-nums; cursor: pointer; transition: background var(--dur-base) ease, color var(--dur-base) ease, transform 160ms ease; }
.pick-days:hover { background: color-mix(in srgb, var(--ink) 8%, var(--mat-inset)); color: var(--ink); }
.pick-days:active { transform: scale(.94); }
.pick-days.lit { background: color-mix(in srgb, var(--accent) 18%, var(--mat-inset)); color: var(--accent); }
.pick-days b { min-width: 18px; padding: 1px 6px; border-radius: 999px; background: var(--accent); color: var(--accent-ink, #10140a); font-size: var(--fs-2xs); text-align: center; }
.pick-days:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
.pick-days :deep(svg) { transition: transform 260ms cubic-bezier(.3, 1.4, .5, 1); }
.pick-days.is-receiving :deep(svg) { transform: rotate(-14deg) scale(1.25); }
</style>
