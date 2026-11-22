<script setup lang="ts">
defineOptions({ name: 'SleepList' });
import { computed, ref } from 'vue';
import AiInsight from '../components/AiInsight.vue';
import { RouterLink } from 'vue-router';
import Bars from '../components/Bars.vue';
import StageBar from '../components/StageBar.vue';
import { backend, bedtimeMinutes, clock, duration, lastDays, mean, median, nights, quantile, shortDay, todayKey, useReload, wakeDay } from '../data';
import type { SleepSession } from '../../types';

const sessions = ref<SleepSession[]>([]);
const { loading } = useReload(async () => { sessions.value = await backend.getRecentSleep(500); });

const range = ref<30 | 90 | 3650>(30);
const all = computed(() => nights(sessions.value));
const byDay = computed(() => new Map(all.value.map((n) => [wakeDay(n), n])));
const firstDay = computed(() => { const n = all.value; return n.length ? wakeDay(n[n.length - 1]) : null; });
const days = computed(() => {
  if (range.value !== 3650 || !firstDay.value) return lastDays(range.value === 3650 ? 30 : range.value);
  const span = Math.round((new Date(todayKey()).getTime() - new Date(firstDay.value).getTime()) / 86400000) + 1;
  return lastDays(Math.max(30, span));
});
const inRange = computed(() => days.value.map((d) => byDay.value.get(d)).filter((n): n is SleepSession => Boolean(n)));

const bars = computed(() => days.value.map((key, i) => {
  const n = byDay.value.get(key);
  return { key, label: i % Math.max(1, Math.round(days.value.length / 7)) === 0 ? shortDay(key) : '', value: n ? n.duration_minutes / 60 : null, tip: n ? `${shortDay(key)}, score ${n.score ?? '–'}` : `${shortDay(key)}, no data` };
}));

const avgHours = computed(() => mean(inRange.value.map((n) => n.duration_minutes)));
const avgScore = computed(() => mean(inRange.value.map((n) => n.score).filter((s): s is number => s != null)));
const enough = computed(() => inRange.value.filter((n) => n.duration_minutes >= 420).length);
const bed = computed(() => {
  const v = inRange.value.map(bedtimeMinutes);
  const m = median(v);
  if (m == null) return null;
  const total = (m + 18 * 60) % (24 * 60);
  const d = new Date(); d.setHours(Math.floor(total / 60), Math.round(total % 60), 0, 0);
  const spread = v.length >= 5 ? ((quantile(v, 0.9)! - quantile(v, 0.1)!) / 60) : null;
  return { time: clock(d.toISOString()), spread };
});

/* Score by how long you slept, from your own nights. */
const buckets = computed(() => {
  const groups = [
    { label: 'Under 6 h', lo: 0, hi: 360 },
    { label: '6 to 7 h', lo: 360, hi: 420 },
    { label: '7 to 8 h', lo: 420, hi: 480 },
    { label: '8 h or more', lo: 480, hi: 1e9 },
  ];
  return groups.map((g) => {
    const scores = all.value.filter((n) => n.duration_minutes >= g.lo && n.duration_minutes < g.hi && n.score != null).map((n) => n.score!);
    return { ...g, count: scores.length, score: mean(scores) };
  }).filter((g) => g.count >= 3);
});
const bestBucket = computed(() => Math.max(...buckets.value.map((b) => b.score ?? 0), 1));
/* Only claim "more sleep, higher score" when the person's own nights show it. */
const rises = computed(() => buckets.value.every((b, i, a) => i === 0 || (b.score ?? 0) >= (a[i - 1].score ?? 0)));

