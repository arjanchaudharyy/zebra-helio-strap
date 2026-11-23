<script setup lang="ts">
/* Recovery: everything the Zepp app shows about how rested and recharged you are, in plain words. */
defineOptions({ name: 'Recovery' });
import { computed, ref } from 'vue';
import MetricTrend from '../components/MetricTrend.vue';
import AiInsight from '../components/AiInsight.vue';
import { fetchSeries, useReload } from '../data';
import type { MetricSeries } from '../../types';

const METRICS = ['readiness', 'physical_readiness', 'mental_readiness', 'hybrid_charge', 'physical_charge', 'mental_charge',
  'stress', 'sleep_hrv', 'hrv_baseline', 'sleep_rhr', 'rhr_baseline', 'respiratory_rate', 'spo2_night_score', 'spo2_odi', 'training_load',
  'stress_relaxed_pct', 'stress_normal_pct', 'stress_medium_pct', 'stress_high_pct'];
const series = ref<MetricSeries[]>([]);
const range = ref<90 | 400>(90);
const { loading } = useReload(async () => { series.value = await fetchSeries(METRICS, 400); });
const get = (m: string) => {
  const s = series.value.find((x) => x.metric === m);
  if (!s) return undefined;
  const cut = new Date(Date.now() - range.value * 86400000).toISOString().slice(0, 10);
  return { ...s, points: s.points.filter((p) => p.date >= cut) };
};
/* Where the day's stress readings fell, last 7 days that have the split. */
const BANDS = [
  { m: 'stress_relaxed_pct', name: 'Relaxed', c: '#30d158' }, { m: 'stress_normal_pct', name: 'Normal', c: '#64d2ff' },
  { m: 'stress_medium_pct', name: 'Medium', c: '#ff9f0a' }, { m: 'stress_high_pct', name: 'High', c: '#ff375f' },
];
const split = computed(() => {
  const by = new Map<string, number[]>();
  BANDS.forEach((b, i) => series.value.find((x) => x.metric === b.m)?.points.forEach((p) => {
    if (p.value == null) return;
    const row = by.get(p.date) ?? [0, 0, 0, 0]; row[i] = p.value; by.set(p.date, row);
  }));
  return [...by.entries()].filter(([, r]) => r.some((v) => v > 0)).sort(([a], [b]) => a.localeCompare(b)).slice(-7)
    .map(([date, r]) => ({ date, r, label: new Date(`${date}T12:00:00`).toLocaleDateString('en-GB', { weekday: 'short', day: 'numeric' }) }));
});
const calmShare = computed(() => {
  if (!split.value.length) return null;
  const avg = (i: number) => split.value.reduce((a, d) => a + d.r[i], 0) / split.value.length;
  return { calm: Math.round(avg(0) + avg(1)), high: Math.round(avg(3)) };
});
const has = computed(() => series.value.some((s) => s.days_with_data > 0));
</script>

