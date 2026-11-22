<script setup lang="ts">
defineOptions({ name: 'Today' });
import { computed, ref } from 'vue';
import { RouterLink } from 'vue-router';
import Dial from '../components/Dial.vue';
import Bars from '../components/Bars.vue';
import LineChart from '../components/LineChart.vue';
import StageBar from '../components/StageBar.vue';
import {
  backend, bedtimeMinutes, byDate, clock, duration, fetchSeries, hoursMinutes, lastDays, longDay, mean, median,
  METRICS_ALL, nights, quantile, restingSeries, shortDay, todayKey, useReload, wakeDay, weekday,
} from '../data';
import type { HealthOverview, HeartRatePoint, MetricSeries, SleepSession } from '../../types';

const overview = ref<HealthOverview | null>(null);
const sleep = ref<SleepSession[]>([]);
const heart = ref<HeartRatePoint[]>([]);
const series = ref<MetricSeries[]>([]);

const { loading } = useReload(async () => {
  const [o, s, h, m] = await Promise.all([
    backend.getHealthOverview().catch(() => null),
    backend.getRecentSleep(60).catch(() => [] as SleepSession[]),
    backend.getHeartRateSeries(24).catch(() => [] as HeartRatePoint[]),
    fetchSeries(METRICS_ALL, 90).catch(() => [] as MetricSeries[]),
  ]);
  overview.value = o;
  sleep.value = s;
  heart.value = h;
  series.value = m;
});

const today = longDay(new Date());
const get = (name: string) => series.value.find((s) => s.metric === name);

/* Sleep */
const allNights = computed(() => nights(sleep.value));
const lastNight = computed(() => allNights.value[0] ?? null);
const lastNightIsRecent = computed(() => {
  const n = lastNight.value;
  return Boolean(n) && (Date.now() - new Date(n!.end_time).getTime()) < 36 * 3600 * 1000;
});
const usualSleep = computed(() => median(allNights.value.slice(1, 31).map((n) => n.duration_minutes)));
const sleepHM = computed(() => hoursMinutes(lastNight.value?.duration_minutes));

/* Resting heart rate */
const rest = computed(() => restingSeries(series.value));
const restPoints = computed(() => rest.value?.points ?? []);
const restLatest = computed(() => {
  const p = restPoints.value[restPoints.value.length - 1];
  if (!p) return null;
  const age = (new Date(todayKey()).getTime() - new Date(p.date).getTime()) / 86400000;
  return age <= 3 ? p : null;
});
const restBand = computed<[number, number] | null>(() => {
  const prior = restPoints.value.slice(-31, -1).map((p) => p.value);
  if (prior.length < 7) return null;
  return [Math.round(quantile(prior, 0.1)!), Math.round(quantile(prior, 0.9)!)];
});
const restUsual = computed(() => median(restPoints.value.slice(-31, -1).map((p) => p.value)));

/* Steps */
const stepsMap = computed(() => byDate(get('steps')));
const stepsToday = computed(() => overview.value?.steps_today ?? stepsMap.value.get(todayKey()) ?? null);
const stepsGoal = computed(() => overview.value?.steps_goal ?? 10000);
const days14 = lastDays(14);
const stepBars = computed(() => days14.map((key) => ({
  key, label: weekday(key).slice(0, 2), value: stepsMap.value.get(key) ?? null, tip: shortDay(key),
})));
const steps7 = computed(() => mean(lastDays(7).map((k) => stepsMap.value.get(k)).filter((v): v is number => v != null && v > 300)));

/* Sleep 14 nights */
const sleepByDay = computed(() => new Map(allNights.value.map((n) => [wakeDay(n), n])));
const sleepBars = computed(() => days14.map((key) => {
  const night = sleepByDay.value.get(key);
  return { key, label: weekday(key).slice(0, 2), value: night ? night.duration_minutes / 60 : null, tip: night ? `${shortDay(key)}, score ${night.score ?? '–'}` : shortDay(key) };
}));