/* Last night's contributors: each part of the night against a simple target. */
const last = computed(() => all.value[0] ?? null);
const contributors = computed(() => {
  const n = last.value;
  if (!n) return [];
  const total = n.duration_minutes;
  const pct = (m: number | null | undefined) => (m == null || !total ? null : (m / total) * 100);
  const deep = pct(n.deep_minutes);
  const rem = pct(n.rem_minutes);
  const usualBed = median(all.value.slice(1, 31).map(bedtimeMinutes));
  const bedDiff = usualBed == null ? null : Math.abs(bedtimeMinutes(n) - usualBed);
  const rate = (v: number | null, good: (x: number) => boolean, fair: (x: number) => boolean) => (v == null ? 'none' : good(v) ? 'good' : fair(v) ? 'fair' : 'poor');
  return [
    { label: 'Total sleep', value: duration(total), fill: Math.min(1, total / 480), rating: rate(total, (x) => x >= 450, (x) => x >= 390) },
    { label: 'Deep sleep', value: deep == null ? 'No data' : `${duration(n.deep_minutes)}, ${Math.round(deep)}%`, fill: deep == null ? 0 : Math.min(1, deep / 20), rating: rate(deep, (x) => x >= 15, (x) => x >= 10) },
    { label: 'REM sleep', value: rem == null ? 'No data' : `${duration(n.rem_minutes)}, ${Math.round(rem)}%`, fill: rem == null ? 0 : Math.min(1, rem / 22), rating: rate(rem, (x) => x >= 20, (x) => x >= 14) },
    { label: 'Time awake', value: n.awake_minutes == null ? 'No data' : duration(n.awake_minutes) || '0 min', fill: n.awake_minutes == null ? 0 : Math.max(0, 1 - n.awake_minutes / 60), rating: rate(n.awake_minutes ?? null, (x) => x <= 20, (x) => x <= 45) },
    { label: 'Timing', value: bedDiff == null ? 'No baseline' : `${duration(bedDiff) || '0 min'} from your usual bedtime`, fill: bedDiff == null ? 0 : Math.max(0, 1 - bedDiff / 180), rating: rate(bedDiff, (x) => x <= 45, (x) => x <= 90) },
  ];
});
const word = (r: string) => ({ good: 'Optimal', fair: 'Fair', poor: 'Pay attention', none: 'No data' } as Record<string, string>)[r];
</script>

<template>
  <div class="v4-page">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Sleep</h1>
        <p class="v4-sub">{{ all.length }} nights on this computer</p>
      </div>
      <div class="v4-tabs" role="group" aria-label="Range">
        <button type="button" :aria-pressed="range === 30" @click="range = 30">30 days</button>
        <button type="button" :aria-pressed="range === 90" @click="range = 90">90 days</button>
        <button type="button" :aria-pressed="range === 3650" @click="range = 3650">All time</button>
      </div>
    </header>

    <div v-if="!loading && !all.length" class="v4-card v4-empty"><strong>No sleep recorded yet</strong>Wear your band to bed and sync in the morning.</div>

    <div v-else class="v4-stack">
      <section class="v4-card">
        <div class="v4-stats">
          <div><div class="v4-stat-label">Average sleep</div><div class="v4-stat-value">{{ duration(avgHours) || '–' }}</div></div>
          <div><div class="v4-stat-label">Average score</div><div class="v4-stat-value">{{ avgScore == null ? '–' : Math.round(avgScore) }}</div></div>
          <div><div class="v4-stat-label">Nights of 7 h or more</div><div class="v4-stat-value">{{ enough }} <span class="v4-unit">of {{ inRange.length }}</span></div></div>
          <div v-if="bed"><div class="v4-stat-label">Usual bedtime</div><div class="v4-stat-value">{{ bed.time }}</div></div>
        </div>
        <div class="chart">
          <Bars :bars="bars" color="var(--v-sleep)" :height="180" :reference="8" reference-label="8 h" :format="(v) => duration(v * 60)" />
        </div>
      </section>

      <AiInsight topic="sleep" />

      <section v-if="last" class="v4-card">
        <div class="v4-card-head"><h2 class="v4-card-title">Last night's contributors</h2><span class="v4-card-note">{{ shortDay(wakeDay(last)) }}, score {{ last.score ?? '–' }}</span></div>
        <div class="contribs">
          <div v-for="c in contributors" :key="c.label" class="contrib">
            <div class="c-top"><span class="c-label">{{ c.label }}</span><span :class="['v4-pill', c.rating]">{{ word(c.rating) }}</span></div>
            <div class="c-track"><span :class="c.rating" :style="{ width: `${Math.max(4, c.fill * 100)}%` }" /></div>
            <div class="c-value">{{ c.value }}</div>
          </div>
        </div>
      </section>

      <div class="v4-grid-2">
        <section v-if="buckets.length >= 2" class="v4-card">
          <div class="v4-card-head"><h2 class="v4-card-title">Your score by hours slept</h2><span class="v4-card-note">All your nights</span></div>
          <div class="buckets">
            <div v-for="b in buckets" :key="b.label" class="bucket">
              <span class="b-label">{{ b.label }}</span>
              <span class="b-track"><span :style="{ width: `${((b.score ?? 0) / bestBucket) * 100}%` }" /></span>
              <span class="b-score v4-num">{{ Math.round(b.score ?? 0) }}</span>
            </div>
          </div>
          <p class="hint">{{ rises ? 'On your nights, more sleep gives a higher score.' : 'Average sleep score for each length of night.' }}</p>
        </section>

        <section v-if="bed && bed.spread != null" class="v4-card">
          <div class="v4-card-head"><h2 class="v4-card-title">Bedtime</h2><span class="v4-card-note">{{ range === 3650 ? 'All time' : `Last ${range} days` }}</span></div>
          <div class="big-line"><span class="v4-num big">{{ bed.time }}</span></div>
          <p class="hint">Your bedtime usually moves by about {{ Math.round(bed.spread) }} {{ Math.round(bed.spread) === 1 ? 'hour' : 'hours' }}. {{ bed.spread >= 2 ? 'Going to bed at the same time each night helps your score.' : 'That is steady.' }}</p>
        </section>
      </div>

      <section class="v4-card">
        <div class="v4-card-head"><h2 class="v4-card-title">Nights</h2></div>
        <div class="v4-list">
          <RouterLink v-for="n in all.slice(0, Math.min(range, 120))" :key="n.sleep_id" :to="`/sleep/${encodeURIComponent(n.sleep_id)}`" class="v4-row night">
            <div>
              <div class="n-day">{{ shortDay(wakeDay(n)) }}</div>
              <div class="n-time">{{ clock(n.start_time) }} to {{ clock(n.end_time) }}</div>
            </div>
            <div class="n-right">
              <div class="n-stages"><StageBar compact :deep="n.deep_minutes" :light="n.light_minutes" :rem="n.rem_minutes" :awake="n.awake_minutes" /></div>
              <span class="n-dur">{{ duration(n.duration_minutes) }}</span>
              <span class="n-score v4-num">{{ n.score ?? '–' }}</span>
            </div>
          </RouterLink>
        </div>
      </section>
    </div>
  </div>
