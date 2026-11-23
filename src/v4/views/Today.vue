<script setup lang="ts">
defineOptions({ name: 'Today' });
import { computed, ref } from 'vue';
import { RouterLink } from 'vue-router';
import Dial from '../components/Dial.vue';
import Bars from '../components/Bars.vue';
import LineChart from '../components/LineChart.vue';
import StageBar from '../components/StageBar.vue';
import Vital from '../components/Vital.vue';
import AiInsight from '../components/AiInsight.vue';
import {
  backend, byDate, clock, duration, fetchSeries, hoursMinutes, lastDays, longDay, mean, median,
  METRICS_ALL, nights, quantile, restingSeries, shortDay, todayKey, useReload, wakeDay, weekday,
} from '../data';
import { bodyAge, loadSnapshot, readBirthYear, type Snapshot } from '../health';
import type { HealthOverview, HeartRatePoint, MetricSeries, SleepSession } from '../../types';

const overview = ref<HealthOverview | null>(null);
const sleep = ref<SleepSession[]>([]);
const heart = ref<HeartRatePoint[]>([]);
const series = ref<MetricSeries[]>([]);
const snap = ref<Snapshot | null>(null);
type StrapDay = Awaited<ReturnType<typeof backend.strapDaily>>[number];
const strap = ref<StrapDay[]>([]);

const { loading } = useReload(async () => {
  const [o, s, h, m, sn] = await Promise.all([
    backend.getHealthOverview().catch(() => null),
    backend.getRecentSleep(90).catch(() => [] as SleepSession[]),
    backend.getHeartRateSeries(24).catch(() => [] as HeartRatePoint[]),
    fetchSeries(METRICS_ALL, 90).catch(() => [] as MetricSeries[]),
    loadSnapshot().catch(() => null),
  ]);
  overview.value = o; sleep.value = s; heart.value = h; series.value = m; snap.value = sn;
  strap.value = await backend.strapDaily().catch(() => []);
});

const hour = new Date().getHours();
const greeting = hour < 5 ? 'Still up' : hour < 12 ? 'Good morning' : hour < 18 ? 'Good afternoon' : 'Good evening';
const today = longDay(new Date());
const get = (name: string) => series.value.find((s) => s.metric === name);
const week = lastDays(7);

/* Sleep */
const allNights = computed(() => nights(sleep.value));
const lastNight = computed(() => allNights.value[0] ?? null);
const lastNightIsRecent = computed(() => Boolean(lastNight.value) && (Date.now() - new Date(lastNight.value!.end_time).getTime()) < 36 * 3600 * 1000);
const usualSleep = computed(() => median(allNights.value.slice(1, 31).map((n) => n.duration_minutes)));
const sleepHM = computed(() => hoursMinutes(lastNight.value?.duration_minutes));
const sleepByDay = computed(() => new Map(allNights.value.map((n) => [wakeDay(n), n])));

/* Resting heart rate */
const rest = computed(() => restingSeries(series.value));
const restMap = computed(() => byDate(rest.value));
const restPoints = computed(() => rest.value?.points ?? []);
const restLatest = computed(() => {
  const p = restPoints.value[restPoints.value.length - 1];
  if (!p) return null;
  return (new Date(todayKey()).getTime() - new Date(p.date).getTime()) / 86400000 <= 3 ? p : null;
});
const restBand = computed<[number, number] | null>(() => {
  const prior = restPoints.value.slice(-31, -1).map((p) => p.value);
  return prior.length < 7 ? null : [Math.round(quantile(prior, 0.1)!), Math.round(quantile(prior, 0.9)!)];
});
const restUsual = computed(() => median(restPoints.value.slice(-31, -1).map((p) => p.value)));