<template>
  <div class="v4-page">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Recovery</h1>
        <p class="v4-sub">How rested and recharged your body is, compared only with you.</p>
      </div>
      <div class="v4-tabs" role="group" aria-label="Range">
        <button type="button" :aria-pressed="range === 90" @click="range = 90">3 months</button>
        <button type="button" :aria-pressed="range === 400" @click="range = 400">All</button>
      </div>
    </header>

    <div v-if="!loading && !has" class="v4-card v4-empty"><strong>No recovery data yet</strong>Connect Advanced data in Settings, then sync.</div>

    <div v-else class="v4-stack">
      <AiInsight topic="today" title="Your recovery, by Zebra AI" />

      <section v-if="split.length" class="v4-card split">
        <h2 class="v4-card-title">How stressed you were</h2>
        <p v-if="calmShare" class="lead">This week you were relaxed or normal {{ calmShare.calm }}% of the time<span v-if="calmShare.high">, and highly stressed {{ calmShare.high }}%</span>.</p>
        <div class="rows">
          <div v-for="d in split" :key="d.date" class="srow">
            <span class="d">{{ d.label }}</span>
            <div class="bar"><i v-for="(b, i) in BANDS" :key="b.m" :style="{ width: d.r[i] + '%', background: b.c }" :title="`${b.name} ${d.r[i]}%`" /></div>
          </div>
        </div>
        <div class="legend"><span v-for="b in BANDS" :key="b.m"><i :style="{ background: b.c }" />{{ b.name }}</span></div>
      </section>

      <div class="v4-grid-2">
        <MetricTrend title="Readiness" meaning="How ready your body is for effort today, from sleep, HRV, resting heart rate and temperature." :series="get('readiness')" color="var(--v-ready)" :higher-is-better="true" />
        <MetricTrend title="BioCharge" meaning="Your energy level. It drains with effort and stress and refills with sleep and rest." :series="get('hybrid_charge')" color="var(--v-good)" :higher-is-better="true" />
        <MetricTrend title="Body readiness" meaning="The physical side of readiness: how recovered your muscles and heart are." :series="get('physical_readiness')" color="var(--v-ready)" :higher-is-better="true" />
        <MetricTrend title="Mind readiness" meaning="The mental side of readiness, mostly from stress and sleep." :series="get('mental_readiness')" color="var(--v-sleep-2)" :higher-is-better="true" />
        <MetricTrend title="Body charge" meaning="The physical side of your energy." :series="get('physical_charge')" color="var(--v-teal)" :higher-is-better="true" />
        <MetricTrend title="Mind charge" meaning="The mental side of your energy." :series="get('mental_charge')" color="var(--v-sleep-2)" :higher-is-better="true" />
        <MetricTrend title="HRV during sleep" meaning="Higher usually means better recovery. It drops with stress, alcohol, late meals and illness." :series="get('sleep_hrv')" :baseline="get('hrv_baseline')" unit="ms" color="var(--v-good)" :higher-is-better="true" />
        <MetricTrend title="Resting heart rate during sleep" meaning="Lower usually means better recovery and fitness." :series="get('sleep_rhr')" :baseline="get('rhr_baseline')" unit="bpm" color="var(--v-heart)" :higher-is-better="false" />
        <MetricTrend title="Stress" meaning="Daily average, 0 to 100. Under 40 is relaxed." :series="get('stress')" color="var(--v-move)" :higher-is-better="false" />
        <MetricTrend title="Breathing during sleep" meaning="Breaths a minute. A sudden rise can be an early sign of illness." :series="get('respiratory_rate')" unit="/min" color="var(--v-teal)" :digits="1" :higher-is-better="null" />
        <MetricTrend title="Night oxygen score" meaning="How steady your blood oxygen stayed overnight. Higher is better." :series="get('spo2_night_score')" color="var(--v-teal)" :higher-is-better="true" />
        <MetricTrend title="Breathing dips at night" meaning="Oxygen dips per hour of sleep. Under 5 is normal." :series="get('spo2_odi')" unit="/h" color="var(--v-heart)" :digits="1" :higher-is-better="false" />
        <MetricTrend title="Training load" meaning="How much your workouts have stressed your body lately." :series="get('training_load')" color="var(--v-move)" :higher-is-better="null" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.split .lead { margin: 4px 0 14px; color: var(--v-muted); font-size: 13.5px; }
.rows { display: grid; gap: 8px; }
.srow { display: grid; grid-template-columns: 52px 1fr; align-items: center; gap: 10px; }
.d { color: var(--v-muted); font-size: 12px; }
.bar { display: flex; height: 12px; border-radius: 6px; overflow: hidden; background: var(--v-line); }
.bar i { display: block; height: 100%; }
.legend { display: flex; flex-wrap: wrap; gap: 14px; margin-top: 12px; color: var(--v-muted); font-size: 12px; }
.legend i { display: inline-block; width: 8px; height: 8px; border-radius: 4px; margin-right: 6px; }
</style>