/* Heart rate today */
const heartPoints = computed(() => heart.value.map((p) => ({ t: new Date(p.timestamp).getTime(), v: p.value })).filter((p) => Number.isFinite(p.t)));
const heartLatest = computed(() => heart.value[heart.value.length - 1] ?? null);
const heartRange = computed(() => {
  const v = heart.value.map((p) => p.value);
  return v.length ? { lo: Math.min(...v), hi: Math.max(...v) } : null;
});
const hourLabel = (t: number) => clock(new Date(t).toISOString()).replace(':00', '');

/* Plain-language notes. Each one compares only with the person's own history. */
const notes = computed(() => {
  const out: { tone: 'sleep' | 'heart' | 'move'; text: string }[] = [];
  const n = lastNight.value;
  if (n && lastNightIsRecent.value && usualSleep.value) {
    const diff = n.duration_minutes - usualSleep.value;
    if (Math.abs(diff) >= 20) out.push({ tone: 'sleep', text: `You slept ${duration(Math.abs(diff))} ${diff > 0 ? 'more' : 'less'} than your usual ${duration(usualSleep.value)}.` });
    else out.push({ tone: 'sleep', text: `You slept about your usual amount, ${duration(usualSleep.value)}.` });
  }
  const recentBeds = allNights.value.slice(0, 14).map(bedtimeMinutes);
  if (recentBeds.length >= 7) {
    const spread = (quantile(recentBeds, 0.9)! - quantile(recentBeds, 0.1)!) / 60;
    if (spread >= 2) out.push({ tone: 'sleep', text: `Your bedtime moved by about ${Math.round(spread)} hours over the last two weeks. A steady bedtime usually raises your sleep score.` });
  }
  if (restLatest.value && restUsual.value) {
    const d = Math.round(restLatest.value.value - restUsual.value);
    if (Math.abs(d) >= 3) out.push({ tone: 'heart', text: `Resting heart rate is ${Math.abs(d)} bpm ${d > 0 ? 'above' : 'below'} your 30-day usual of ${Math.round(restUsual.value)} bpm.${d > 0 ? ' Short sleep, stress or illness can raise it.' : ''}` });
    else out.push({ tone: 'heart', text: `Resting heart rate is normal for you, about ${Math.round(restUsual.value)} bpm.` });
  }
  if (steps7.value != null) {
    out.push({ tone: 'move', text: `You averaged ${Math.round(steps7.value).toLocaleString('en-GB')} steps a day this week.` });
  }
  return out.slice(0, 4);
});

const nothing = computed(() => !loading.value && !allNights.value.length && !heart.value.length && !stepsMap.value.size);
</script>