/* Steps and PAI */
const stepsMap = computed(() => byDate(get('steps')));
const stepsToday = computed(() => overview.value?.steps_today ?? stepsMap.value.get(todayKey()) ?? null);
const stepsGoal = computed(() => overview.value?.steps_goal ?? 10000);
const steps7 = computed(() => mean(week.map((k) => stepsMap.value.get(k)).filter((v): v is number => v != null && v > 300)));
const steps30 = computed(() => mean(lastDays(30).slice(0, 23).map((k) => stepsMap.value.get(k)).filter((v): v is number => v != null && v > 300)));

/* Body age */
const realAge = computed(() => { const y = readBirthYear(); return y ? new Date().getFullYear() - y : null; });
const age = computed(() => (snap.value && realAge.value != null ? bodyAge(realAge.value, snap.value.last30) : null));

/* Vitals strip */
const fmtSigned = (v: number, unit: string) => `${v > 0 ? '+' : v < 0 ? '−' : '±'}${Math.abs(Math.round(v)).toLocaleString('en-GB')}${unit} vs usual`;
const strapByDay = computed(() => new Map(strap.value.map((d) => [d.date, d])));
const strapLatest = (k: keyof StrapDay) => { for (let i = strap.value.length - 1; i >= 0; i--) { const v = strap.value[i][k]; if (typeof v === 'number') return v; } return null; };
const strapUsual = (k: keyof StrapDay) => median(strap.value.slice(0, -1).map((d) => d[k]).filter((v): v is number => typeof v === 'number'));
const strapWeek = (k: keyof StrapDay) => week.map((d) => { const v = strapByDay.value.get(d)?.[k]; return typeof v === 'number' ? v : null; });
const readiness = computed(() => strapLatest('readiness'));
const readinessWord = computed(() => (readiness.value == null ? '' : readiness.value >= 80 ? 'Ready to push' : readiness.value >= 60 ? 'Take it steady' : 'Recover today'));
const strapVital = (k: keyof StrapDay, label: string, unit: string, color: string, digits: number, higherIsGood: boolean | null, to: string) => {
  const v = strapLatest(k); const u = strapUsual(k); const d = v != null && u != null ? v - u : null;
  const small = digits ? 0.15 : 1;
  return { label, to, color, unit, value: v == null ? null : v.toFixed(digits), week: strapWeek(k),
    change: d == null ? 'From your strap' : `${d > 0 ? '+' : d < 0 ? '−' : '±'}${Math.abs(d).toFixed(digits)}${unit === '°C' ? '°' : ''} vs usual`,
    tone: (d == null || Math.abs(d) < small || higherIsGood == null ? 'flat' : (d > 0) === higherIsGood ? 'good' : 'bad') as 'good' | 'bad' | 'flat' };
};

const vitals = computed(() => {
  const restDelta = restLatest.value && restUsual.value ? restLatest.value.value - restUsual.value : null;
  const sleepDelta = lastNightIsRecent.value && lastNight.value && usualSleep.value ? lastNight.value.duration_minutes - usualSleep.value : null;
  const stepDelta = steps7.value != null && steps30.value != null ? steps7.value - steps30.value : null;
  return [
    { label: 'Resting HR', to: '/heart', color: 'var(--v-heart)', value: restLatest.value ? String(Math.round(restLatest.value.value)) : null, unit: 'bpm',
      week: week.map((k) => restMap.value.get(k) ?? null), change: restDelta == null ? null : fmtSigned(restDelta, ' bpm'),
      tone: restDelta == null || Math.abs(restDelta) < 2 ? 'flat' : restDelta > 0 ? 'bad' : 'good' },
    { label: 'Sleep', to: '/sleep', color: 'var(--v-sleep)', value: lastNightIsRecent.value && sleepHM.value ? `${sleepHM.value.h}:${String(sleepHM.value.m).padStart(2, '0')}` : null, unit: 'h',
      week: week.map((k) => { const n = sleepByDay.value.get(k); return n ? n.duration_minutes / 60 : null; }),
      change: sleepDelta == null ? null : `${sleepDelta >= 0 ? '+' : '−'}${duration(Math.abs(sleepDelta))} vs usual`,
      tone: sleepDelta == null || Math.abs(sleepDelta) < 20 ? 'flat' : sleepDelta > 0 ? 'good' : 'bad' },
    { label: 'Steps, 7-day avg', to: '/activity', color: 'var(--v-move)', value: steps7.value == null ? null : Math.round(steps7.value).toLocaleString('en-GB'), unit: '',
      week: week.map((k) => stepsMap.value.get(k) ?? null), change: stepDelta == null ? null : fmtSigned(stepDelta, ''),
      tone: stepDelta == null || Math.abs(stepDelta) < 500 ? 'flat' : stepDelta > 0 ? 'good' : 'bad' },
    strapVital('hrvRmssd', 'HRV', 'ms', 'var(--v-good)', 0, true, '/heart'),
    strapVital('spo2Avg', 'Blood oxygen', '%', '#4fc3d9', 0, true, '/heart'),
    strapVital('skinTempC', 'Skin temperature', '°C', '#ff9a62', 1, null, '/heart'),
  ] as const;
});

