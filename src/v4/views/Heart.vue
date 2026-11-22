<script setup lang="ts">
defineOptions({ name: 'HeartRateDetail' });
import { computed, ref } from 'vue';
import LineChart from '../components/LineChart.vue';
import { backend, clock, fetchSeries, median, quantile, restingSeries, shortDay, useReload } from '../data';
import type { HeartRatePoint, MetricSeries } from '../../types';

const hours = ref<24 | 168>(24);
const heart = ref<HeartRatePoint[]>([]);
const series = ref<MetricSeries[]>([]);
const { loading, reload } = useReload(async () => {
  const [h, s] = await Promise.all([
    backend.getHeartRateSeries(hours.value),
    fetchSeries(['resting_hr', 'device_resting_hr'], 180),
  ]);
  heart.value = h;
  series.value = s;
});
const setHours = (value: 24 | 168) => { hours.value = value; void reload(); };

const rest = computed(() => restingSeries(series.value));
const restPoints = computed(() => (rest.value?.points ?? []).map((p) => ({ t: new Date(`${p.date}T12:00:00`).getTime(), v: p.value })));
const restLatest = computed(() => { const p = rest.value?.points ?? []; return p.length ? p[p.length - 1] : null; });
const restUsual = computed(() => median((rest.value?.points ?? []).slice(-31, -1).map((p) => p.value)));
const restLow = computed(() => {
  const v = (rest.value?.points ?? []).map((p) => p.value);
  return v.length ? Math.min(...v) : null;
});
/* Compare the first and last month of the range, ignoring single-day outliers. */
const trend = computed(() => {
  const p = rest.value?.points ?? [];
  if (p.length < 40) return null;
  const first = median(p.slice(0, 30).map((x) => x.value));
  const last = median(p.slice(-30).map((x) => x.value));
  if (first == null || last == null) return null;
  return Math.round(last - first);
});

const points = computed(() => heart.value.map((p) => ({ t: new Date(p.timestamp).getTime(), v: p.value })));
const stats = computed(() => {
  const v = heart.value.map((p) => p.value);
  if (!v.length) return null;
  return { lo: Math.min(...v), hi: Math.max(...v), typical: Math.round(median(v)!), p90: Math.round(quantile(v, 0.9)!) };
});
const xLabel = (t: number) => (hours.value === 24 ? clock(new Date(t).toISOString()) : shortDay(new Date(t).toISOString().slice(0, 10)));
const dayLabel = (t: number) => shortDay(new Date(t).toISOString().slice(0, 10));
</script>

<template>
  <div class="v4-page">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Heart</h1>
        <p class="v4-sub">Heart rate from your band, compared only with you</p>
      </div>
    </header>

    <div class="v4-stack">
      <section class="v4-card">
        <div class="v4-card-head">
          <h2 class="v4-card-title">Heart rate</h2>
          <div class="v4-tabs" role="group" aria-label="Range">
            <button type="button" :aria-pressed="hours === 24" @click="setHours(24)">24 hours</button>
            <button type="button" :aria-pressed="hours === 168" @click="setHours(168)">7 days</button>
          </div>
        </div>
        <div v-if="stats" class="v4-stats stats">
          <div><div class="v4-stat-label">Lowest</div><div class="v4-stat-value">{{ stats.lo }} <span class="v4-unit">bpm</span></div></div>
          <div><div class="v4-stat-label">Typical</div><div class="v4-stat-value">{{ stats.typical }} <span class="v4-unit">bpm</span></div></div>
          <div><div class="v4-stat-label">Highest</div><div class="v4-stat-value">{{ stats.hi }} <span class="v4-unit">bpm</span></div></div>
        </div>
        <LineChart v-if="points.length > 1" :points="points" color="var(--v-heart)" :height="220" :reference="restLatest?.value ?? null" reference-label="Resting" :x-label="xLabel" unit="bpm" />
        <p v-else-if="!loading" class="v4-empty">No heart rate readings in this range.</p>
      </section>

      <section v-if="restPoints.length > 1" class="v4-card">
        <div class="v4-card-head">
          <h2 class="v4-card-title">Resting heart rate, last 6 months</h2>
          <span class="v4-card-note">One value a day from your band</span>
        </div>
        <div class="v4-stats stats">
          <div><div class="v4-stat-label">Latest</div><div class="v4-stat-value">{{ restLatest ? Math.round(restLatest.value) : '–' }} <span class="v4-unit">bpm</span></div></div>
          <div><div class="v4-stat-label">30-day usual</div><div class="v4-stat-value">{{ restUsual == null ? '–' : Math.round(restUsual) }} <span class="v4-unit">bpm</span></div></div>
          <div><div class="v4-stat-label">Lowest</div><div class="v4-stat-value">{{ restLow ?? '–' }} <span class="v4-unit">bpm</span></div></div>
        </div>
        <LineChart :points="restPoints" color="var(--v-heart)" :height="200" :gap-ms="4 * 86400000" :x-label="dayLabel" unit="bpm" show-dots />
        <p v-if="trend != null && Math.abs(trend) >= 2" class="hint">
          Your resting heart rate is {{ Math.abs(trend) }} bpm {{ trend > 0 ? 'higher' : 'lower' }} than at the start of this period.
          {{ trend > 0 ? 'More sleep and regular movement usually bring it down.' : 'A lower resting heart rate usually means better fitness or recovery.' }}
        </p>
      </section>
    </div>
  </div>
</template>

<style scoped>
.stats { margin-bottom: 24px; }
.hint { color: var(--v-muted); font-size: 13px; margin: 8px 0 0; }
</style>
