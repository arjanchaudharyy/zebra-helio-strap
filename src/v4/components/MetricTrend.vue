<script setup lang="ts">
/* One metric: plain-language title and meaning, latest value, usual range, and its trend line. */
import { computed } from 'vue';
import LineChart from './LineChart.vue';
import { median, quantile, shortDay } from '../data';
import type { MetricSeries } from '../../types';

const props = defineProps<{
  title: string;
  meaning: string;
  series?: MetricSeries;
  unit?: string;
  color: string;
  digits?: number;
  /** Optional second series drawn as the usual/baseline reference value. */
  baseline?: MetricSeries;
  higherIsBetter?: boolean | null;
}>();

const pts = computed(() => (props.series?.points ?? []).filter((p) => Number.isFinite(p.value)));
const points = computed(() => pts.value.map((p) => ({ t: new Date(`${p.date}T12:00:00`).getTime(), v: p.value })));
const latest = computed(() => pts.value[pts.value.length - 1] ?? null);
const prior = computed(() => pts.value.slice(-31, -1).map((p) => p.value));
const usual = computed(() => {
  const b = props.baseline?.points ?? [];
  if (b.length) return b[b.length - 1].value;
  return median(prior.value);
});
const band = computed(() => (prior.value.length >= 7 ? [quantile(prior.value, 0.1)!, quantile(prior.value, 0.9)!] : null));
const fmt = (v: number) => v.toFixed(props.digits ?? 0);
const verdict = computed(() => {
  if (!latest.value || usual.value == null) return null;
  const d = latest.value.value - usual.value;
  const spread = band.value ? (band.value[1] - band.value[0]) / 2 : Math.abs(usual.value) * 0.08;
  if (Math.abs(d) <= Math.max(spread * 0.5, 10 ** -(props.digits ?? 0))) return { text: 'About usual', tone: 'flat' };
  const up = d > 0;
  const good = props.higherIsBetter == null ? null : up === props.higherIsBetter;
  return { text: `${up ? 'Above' : 'Below'} your usual`, tone: good == null ? 'flat' : good ? 'good' : 'bad' };
});
const dayLabel = (t: number) => shortDay(new Date(t).toISOString().slice(0, 10));
</script>

<template>
  <section class="v4-card trend">
    <div class="t-head">
      <div>
        <h2 class="v4-card-title">{{ title }}</h2>
        <p class="t-meaning">{{ meaning }}</p>
      </div>
      <div v-if="latest" class="t-now">
        <div><span class="v4-num t-val">{{ fmt(latest.value) }}</span><span v-if="unit" class="v4-unit">{{ unit }}</span></div>
        <div :class="['t-verdict', verdict?.tone]">{{ verdict?.text ?? shortDay(latest.date) }}</div>
      </div>
    </div>
    <LineChart v-if="points.length > 1" :points="points" :color="color" :height="140" :reference="usual" reference-label="Usual" :gap-ms="4 * 86400000" :x-label="dayLabel" :unit="unit ?? ''" />
    <p v-else class="v4-empty">No readings yet.</p>
    <p v-if="usual != null && latest" class="t-foot">Usual {{ fmt(usual) }}{{ unit ? ` ${unit}` : '' }} · {{ pts.length }} days of data since {{ shortDay(pts[0].date) }}</p>
  </section>
</template>

<style scoped>
.t-head { display: flex; justify-content: space-between; gap: 16px; margin-bottom: 14px; }
.t-meaning { color: var(--v-muted); font-size: 13px; margin: 4px 0 0; max-width: 46ch; }
.t-now { text-align: right; flex: none; }
.t-val { font-size: 32px; }
.t-verdict { font-size: 12px; font-weight: 600; color: var(--v-muted); margin-top: 4px; }
.t-verdict.good { color: var(--v-good); }
.t-verdict.bad { color: var(--v-move); }
.t-foot { color: var(--v-faint); font-size: 12px; margin: 8px 0 0; }
</style>