/* Tonight: recommended bedtime from the usual wake time and an 8-hour need, plus last week's sleep debt. */
const NEED_MIN = 8 * 60;
const tonight = computed(() => {
  const recent = allNights.value.slice(0, 14);
  if (recent.length < 5) return null;
  const wakes = recent.map((n) => { const d = new Date(n.end_time); return d.getHours() * 60 + d.getMinutes(); });
  const wake = median(wakes)!;
  const bed = (wake - NEED_MIN - 15 + 24 * 60) % (24 * 60);
  const fmt = (m: number) => { const d = new Date(); d.setHours(Math.floor(m / 60), Math.round(m % 60), 0, 0); return clock(d.toISOString()); };
  const debt = week.reduce((sum, k) => { const n = sleepByDay.value.get(k); return n ? sum + Math.max(0, NEED_MIN - n.duration_minutes) : sum; }, 0);
  const tracked = week.filter((k) => sleepByDay.value.has(k)).length;
  return { bed: fmt(bed), wake: fmt(wake), debt, tracked };
});

/* Charts */
const days14 = lastDays(14);
const stepBars = computed(() => days14.map((key) => ({ key, label: weekday(key).slice(0, 2), value: stepsMap.value.get(key) ?? null, tip: shortDay(key) })));
const heartPoints = computed(() => heart.value.map((p) => ({ t: new Date(p.timestamp).getTime(), v: p.value })).filter((p) => Number.isFinite(p.t)));
const heartLatest = computed(() => heart.value[heart.value.length - 1] ?? null);
const heartRange = computed(() => { const v = heart.value.map((p) => p.value); return v.length ? { lo: Math.min(...v), hi: Math.max(...v) } : null; });
const hourLabel = (t: number) => clock(new Date(t).toISOString()).replace(':00', '');
const sleepRating = (score: number | null | undefined) => (score == null ? 'none' : score >= 80 ? 'good' : score >= 60 ? 'fair' : 'poor');
const sleepWord = (score: number | null | undefined) => (score == null ? 'No score' : score >= 80 ? 'Good' : score >= 60 ? 'Fair' : 'Poor');

const nothing = computed(() => !loading.value && !allNights.value.length && !heart.value.length && !stepsMap.value.size);
</script>

