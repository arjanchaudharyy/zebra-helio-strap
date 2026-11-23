<script setup lang="ts">
/* Live: heart rate straight from the strap over Bluetooth, about once a second. */
defineOptions({ name: 'Live' });
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import LineChart from '../components/LineChart.vue';
import PixelIcon from '../components/PixelIcon.vue';
import { backend, clock, fetchSeries, median, restingSeries } from '../data';
import { isDesktop } from '../../lib/bridge';

type StrapEvent =
  | { kind: 'status'; state: string; detail?: string | null }
  | { kind: 'info'; name: string; battery?: number | null; firmware?: string | null }
  | { kind: 'hr'; bpm: number; at: number; rr: number[] };

const state = ref<'idle' | 'scanning' | 'connecting' | 'live' | 'error' | 'stopped'>('idle');
const detail = ref<string | null>(null);
const info = ref<{ name: string; battery?: number | null; firmware?: string | null } | null>(null);
const samples = ref<{ t: number; v: number }[]>([]);
const rr = ref<number[]>([]);
const resting = ref<number | null>(null);
let unlisten: (() => void) | null = null;
let demoTimer = 0;

const WINDOW_MS = 2 * 60 * 1000;
const bpm = computed(() => samples.value[samples.value.length - 1]?.v ?? null);
const windowPoints = computed(() => {
  const cut = Date.now() - WINDOW_MS;
  return samples.value.filter((p) => p.t >= cut);
});
const stats = computed(() => {
  const v = windowPoints.value.map((p) => p.v);
  return v.length ? { lo: Math.min(...v), hi: Math.max(...v), avg: Math.round(v.reduce((a, b) => a + b, 0) / v.length) } : null;
});
/* RMSSD from beat-to-beat intervals: the standard short-term HRV measure. */
const rmssd = computed(() => {
  const r = rr.value.slice(-120).filter((x) => x > 300 && x < 2000);
  if (r.length < 20) return null;
  let sum = 0;
  for (let i = 1; i < r.length; i++) sum += (r[i] - r[i - 1]) ** 2;
  return Math.round(Math.sqrt(sum / (r.length - 1)));
});
const zone = computed(() => {
  if (bpm.value == null || resting.value == null) return null;
  const d = bpm.value - resting.value;
  if (d < 8) return { label: 'Resting', tone: 'good' };
  if (d < 25) return { label: 'Light', tone: 'fair' };
  if (d < 50) return { label: 'Active', tone: 'fair' };
  return { label: 'Hard', tone: 'poor' };
});
const beatMs = computed(() => (bpm.value ? Math.round(60000 / bpm.value) : 1000));
const stateText = computed(() => ({
  idle: 'Not connected', scanning: 'Looking for your strap…', connecting: 'Connecting…', live: 'Live', error: detail.value ?? 'Something went wrong', stopped: 'Stopped',
} as Record<string, string>)[state.value]);

const onEvent = (e: StrapEvent) => {
  if (e.kind === 'status') { state.value = e.state as typeof state.value; detail.value = e.detail ?? null; }
  else if (e.kind === 'info') info.value = e;
  else if (e.kind === 'hr') {
    samples.value.push({ t: e.at, v: e.bpm });
    if (samples.value.length > 3600) samples.value.splice(0, samples.value.length - 3600);
    if (e.rr?.length) { rr.value.push(...e.rr); if (rr.value.length > 600) rr.value.splice(0, rr.value.length - 600); }
  }
};

const unlock = ref<{ busy: boolean; text: string | null; ok: boolean }>({ busy: false, text: null, ok: false });
const fetchKey = async () => {
  unlock.value = { busy: true, text: null, ok: false };
  try {
    const n = await backend.strapKeyFetch();
    unlock.value = n > 0
      ? { busy: false, ok: true, text: 'Key saved privately on this Mac. Full strap history, HRV, skin temperature and vibration can now be unlocked.' }
      : { busy: false, ok: false, text: 'Your Zepp account did not return a key for this strap.' };
  } catch (e) {
    unlock.value = { busy: false, ok: false, text: (e as { message?: string })?.message ?? String(e) };
  }
};

const start = async () => {
  detail.value = null;
  if (!isDesktop()) {
    // Browser preview: a gentle fake signal so the page can be seen.
    state.value = 'live'; info.value = { name: 'Amazfit Helio Strap (preview)', battery: 74, firmware: '3.27.0.1' };
    window.clearInterval(demoTimer);
    demoTimer = window.setInterval(() => onEvent({ kind: 'hr', bpm: Math.round(68 + 4 * Math.sin(Date.now() / 9000) + Math.random() * 2), at: Date.now(), rr: [] }), 1000);
    return;
  }
  state.value = 'scanning';
  await backend.strapLiveStart();
};
const stop = async () => {
  window.clearInterval(demoTimer);
  if (isDesktop()) await backend.strapLiveStop().catch(() => undefined);
  state.value = 'stopped';
};

onMounted(async () => {
  if (isDesktop()) unlisten = await backend.listen<StrapEvent>('zebra://strap', onEvent);
  const series = await fetchSeries(['resting_hr', 'device_resting_hr'], 30).catch(() => []);
  resting.value = median((restingSeries(series)?.points ?? []).map((p) => p.value));
  void start();
});
onBeforeUnmount(() => { unlisten?.(); void stop(); });
const timeLabel = (t: number) => clock(new Date(t).toISOString());
</script>

