<script setup lang="ts">
/* Recovery: everything the Zepp app shows about how rested and recharged you are, in plain words. */
defineOptions({ name: 'Recovery' });
import { computed, ref } from 'vue';
import MetricTrend from '../components/MetricTrend.vue';
import AiInsight from '../components/AiInsight.vue';
import { fetchSeries, useReload } from '../data';
import type { MetricSeries } from '../../types';

const METRICS = ['readiness', 'physical_readiness', 'mental_readiness', 'hybrid_charge', 'physical_charge', 'mental_charge',
  'stress', 'sleep_hrv', 'hrv_baseline', 'sleep_rhr', 'rhr_baseline', 'respiratory_rate', 'spo2_night_score', 'spo2_odi', 'training_load'];
const series = ref<MetricSeries[]>([]);
const range = ref<90 | 400>(90);
const { loading } = useReload(async () => { series.value = await fetchSeries(METRICS, 400); });
const get = (m: string) => {
  const s = series.value.find((x) => x.metric === m);
  if (!s) return undefined;
  const cut = new Date(Date.now() - range.value * 86400000).toISOString().slice(0, 10);
  return { ...s, points: s.points.filter((p) => p.date >= cut) };
};
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

      <div class="v4-grid-2">
        <MetricTrend title="Readiness" meaning="How ready your body is for effort today, from sleep, HRV, resting heart rate and temperature." :series="get('readiness')" color="var(--v-ready)" :higher-is-better="true" />
        <MetricTrend title="BioCharge" meaning="Your energy level. It drains with effort and stress and refills with sleep and rest." :series="get('hybrid_charge')" color="var(--v-good)" :higher-is-better="true" />
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