<template>
  <div class="v4-page today">
    <header class="v4-head">
      <div>
        <p class="hello">{{ greeting }}</p>
        <h1 class="v4-title">{{ today }}</h1>
      </div>
    </header>

    <div v-if="nothing" class="v4-card">
      <div class="v4-empty"><strong>No data on this computer yet</strong>Connect your Zepp account in Settings, then sync.</div>
      <RouterLink to="/settings/account" class="v4-btn primary" style="margin-top: 14px">Open Settings</RouterLink>
    </div>

    <template v-else>
      <section class="dials v4-card" aria-label="Today at a glance">
        <Dial to="/sleep" label="Sleep score" :value="lastNightIsRecent ? (lastNight?.score ?? null) : null" :min="0" :max="100" color="var(--v-sleep)"
          :display="String(lastNight?.score ?? '')" :caption="lastNightIsRecent && sleepHM ? `${sleepHM.h} h ${sleepHM.m} min asleep` : 'No sleep recorded last night'" />
        <Dial v-if="readiness != null" to="/heart" label="Readiness" :value="readiness" :min="0" :max="100" color="var(--v-good)"
          :display="String(Math.round(readiness))" :caption="readinessWord" />
        <Dial v-else to="/heart" label="Resting heart rate" :value="restLatest?.value ?? null" :min="40" :max="100" color="var(--v-heart)"
          :display="String(Math.round(restLatest?.value ?? 0))" unit="bpm" marker :band="restBand" :caption="restBand ? `Usual ${restBand[0]}–${restBand[1]} bpm` : ''" />
        <Dial to="/age" label="Body age" :value="age && age.counted >= 3 ? age.age : null" :min="(realAge ?? 20) - 8" :max="(realAge ?? 20) + 8" color="var(--v-brand)"
          :display="age ? age.age.toFixed(1) : ''" marker :band="realAge != null ? [realAge - 0.4, realAge + 0.4] : null"
          :caption="age && age.counted >= 3 ? `${Math.abs(age.delta).toFixed(1)} y ${age.delta > 0 ? 'older' : 'younger'} than ${realAge}` : realAge == null ? 'Add your birth year' : 'Needs 14 days of data'" />
        <Dial to="/activity" label="Steps today" :value="stepsToday" :min="0" :max="stepsGoal" color="var(--v-move)"
          :display="(stepsToday ?? 0).toLocaleString('en-GB')" :caption="`${Math.round(((stepsToday ?? 0) / stepsGoal) * 100)}% of ${stepsGoal.toLocaleString('en-GB')}`" />
      </section>

      <AiInsight topic="today" title="Your day, by Zebra AI" class="block" />

      <section class="vitals" aria-label="Vitals">
        <Vital v-for="v in vitals" :key="v.label" v-bind="v" />
      </section>

      <div class="v4-grid-2 block">
        <RouterLink v-if="lastNight" :to="`/sleep/${encodeURIComponent(lastNight.sleep_id)}`" class="v4-card">
          <div class="v4-card-head">
            <h2 class="v4-card-title">{{ lastNightIsRecent ? 'Last night' : `Last recorded night, ${shortDay(wakeDay(lastNight))}` }}</h2>
            <span :class="['v4-pill', sleepRating(lastNight.score)]">{{ sleepWord(lastNight.score) }}</span>
          </div>
          <div class="big-line">
            <span class="v4-num big">{{ hoursMinutes(lastNight.duration_minutes)?.h }}</span><span class="v4-unit">h</span>
            <span class="v4-num big" style="margin-left: 10px">{{ hoursMinutes(lastNight.duration_minutes)?.m }}</span><span class="v4-unit">min</span>
            <span class="span-time">{{ clock(lastNight.start_time) }} to {{ clock(lastNight.end_time) }}</span>
          </div>
          <StageBar :deep="lastNight.deep_minutes" :light="lastNight.light_minutes" :rem="lastNight.rem_minutes" :awake="lastNight.awake_minutes" />
        </RouterLink>

        <section v-if="tonight" class="v4-card tonight">
          <div class="v4-card-head">
            <h2 class="v4-card-title">Tonight</h2>
            <span class="v4-card-note">For 8 hours of sleep</span>
          </div>
          <div class="tn-row">
            <div>
              <div class="v4-stat-label">Go to bed by</div>
              <div class="v4-num tn-big">{{ tonight.bed }}</div>
            </div>
            <svg class="tn-arrow" viewBox="0 0 60 12" aria-hidden="true"><path d="M0 6h52M46 1l6 5-6 5" /></svg>
            <div>
              <div class="v4-stat-label">Your usual wake time</div>
              <div class="v4-num tn-big">{{ tonight.wake }}</div>
            </div>
          </div>
          <div class="tn-debt">
            <div class="v4-stat-label">Sleep debt, last 7 days</div>
            <div class="tn-debt-row">
              <span class="v4-num tn-mid">{{ duration(tonight.debt) || '0 min' }}</span>
              <span :class="['v4-pill', tonight.debt > 300 ? 'poor' : tonight.debt > 120 ? 'fair' : 'good']">{{ tonight.debt > 300 ? 'High' : tonight.debt > 120 ? 'Some' : 'Low' }}</span>
            </div>
            <p class="hint">Hours short of 8 h on the {{ tonight.tracked }} tracked nights this week.</p>
          </div>
        </section>

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

        <RouterLink v-if="stepBars.some((b) => b.value != null)" to="/activity" class="v4-card">
          <div class="v4-card-head">
            <h2 class="v4-card-title">Steps, last 14 days</h2>
            <span v-if="steps7 != null" class="v4-card-note">7-day average {{ Math.round(steps7).toLocaleString('en-GB') }}</span>
          </div>
          <Bars :bars="stepBars" color="var(--v-move)" :height="190" :reference="stepsGoal" :reference-label="`Goal ${stepsGoal.toLocaleString('en-GB')}`" />
        </RouterLink>
      </div>
    </template>
  </div>
