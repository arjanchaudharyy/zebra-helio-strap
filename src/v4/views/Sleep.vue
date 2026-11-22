<script setup lang="ts">
defineOptions({ name: 'SleepList' });
import { computed, ref } from 'vue';
import { RouterLink } from 'vue-router';
import Bars from '../components/Bars.vue';
import StageBar from '../components/StageBar.vue';
import { backend, bedtimeMinutes, clock, duration, lastDays, mean, median, nights, quantile, shortDay, useReload, wakeDay } from '../data';
import type { SleepSession } from '../../types';

const sessions = ref<SleepSession[]>([]);
const { loading } = useReload(async () => { sessions.value = await backend.getRecentSleep(400); });

const range = ref<30 | 90>(30);
const all = computed(() => nights(sessions.value));
const byDay = computed(() => new Map(all.value.map((n) => [wakeDay(n), n])));
const days = computed(() => lastDays(range.value));
const inRange = computed(() => days.value.map((d) => byDay.value.get(d)).filter((n): n is SleepSession => Boolean(n)));

const bars = computed(() => days.value.map((key, i) => {
  const n = byDay.value.get(key);
  return { key, label: i % (range.value === 30 ? 5 : 15) === 0 ? shortDay(key) : '', value: n ? n.duration_minutes / 60 : null, tip: n ? `${shortDay(key)}, score ${n.score ?? '–'}` : `${shortDay(key)}, no data` };
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
          <p class="hint">More sleep gives you a higher score. This is from your own data.</p>
        </section>

        <section v-if="bed && bed.spread != null" class="v4-card">
          <div class="v4-card-head"><h2 class="v4-card-title">Bedtime</h2><span class="v4-card-note">Last {{ range }} days</span></div>
          <div class="big-line"><span class="v4-num big">{{ bed.time }}</span></div>
          <p class="hint">Your bedtime usually moves by about {{ Math.round(bed.spread) }} {{ Math.round(bed.spread) === 1 ? 'hour' : 'hours' }}. {{ bed.spread >= 2 ? 'Going to bed at the same time each night helps your score.' : 'That is steady.' }}</p>
        </section>
      </div>

      <section class="v4-card">
        <div class="v4-card-head"><h2 class="v4-card-title">Nights</h2></div>
        <div class="v4-list">
          <RouterLink v-for="n in all.slice(0, range)" :key="n.sleep_id" :to="`/sleep/${encodeURIComponent(n.sleep_id)}`" class="v4-row night">
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