<template>
  <div class="v4-page today">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Today</h1>
        <p class="v4-sub">{{ today }}</p>
      </div>
    </header>

    <div v-if="nothing" class="v4-card">
      <div class="v4-empty">
        <strong>No data on this computer yet</strong>
        Connect your Zepp account in Settings, then sync.
      </div>
      <RouterLink to="/settings/account" class="v4-btn primary" style="margin-top: 14px">Open Settings</RouterLink>
    </div>

    <template v-else>
      <section class="dials v4-card" aria-label="Today at a glance">
        <Dial
          to="/sleep"
          label="Sleep score"
          :value="lastNightIsRecent ? (lastNight?.score ?? null) : null"
          :min="0" :max="100"
          color="var(--v-sleep)"
          :display="String(lastNight?.score ?? '')"
          :caption="lastNightIsRecent && sleepHM ? `${sleepHM.h} h ${sleepHM.m} min asleep` : 'No sleep recorded last night'"
        />
        <Dial
          to="/heart"
          label="Resting heart rate"
          :value="restLatest?.value ?? null"
          :min="40" :max="100"
          color="var(--v-heart)"
          :display="String(Math.round(restLatest?.value ?? 0))"
          unit="bpm"
          marker
          :band="restBand"
          :caption="restBand ? `Usual ${restBand[0]}–${restBand[1]} bpm` : ''"
        />
        <Dial
          to="/activity"
          label="Steps"
          :value="stepsToday"
          :min="0" :max="stepsGoal"
          color="var(--v-move)"
          :display="(stepsToday ?? 0).toLocaleString('en-GB')"
          :caption="`Goal ${stepsGoal.toLocaleString('en-GB')}`"
        />
      </section>

      <section v-if="notes.length" class="notes" aria-label="What stands out">
        <p v-for="(note, i) in notes" :key="i" :class="['note', note.tone]">{{ note.text }}</p>
      </section>

      <div class="v4-grid-2">
        <RouterLink v-if="lastNight" :to="`/sleep/${encodeURIComponent(lastNight.sleep_id)}`" class="v4-card">
          <div class="v4-card-head">
            <h2 class="v4-card-title">{{ lastNightIsRecent ? 'Last night' : `Last recorded night, ${shortDay(wakeDay(lastNight))}` }}</h2>
            <span class="v4-card-note">{{ clock(lastNight.start_time) }} to {{ clock(lastNight.end_time) }}</span>
          </div>
          <div class="big-line">
            <span class="v4-num big">{{ hoursMinutes(lastNight.duration_minutes)?.h }}</span><span class="v4-unit">h</span>
            <span class="v4-num big" style="margin-left: 10px">{{ hoursMinutes(lastNight.duration_minutes)?.m }}</span><span class="v4-unit">min</span>
          </div>
          <StageBar :deep="lastNight.deep_minutes" :light="lastNight.light_minutes" :rem="lastNight.rem_minutes" :awake="lastNight.awake_minutes" />
        </RouterLink>

        <RouterLink v-if="heartPoints.length > 1" to="/heart" class="v4-card">
          <div class="v4-card-head">
            <h2 class="v4-card-title">Heart rate, last 24 hours</h2>
            <span v-if="heartRange" class="v4-card-note">{{ heartRange.lo }}–{{ heartRange.hi }} bpm</span>
          </div>
          <div class="big-line">
            <span class="v4-num big">{{ heartLatest?.value }}</span><span class="v4-unit">bpm at {{ clock(heartLatest?.timestamp) }}</span>
          </div>
          <LineChart :points="heartPoints" color="var(--v-heart)" :height="150" :reference="restLatest?.value ?? null" reference-label="Resting" :x-label="hourLabel" unit="bpm" />
        </RouterLink>

        <RouterLink v-if="sleepBars.some((b) => b.value != null)" to="/sleep" class="v4-card">
          <div class="v4-card-head">
            <h2 class="v4-card-title">Sleep, last 14 nights</h2>
            <span v-if="usualSleep" class="v4-card-note">Usual {{ duration(usualSleep) }}</span>
          </div>
          <Bars :bars="sleepBars" color="var(--v-sleep)" :height="150" :reference="8" reference-label="8 h" :format="(v) => duration(v * 60)" />
        </RouterLink>

        <RouterLink v-if="stepBars.some((b) => b.value != null)" to="/activity" class="v4-card">
          <div class="v4-card-head">
            <h2 class="v4-card-title">Steps, last 14 days</h2>
            <span v-if="steps7 != null" class="v4-card-note">7-day average {{ Math.round(steps7).toLocaleString('en-GB') }}</span>
          </div>
          <Bars :bars="stepBars" color="var(--v-move)" :height="150" :reference="stepsGoal" :reference-label="`Goal ${stepsGoal.toLocaleString('en-GB')}`" />
        </RouterLink>
      </div>
    </template>
  </div>
</template>

<style scoped>
.dials { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 24px; padding: 32px 24px 24px; margin-bottom: 16px; }
@media (max-width: 760px) { .dials { grid-template-columns: minmax(0, 1fr); } }
.notes { display: grid; gap: 2px; margin: 0 0 16px; border-radius: var(--v-radius-lg); overflow: hidden; border: 1px solid var(--v-line); }
.note { margin: 0; padding: 14px 20px 14px 40px; background: var(--v-surface); font-size: 15px; position: relative; }
.note::before { content: ''; position: absolute; left: 20px; top: 21px; width: 8px; height: 8px; border-radius: 50%; }
.note.sleep::before { background: var(--v-sleep); }
.note.heart::before { background: var(--v-heart); }
.note.move::before { background: var(--v-move); }
.big-line { display: flex; align-items: baseline; margin-bottom: 18px; }
.big { font-size: 44px; }
</style>