</template>

<style scoped>
.hello { color: var(--v-brand); font-weight: 700; font-size: 13px; margin: 0 0 6px; }
.age-chip {
  display: flex; align-items: baseline; gap: 10px; padding: 10px 16px; border-radius: 14px;
  background: var(--v-surface); border: 1px solid var(--v-line);
}
.age-chip:hover { border-color: var(--v-line-strong); }
.age-k { color: var(--v-muted); font-size: 12px; font-weight: 600; }
.age-v { font-size: 26px; }
.age-d { font-size: 12px; font-weight: 700; color: var(--v-muted); }
.age-d.up { color: var(--v-move); }
.age-d.down { color: var(--v-good); }
.age-set { color: var(--v-brand); font-weight: 700; font-size: 13px; }
.dials { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); padding: 20px 8px 6px; margin-bottom: 16px; }
.dials > :deep(*) + :deep(*) { border-left: 1px solid var(--v-line); }
.block { margin-bottom: 16px; }
.vitals { display: grid; grid-template-columns: repeat(6, minmax(0, 1fr)); gap: 10px; margin-bottom: 16px; }
@media (max-width: 1250px) { .vitals { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
.big-line { display: flex; align-items: baseline; margin-bottom: 18px; flex-wrap: wrap; }
.big { font-size: 44px; }
.span-time { margin-left: auto; color: var(--v-muted); font-size: 13px; }
.tn-row { display: flex; align-items: center; gap: 18px; }
.tn-big { font-size: 34px; margin-top: 4px; }
.tn-arrow { width: 48px; height: 12px; fill: none; stroke: var(--v-faint); stroke-width: 1.5; flex: none; }
.tn-debt { margin-top: 22px; padding-top: 18px; border-top: 1px solid var(--v-line); }
.tn-debt-row { display: flex; align-items: center; gap: 10px; margin-top: 4px; }
.tn-mid { font-size: 26px; }
.hint { color: var(--v-muted); font-size: 13px; margin: 8px 0 0; }
@media (max-width: 1100px) { .dials { grid-template-columns: repeat(2, minmax(0, 1fr)); } .dials > :deep(*:nth-child(3)) { border-left: 0; } }
@media (max-width: 860px) { .vitals { grid-template-columns: repeat(2, minmax(0, 1fr)); } .dials { grid-template-columns: 1fr; } .dials > :deep(*) + :deep(*) { border-left: 0; } }
</style>
