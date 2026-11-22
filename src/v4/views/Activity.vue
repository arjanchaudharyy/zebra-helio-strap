<script setup lang="ts">
defineOptions({ name: 'ActivityDetail' });
import { computed, ref } from 'vue';
import Bars from '../components/Bars.vue';
import { backend, byDate, fetchSeries, lastDays, mean, shortDay, useReload } from '../data';
import type { HealthOverview, MetricSeries } from '../../types';

const range = ref<30 | 90>(30);
const series = ref<MetricSeries[]>([]);
const overview = ref<HealthOverview | null>(null);
const { loading } = useReload(async () => {
  const [s, o] = await Promise.all([
    fetchSeries(['steps', 'distance', 'active_calories', 'pai_total', 'pai_daily'], 180),
    backend.getHealthOverview().catch(() => null),
  ]);
  series.value = s;
  overview.value = o;
});
const get = (name: string) => byDate(series.value.find((s) => s.metric === name));
const steps = computed(() => get('steps'));
const distance = computed(() => get('distance'));
const pai = computed(() => get('pai_total'));
const goal = computed(() => overview.value?.steps_goal ?? 10000);
const days = computed(() => lastDays(range.value));
const every = computed(() => (range.value === 30 ? 5 : 15));

const stepBars = computed(() => days.value.map((key, i) => ({ key, label: i % every.value === 0 ? shortDay(key) : '', value: steps.value.get(key) ?? null, tip: shortDay(key) })));
const paiBars = computed(() => days.value.map((key, i) => ({ key, label: i % every.value === 0 ? shortDay(key) : '', value: pai.value.get(key) ?? null, tip: shortDay(key) })));

/* Days under 300 steps are almost always "band not worn", so they do not pull the average down. */
const worn = computed(() => days.value.map((k) => steps.value.get(k)).filter((v): v is number => v != null && v >= 300));
const avgSteps = computed(() => mean(worn.value));
const goalDays = computed(() => worn.value.filter((v) => v >= goal.value).length);
const totalKm = computed(() => {
  const v = days.value.map((k) => distance.value.get(k)).filter((x): x is number => x != null);
  return v.length ? v.reduce((a, b) => a + b, 0) / 1000 : null;
});
const paiNow = computed(() => {
  const p = series.value.find((s) => s.metric === 'pai_total')?.points ?? [];
  return p.length ? p[p.length - 1].value : null;
});
const hasPai = computed(() => paiBars.value.some((b) => b.value != null && b.value > 0));
</script>

<template>
  <div class="v4-page">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Activity</h1>
        <p class="v4-sub">Steps and how hard your heart worked</p>
      </div>
      <div class="v4-tabs" role="group" aria-label="Range">
        <button type="button" :aria-pressed="range === 30" @click="range = 30">30 days</button>
        <button type="button" :aria-pressed="range === 90" @click="range = 90">90 days</button>
      </div>
    </header>

    <div v-if="!loading && !steps.size" class="v4-card v4-empty"><strong>No activity yet</strong>Sync your band to see steps here.</div>

    <div v-else class="v4-stack">
      <section class="v4-card">
        <div class="v4-stats">
          <div><div class="v4-stat-label">Average steps</div><div class="v4-stat-value">{{ avgSteps == null ? '–' : Math.round(avgSteps).toLocaleString('en-GB') }}</div></div>
          <div><div class="v4-stat-label">Days at goal</div><div class="v4-stat-value">{{ goalDays }} <span class="v4-unit">of {{ worn.length }}</span></div></div>
          <div v-if="totalKm != null"><div class="v4-stat-label">Distance</div><div class="v4-stat-value">{{ totalKm.toFixed(1) }} <span class="v4-unit">km</span></div></div>
        </div>
        <div class="chart">
          <Bars :bars="stepBars" color="var(--v-move)" :height="200" :reference="goal" :reference-label="`Goal ${goal.toLocaleString('en-GB')}`" />
        </div>
      </section>

      <section v-if="hasPai" class="v4-card">
        <div class="v4-card-head">
          <h2 class="v4-card-title">PAI</h2>
          <span class="v4-card-note">Now {{ paiNow == null ? '–' : Math.round(paiNow) }}</span>
        </div>
        <p class="hint">PAI scores how hard your heart worked over the last 7 days. Keeping it at 100 or more is linked to a healthier heart.</p>
        <div class="chart">
          <Bars :bars="paiBars" color="var(--v-move)" :height="160" :reference="100" reference-label="100" />
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.chart { margin-top: 28px; }
.hint { color: var(--v-muted); font-size: 13px; margin: 0; }
</style>