<template>
  <div class="v4-page live">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Live</h1>
        <p class="v4-sub">Straight from your strap over Bluetooth. Nothing goes to the cloud.</p>
      </div>
      <button v-if="state === 'live' || state === 'scanning' || state === 'connecting'" type="button" class="v4-btn" @click="stop">Stop</button>
      <button v-else type="button" class="v4-btn primary" @click="start">Connect</button>
    </header>

    <section class="v4-card hero">
      <div :class="['status', state]"><span class="led" />{{ stateText }}</div>
      <div class="beat">
        <PixelIcon name="heart" :size="64" :class="['heart', { on: state === 'live' && bpm }]" :style="{ animationDuration: `${beatMs}ms` }" />
        <div>
          <div class="bpm"><span class="v4-num">{{ bpm ?? '–' }}</span><span class="unit">bpm</span></div>
          <div v-if="zone" :class="['v4-pill', zone.tone]">{{ zone.label }}</div>
        </div>
      </div>
      <div class="stats">
        <div><div class="v4-stat-label">2-min low</div><div class="v4-stat-value">{{ stats?.lo ?? '–' }}</div></div>
        <div><div class="v4-stat-label">2-min average</div><div class="v4-stat-value">{{ stats?.avg ?? '–' }}</div></div>
        <div><div class="v4-stat-label">2-min high</div><div class="v4-stat-value">{{ stats?.hi ?? '–' }}</div></div>
        <div><div class="v4-stat-label">Live HRV (RMSSD)</div><div class="v4-stat-value">{{ rmssd ?? '–' }}<span v-if="rmssd" class="v4-unit">ms</span></div></div>
        <div><div class="v4-stat-label">Your resting</div><div class="v4-stat-value">{{ resting ? Math.round(resting) : '–' }}<span v-if="resting" class="v4-unit">bpm</span></div></div>
      </div>
    </section>

    <section class="v4-card chart">
      <div class="v4-card-head"><h2 class="v4-card-title">Last 2 minutes</h2><span class="v4-card-note">{{ windowPoints.length }} readings</span></div>
      <LineChart v-if="windowPoints.length > 1" :points="windowPoints" color="var(--v-heart)" :height="200" :reference="resting" reference-label="Resting" :gap-ms="10000" :x-label="timeLabel" :x-ticks="4" unit="bpm" />
      <p v-else class="v4-empty">Readings appear here once the strap is connected.</p>
    </section>

    <section class="v4-card unlock">
      <div class="v4-card-head"><h2 class="v4-card-title">Unlock full strap data</h2><span :class="['v4-pill', unlock.ok ? 'good' : 'none']">{{ unlock.ok ? 'Unlocked' : 'Locked' }}</span></div>
      <p class="hint" style="margin-top: 0">Your strap stores heart rate every second, HRV, SpO₂, stress, sleep breathing and skin temperature every minute. Reading them needs the strap's pairing key from your own Zepp account (Advanced data in Settings).</p>
      <button type="button" class="v4-btn primary" :disabled="unlock.busy" style="margin-top: 14px" @click="fetchKey">{{ unlock.busy ? 'Getting key…' : 'Get key from my Zepp account' }}</button>
      <p v-if="unlock.text" :class="['hint', { err: !unlock.ok }]">{{ unlock.text }}</p>
    </section>

    <section v-if="info" class="v4-card device">
      <div class="v4-card-head"><h2 class="v4-card-title">{{ info.name }}</h2></div>
      <div class="v4-stats">
        <div><div class="v4-stat-label">Battery</div><div class="v4-stat-value">{{ info.battery ?? '–' }}<span v-if="info.battery != null" class="v4-unit">%</span></div></div>
        <div><div class="v4-stat-label">Firmware</div><div class="v4-stat-value small">{{ info.firmware ?? '–' }}</div></div>
        <div><div class="v4-stat-label">Link</div><div class="v4-stat-value small">Bluetooth LE</div></div>
      </div>
      <p class="hint">HRV here needs beat-to-beat data. If it stays empty, your strap is not sending it in broadcast mode.</p>
    </section>
  </div>
</template>

<style scoped>
.hero { padding: 26px 28px; margin-bottom: 16px; }
.status { display: inline-flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 600; color: var(--v-muted); }
.led { width: 8px; height: 8px; border-radius: 50%; background: var(--v-faint); }
.status.live .led { background: var(--v-good); box-shadow: 0 0 0 4px rgba(79, 209, 139, 0.15); }
.status.scanning .led, .status.connecting .led { background: var(--v-move); animation: blink 1s infinite; }
.status.error { color: var(--v-heart); }
.status.error .led { background: var(--v-heart); }
@keyframes blink { 50% { opacity: 0.3; } }
.beat { display: flex; align-items: center; gap: 28px; margin: 18px 0 26px; }
.heart { color: var(--v-heart); opacity: 0.35; }
.heart.on { opacity: 1; animation: beat 1s infinite ease-out; }
@keyframes beat { 0% { transform: scale(1); } 12% { transform: scale(1.12); } 30% { transform: scale(1); } }
.bpm { display: flex; align-items: baseline; gap: 8px; }
.bpm .v4-num { font-size: 112px; }
.unit { color: var(--v-muted); font-size: 18px; font-weight: 600; }
.stats { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 16px; padding-top: 20px; border-top: 1px solid var(--v-line); }
.chart, .unlock { margin-bottom: 16px; }
.hint.err { color: var(--v-heart); }
.small { font-size: 20px; }
.hint { color: var(--v-faint); font-size: 12px; margin: 14px 0 0; }
@media (prefers-reduced-motion: reduce) { .heart.on { animation: none; } }
@media (max-width: 900px) { .stats { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
