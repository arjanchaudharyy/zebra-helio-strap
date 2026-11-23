<script setup lang="ts">
/* Insights: your week as big-number cards, AI insights written from those numbers, and alerts. */
defineOptions({ name: 'Insights' });
import { computed, ref } from 'vue';
import AiInsight from '../components/AiInsight.vue';
import HealthWatch from '../components/HealthWatch.vue';
import { backend, bedtimeMinutes, byDate, clock, duration, fetchSeries, lastDays, mean, median, nights, shortDay, useReload, wakeDay } from '../data';
import type { MetricSeries, SleepSession, Workout } from '../../types';

const sleep = ref<SleepSession[]>([]);
const series = ref<MetricSeries[]>([]);
const workouts = ref<Workout[]>([]);
useReload(async () => {
  const [s, m, w] = await Promise.all([
    backend.getRecentSleep(60).catch(() => [] as SleepSession[]),
    fetchSeries(['steps', 'sleep_hrv', 'readiness', 'stress', 'resting_hr', 'device_resting_hr'], 21).catch(() => [] as MetricSeries[]),
    backend.getWorkoutPage(60, 0).then((p) => p.items).catch(() => [] as Workout[]),
  ]);
  sleep.value = s; series.value = m; workouts.value = w;
});

const days = lastDays(14);
const thisWeek = days.slice(7);
const lastWeek = days.slice(0, 7);
const ns = computed(() => nights(sleep.value));
const nightsIn = (keys: string[]) => ns.value.filter((n) => keys.includes(wakeDay(n)));
const avg = (keys: string[], metric: string) => {
  const m = byDate(series.value.find((s) => s.metric === metric));
  return mean(keys.map((k) => m.get(k)).filter((v): v is number => v != null && v > 0));
};
const fmtClock = (minsAfter6pm: number) => { const d = new Date(); const t = (minsAfter6pm + 18 * 60) % 1440; d.setHours(Math.floor(t / 60), Math.round(t % 60), 0, 0); return clock(d.toISOString()); };
const change = (a: number | null, b: number | null, unit: string, digits = 0) => {
  if (a == null || b == null) return 'No data for last week to compare.';
  const d = a - b;
  if (Math.abs(d) < 10 ** -digits) return 'Same as last week.';
  return `${d > 0 ? 'Up' : 'Down'} ${Math.abs(d).toFixed(digits)}${unit} from last week.`;
};