</template>

<style scoped>
.contribs { display: grid; grid-template-columns: repeat(auto-fit, minmax(190px, 1fr)); gap: 22px 28px; }
.c-top { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.c-label { font-weight: 600; }
.c-track { height: 6px; border-radius: 3px; background: var(--v-surface-2); margin: 10px 0 8px; overflow: hidden; }
.c-track span { display: block; height: 100%; border-radius: 3px; }
.c-track span.good { background: var(--v-good); }
.c-track span.fair { background: var(--v-move); }
.c-track span.poor { background: var(--v-heart); }
.c-track span.none { background: var(--v-line-strong); }
.c-value { color: var(--v-muted); font-size: 13px; }
.chart { margin-top: 28px; }
.buckets { display: grid; gap: 12px; }
.bucket { display: grid; grid-template-columns: 96px 1fr 36px; align-items: center; gap: 12px; }
.b-label { color: var(--v-muted); font-size: 13px; }
.b-track { height: 10px; background: var(--v-surface-2); border-radius: 5px; overflow: hidden; }
.b-track span { display: block; height: 100%; background: var(--v-sleep); border-radius: 5px; }
.b-score { font-size: 22px; text-align: right; }
.hint { color: var(--v-muted); font-size: 13px; margin: 16px 0 0; }
.big-line { display: flex; align-items: baseline; }
.big { font-size: 44px; }
.night { grid-template-columns: 1fr auto; }
.n-day { font-weight: 600; }
.n-time { color: var(--v-muted); font-size: 13px; }
.n-right { display: flex; align-items: center; gap: 18px; }
.n-stages { width: 160px; }
.n-dur { color: var(--v-muted); font-size: 13px; width: 90px; text-align: right; }
.n-score { font-size: 24px; width: 34px; text-align: right; color: var(--v-sleep); }
@media (max-width: 760px) { .n-stages { display: none; } }
</style>
