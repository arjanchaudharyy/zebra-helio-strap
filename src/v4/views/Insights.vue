<script setup lang="ts">
/* Insights: your week as big-number cards, AI insights written from those numbers, and alerts. */
defineOptions({ name: 'Insights' });
import { computed, ref } from 'vue';
import AiInsight from '../components/AiInsight.vue';
import HealthWatch from '../components/HealthWatch.vue';
import Symbol from '../components/Symbol.vue';
import { backend, byDate, duration, fetchSeries, lastDays, mean, nights, shortDay, useReload, wakeDay } from '../data';
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

/* Apple Health style highlights: a plain sentence, then this week vs last week as two bars. */
interface Card { key: string; label: string; icon: string; color: string; headline: string; now: number | null; before: number | null; nowText: string; beforeText: string; extra?: string }
const sentence = (what: string, now: number | null, before: number | null, more: string, less: string, fmt: (x: number) => string, small: number) => {
  if (now == null) return `No ${what} data this week.`;
  if (before == null) return `${what[0].toUpperCase()}${what.slice(1)}: ${fmt(now)} this week.`;
  const d = now - before;
  if (Math.abs(d) < small) return `Your ${what} stayed about the same as last week.`;
  return `You ${d > 0 ? more : less} ${fmt(Math.abs(d))} ${d > 0 ? 'more' : 'less'} than last week.`.replace(' more more', ' more').replace(' less less', ' less');
};
const cards = computed<Card[]>(() => {
  const out: Card[] = [];
  const tw = nightsIn(thisWeek); const lw = nightsIn(lastWeek);
  const sTw = mean(tw.map((n) => n.duration_minutes)); const sLw = mean(lw.map((n) => n.duration_minutes));
  out.push({ key: 'sleep', label: 'Sleep', icon: 'sleep', color: 'var(--v-sleep)', now: sTw, before: sLw, nowText: sTw == null ? '–' : duration(sTw), beforeText: sLw == null ? '–' : duration(sLw),
    headline: sTw == null ? 'No sleep tracked this week.' : sLw == null ? `You slept ${duration(sTw)} a night on average.` : Math.abs(sTw - sLw) < 15 ? 'You slept about the same as last week.' : `You slept ${duration(Math.abs(sTw - sLw))} ${sTw > sLw ? 'more' : 'less'} a night than last week.` });
  const st = byDate(series.value.find((x) => x.metric === 'steps'));
  const stTw = mean(thisWeek.map((k) => st.get(k)).filter((v): v is number => v != null && v > 300));
  const stLw = mean(lastWeek.map((k) => st.get(k)).filter((v): v is number => v != null && v > 300));
  out.push({ key: 'steps', label: 'Steps', icon: 'activity', color: 'var(--v-move)', now: stTw, before: stLw,
    nowText: stTw == null ? '–' : Math.round(stTw).toLocaleString('en-GB'), beforeText: stLw == null ? '–' : Math.round(stLw).toLocaleString('en-GB'),
    headline: stTw == null ? 'No steps tracked this week.' : stLw == null ? `You walked ${Math.round(stTw).toLocaleString('en-GB')} steps a day.` : Math.abs(stTw - stLw) < 300 ? 'You walked about as much as last week.' : `You walked ${Math.round(Math.abs(stTw - stLw)).toLocaleString('en-GB')} ${stTw > stLw ? 'more' : 'fewer'} steps a day than last week.` });
  const hTw = avg(thisWeek, 'sleep_hrv'); const hLw = avg(lastWeek, 'sleep_hrv');
  out.push({ key: 'hrv', label: 'Heart rate variability', icon: 'live', color: 'var(--v-good)', now: hTw, before: hLw, nowText: hTw == null ? '–' : `${Math.round(hTw)} ms`, beforeText: hLw == null ? '–' : `${Math.round(hLw)} ms`,
    headline: hTw == null ? 'No HRV data this week.' : hLw == null ? `Your night HRV averaged ${Math.round(hTw)} ms.` : Math.abs(hTw - hLw) < 2 ? 'Your recovery signal held steady.' : hTw > hLw ? 'Your body recovered better than last week.' : 'Your body recovered a little less than last week.' });
  const rTw = avg(thisWeek, 'readiness'); const rLw = avg(lastWeek, 'readiness');
  out.push({ key: 'ready', label: 'Readiness', icon: 'recovery', color: 'var(--v-ready)', now: rTw, before: rLw, nowText: rTw == null ? '–' : String(Math.round(rTw)), beforeText: rLw == null ? '–' : String(Math.round(rLw)),
    headline: rTw == null ? 'No readiness data this week.' : rTw >= 80 ? 'You were well recovered most days.' : rTw >= 60 ? 'You were fairly recovered this week.' : 'Your body needed more recovery this week.' });
  const xTw = avg(thisWeek, 'stress'); const xLw = avg(lastWeek, 'stress');
  out.push({ key: 'stress', label: 'Stress', icon: 'heart', color: 'var(--v-heart)', now: xTw, before: xLw, nowText: xTw == null ? '–' : String(Math.round(xTw)), beforeText: xLw == null ? '–' : String(Math.round(xLw)),
    headline: xTw == null ? 'No stress data this week.' : xTw < 40 ? 'You were mostly relaxed this week.' : xTw < 60 ? 'You had a normal amount of stress.' : 'This was a stressful week. Plan some rest.' });
  const wk = workouts.value.filter((w) => thisWeek.includes(w.start_time.slice(0, 10)));
  const wkL = workouts.value.filter((w) => lastWeek.includes(w.start_time.slice(0, 10)));
  const mins = (ws: Workout[]) => ws.reduce((s2, w) => s2 + (new Date(w.end_time).getTime() - new Date(w.start_time).getTime()) / 60000, 0);
  out.push({ key: 'workouts', label: 'Workouts', icon: 'workouts', color: 'var(--v-heart-2)', now: mins(wk), before: mins(wkL), nowText: duration(mins(wk)) || '0 min', beforeText: duration(mins(wkL)) || '0 min',
    headline: wk.length ? `You worked out ${wk.length} ${wk.length === 1 ? 'time' : 'times'}, ${duration(mins(wk))} in total.` : 'No workouts this week yet.' });
  void sentence;
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
        <article v-for="c in cards" :key="c.key" class="hcard">
          <div class="h-label" :style="{ color: c.color }"><Symbol :name="c.icon" :size="15" />{{ c.label }}</div>
          <p class="h-line">{{ c.headline }}</p>
          <div v-if="c.now != null" class="compare">
            <div class="cmp-row">
              <span class="cmp-name">This week</span>
              <span class="cmp-bar"><i :style="{ width: `${Math.max(6, (c.now / Math.max(c.now, c.before ?? 0, 1)) * 100)}%`, background: c.color }" /></span>
              <span class="cmp-val">{{ c.nowText }}</span>
            </div>
            <div v-if="c.before != null" class="cmp-row">
              <span class="cmp-name">Last week</span>
              <span class="cmp-bar"><i :style="{ width: `${Math.max(6, (c.before / Math.max(c.now, c.before, 1)) * 100)}%`, background: 'var(--v-faint)' }" /></span>
              <span class="cmp-val dim">{{ c.beforeText }}</span>
            </div>
          </div>
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
.cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(300px, 1fr)); gap: 12px; }
.hcard { background: var(--v-surface); border-radius: 18px; padding: 16px 18px 18px; display: grid; gap: 10px; align-content: start; }
.h-label { display: flex; align-items: center; gap: 6px; font-size: 13px; font-weight: 600; }
.h-line { margin: 0; font-size: 17px; font-weight: 600; letter-spacing: -0.015em; line-height: 1.3; }
.compare { display: grid; gap: 8px; margin-top: 4px; }
.cmp-row { display: grid; grid-template-columns: 72px 1fr auto; align-items: center; gap: 10px; }
.cmp-name { font-size: 12px; color: var(--v-muted); }
.cmp-bar { height: 8px; border-radius: 4px; background: var(--v-well); overflow: hidden; }
.cmp-bar i { display: block; height: 100%; border-radius: 4px; }
.cmp-val { font-size: 13px; font-weight: 700; font-variant-numeric: tabular-nums; min-width: 64px; text-align: right; }
.cmp-val.dim { color: var(--v-muted); font-weight: 600; }
</style>