interface Card { key: string; label: string; value: string; unit?: string; note: string; color: string }
const cards = computed<Card[]>(() => {
  const out: Card[] = [];
  const tw = nightsIn(thisWeek); const lw = nightsIn(lastWeek);
  const sleepTw = mean(tw.map((n) => n.duration_minutes)); const sleepLw = mean(lw.map((n) => n.duration_minutes));
  if (sleepTw != null) out.push({ key: 'sleep', label: 'Average sleep', value: duration(sleepTw), note: sleepLw == null ? `${tw.length} nights tracked.` : `${sleepTw >= sleepLw ? 'More' : 'Less'} than last week by ${duration(Math.abs(sleepTw - sleepLw)) || '0 min'}.`, color: 'var(--v-sleep)' });
  const best = [...tw].sort((a, b) => (b.score ?? 0) - (a.score ?? 0))[0];
  if (best) out.push({ key: 'best', label: 'Best night', value: String(best.score ?? '–'), unit: 'score', note: `${shortDay(wakeDay(best))}: ${duration(best.duration_minutes)}, in bed at ${clock(best.start_time)}.`, color: 'var(--v-sleep)' });
  const beds = tw.map(bedtimeMinutes);
  if (beds.length >= 3) {
    const spread = (Math.max(...beds) - Math.min(...beds)) / 60;
    out.push({ key: 'bed', label: 'Usual bedtime', value: fmtClock(median(beds)!), note: `It moved by ${spread.toFixed(1)} hours this week. ${spread > 2 ? 'A steadier bedtime is your easiest win.' : 'Nice and steady.'}`, color: 'var(--v-teal)' });
  }
  const st = byDate(series.value.find((s) => s.metric === 'steps'));
  const stepsTw = thisWeek.map((k) => st.get(k) ?? 0); const stepsLw = lastWeek.map((k) => st.get(k) ?? 0);
  const totalTw = stepsTw.reduce((a, b) => a + b, 0); const totalLw = stepsLw.reduce((a, b) => a + b, 0);
  if (totalTw > 0) {
    const bestDay = thisWeek[stepsTw.indexOf(Math.max(...stepsTw))];
    out.push({ key: 'steps', label: 'Steps this week', value: totalTw.toLocaleString('en-GB'), note: `Best day ${shortDay(bestDay)} with ${Math.max(...stepsTw).toLocaleString('en-GB')}. ${totalLw ? change(totalTw, totalLw, ' steps') : ''}`, color: 'var(--v-move)' });
  }
  const hrvTw = avg(thisWeek, 'sleep_hrv'); const hrvLw = avg(lastWeek, 'sleep_hrv');
  if (hrvTw != null) out.push({ key: 'hrv', label: 'Night HRV', value: Math.round(hrvTw).toString(), unit: 'ms', note: change(hrvTw, hrvLw, ' ms'), color: 'var(--v-good)' });
  const rTw = avg(thisWeek, 'readiness'); const rLw = avg(lastWeek, 'readiness');
  if (rTw != null) out.push({ key: 'ready', label: 'Average readiness', value: Math.round(rTw).toString(), note: change(rTw, rLw, ''), color: 'var(--v-good)' });
  const sTw = avg(thisWeek, 'stress'); const sLw = avg(lastWeek, 'stress');
  if (sTw != null) out.push({ key: 'stress', label: 'Average stress', value: Math.round(sTw).toString(), note: `${sTw < 40 ? 'Mostly relaxed.' : sTw < 60 ? 'Moderate.' : 'High. Plan some downtime.'} ${change(sTw, sLw, '')}`, color: 'var(--v-move)' });
  const wk = workouts.value.filter((w) => thisWeek.includes(w.start_time.slice(0, 10)));
  out.push({ key: 'workouts', label: 'Workouts', value: String(wk.length), note: wk.length ? `${duration(wk.reduce((s, w) => s + (new Date(w.end_time).getTime() - new Date(w.start_time).getTime()) / 60000, 0))} in total.` : 'No workouts this week yet.', color: 'var(--v-heart)' });
  return out;
});
const weekLabel = `${shortDay(thisWeek[0])} to ${shortDay(thisWeek[6])}`;
</script>

<template>
  <div class="v4-page">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Insights</h1>
        <p class="v4-sub">What happened, what it means, and what to do. Written from your own data.</p>
      </div>
    </header>

    <section class="block">
      <div class="sec-head"><h2 class="sec-title">Your week</h2><span class="sec-note">{{ weekLabel }}</span></div>
      <div class="cards">
        <article v-for="c in cards" :key="c.key" class="wcard" :style="{ '--c': c.color }">
          <div class="w-label">{{ c.label }}</div>
          <div class="w-value"><span class="v4-num">{{ c.value }}</span><span v-if="c.unit" class="w-unit">{{ c.unit }}</span></div>
          <p class="w-note">{{ c.note }}</p>
        </article>
      </div>
    </section>

    <section class="block">
      <div class="sec-head"><h2 class="sec-title">For you</h2></div>
      <div class="v4-grid-2">
        <AiInsight topic="today" title="Today" />
        <AiInsight topic="week" title="This week's plan" />
      </div>
    </section>

    <section class="block">
      <div class="sec-head"><h2 class="sec-title">Alerts</h2></div>
      <HealthWatch />
    </section>
  </div>
</template>

<style scoped>
.block { margin-bottom: 28px; }
.sec-head { display: flex; align-items: baseline; gap: 12px; margin: 0 0 12px; }
.sec-title { font-size: 22px; font-weight: 700; letter-spacing: -0.02em; margin: 0; }
.sec-note { color: var(--v-muted); font-size: 13px; }
.cards { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(220px, 1fr); gap: 12px; overflow-x: auto; padding-bottom: 6px; scroll-snap-type: x mandatory; }
.wcard { scroll-snap-align: start; background: var(--v-surface); border-radius: 18px; padding: 18px 18px 20px; min-height: 170px; display: grid; align-content: start; gap: 8px; }
.w-label { font-size: 13px; font-weight: 600; color: var(--c); }
.w-value { display: flex; align-items: baseline; gap: 6px; }
.w-value .v4-num { font-size: 40px; letter-spacing: -0.035em; }
.w-unit { color: var(--v-muted); font-size: 13px; font-weight: 600; }
.w-note { margin: 0; color: var(--v-muted); font-size: 13px; line-height: 1.45; }
</style>
