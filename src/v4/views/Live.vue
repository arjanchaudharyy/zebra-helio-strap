<script setup lang="ts">
/* Live: heart rate straight from the strap over Bluetooth, about once a second. */
defineOptions({ name: 'Live' });
import { computed, onBeforeUnmount, onMounted, ref } from 'vue';
import LineChart from '../components/LineChart.vue';
import { backend, clock, fetchSeries, median, restingSeries } from '../data';
import { isDesktop } from '../../lib/bridge';

type StrapEvent =
  | { kind: 'status'; state: string; detail?: string | null }
  | { kind: 'info'; name: string; battery?: number | null; firmware?: string | null }
  | { kind: 'hr'; bpm: number; at: number; rr: number[] };

const state = ref<'idle' | 'scanning' | 'connecting' | 'live' | 'error' | 'stopped' | 'reconnecting'>('idle');
const detail = ref<string | null>(null);
const info = ref<{ name: string; battery?: number | null; firmware?: string | null } | null>(null);
const samples = ref<{ t: number; v: number }[]>([]);
const rr = ref<number[]>([]);
const resting = ref<number | null>(null);
const nightHrv = ref<number | null>(null);
let unlisten: (() => void) | null = null;
let demoTimer = 0;

const WINDOW_MS = 30 * 60 * 1000;
const bpm = computed(() => samples.value[samples.value.length - 1]?.v ?? null);
const windowPoints = computed(() => {
  const cut = Date.now() - WINDOW_MS;
  return samples.value.filter((p) => p.t >= cut);
});
/* One plain sentence about the number, compared with the person's own resting rate. */
const zone = computed(() => {
  if (bpm.value == null || resting.value == null) return null;
  const d = bpm.value - resting.value;
  if (d < 8) return { label: 'Calm', text: 'Your heart is at its resting pace.', tone: 'good' };
  if (d < 25) return { label: 'Light effort', text: 'A little above resting, like walking around.', tone: 'fair' };
  if (d < 50) return { label: 'Working', text: 'Your heart is working, like a brisk walk or easy jog.', tone: 'fair' };
  return { label: 'Hard effort', text: 'High effort. Ease off if you are not exercising.', tone: 'poor' };
});
const beatMs = computed(() => (bpm.value ? Math.round(60000 / bpm.value) : 1000));
const stateText = computed(() => ({
  idle: 'Not connected', scanning: 'Looking for your strap…', connecting: 'Connecting…', live: 'Live', error: detail.value ?? 'Something went wrong', stopped: 'Stopped', reconnecting: 'Reconnecting to your strap…',
} as Record<string, string>)[state.value]);

const onEvent = (e: StrapEvent) => {
  if (e.kind === 'status') { state.value = e.state as typeof state.value; detail.value = e.detail ?? null; }
  else if (e.kind === 'info') info.value = e;
  else if (e.kind === 'hr') {
    samples.value.push({ t: e.at, v: e.bpm });
    if (samples.value.length > 5000) samples.value.splice(0, samples.value.length - 5000);
    if (e.rr?.length) { rr.value.push(...e.rr); if (rr.value.length > 600) rr.value.splice(0, rr.value.length - 600); }
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

onMounted(async () => {
  if (isDesktop()) {
    unlisten = await backend.listen<StrapEvent>('zebra://strap', onEvent);
    // The strap stays connected in the background; show the saved last 30 minutes at once.
    const saved = await backend.strapLiveRecent(30).catch(() => []);
    samples.value = saved.map((r) => ({ t: r.t, v: r.bpm }));
    if (saved.length && Date.now() - saved[saved.length - 1].t < 15000) state.value = 'live';
  }
  const series = await fetchSeries(['resting_hr', 'device_resting_hr'], 30).catch(() => []);
  resting.value = median((restingSeries(series)?.points ?? []).map((p) => p.value));
  const strap = await backend.strapDaily().catch(() => []);
  for (let i = strap.length - 1; i >= 0; i--) { if (strap[i].hrvRmssd != null) { nightHrv.value = Math.round(strap[i].hrvRmssd as number); break; } }
  void start();
});
onBeforeUnmount(() => { unlisten?.(); window.clearInterval(demoTimer); });
const timeLabel = (t: number) => clock(new Date(t).toISOString());
</script>

<template>
  <div class="v4-page live">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Live</h1>
        <p class="v4-sub">Your heart rate right now. Zebra stays connected and saves every reading.</p>
      </div>
      <button v-else type="button" class="v4-btn primary" @click="start">Connect</button>
    </header>

    <section class="v4-card hero">
      <div :class="['status', state]"><span class="led" />{{ stateText }}</div>
      <div class="beat">
        <svg viewBox="0 0 24 24" :class="['heart', { on: state === 'live' && bpm }]" :style="{ animationDuration: `${beatMs}ms` }" aria-hidden="true"><path d="M12 20.3s-7.8-4.6-7.8-10.4A4.3 4.3 0 0 1 12 7.3a4.3 4.3 0 0 1 7.8 2.6c0 5.8-7.8 10.4-7.8 10.4Z" /></svg>
        <div class="bpm"><span class="v4-num">{{ bpm ?? '–' }}</span><span class="unit">beats a minute</span></div>
      </div>
      <p v-if="zone" class="meaning"><strong>{{ zone.label }}.</strong> {{ zone.text }}<template v-if="resting"> Your resting rate is {{ Math.round(resting) }}.</template></p>
      <p v-else class="meaning">Wear your strap and keep it near this Mac.</p>
    </section>

    <section class="v4-card chart">
      <div class="v4-card-head"><h2 class="v4-card-title">Last 30 minutes</h2></div>
      <LineChart v-if="windowPoints.length > 1" :points="windowPoints" color="var(--v-heart)" :height="200" :reference="resting" reference-label="Resting" :gap-ms="10000" :x-label="timeLabel" :x-ticks="4" unit="bpm" />
      <p v-else class="v4-empty">The line appears after a few seconds of readings.</p>
    </section>

    <p v-if="info" class="device">{{ info.name }}<template v-if="info.battery != null"> · battery {{ info.battery }}%</template></p>
  </div>
</template>

<style scoped>
.live { max-width: 900px; }
.hero { padding: 26px 28px 28px; margin-bottom: 16px; }
.status { display: inline-flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 600; color: var(--v-muted); }
.led { width: 8px; height: 8px; border-radius: 50%; background: var(--v-faint); }
.status.live .led { background: var(--v-good); }
.status.scanning .led, .status.connecting .led { background: var(--v-move); animation: blink 1s infinite; }
.status.error { color: var(--v-heart); }
.status.error .led { background: var(--v-heart); }
@keyframes blink { 50% { opacity: 0.3; } }
.beat { display: flex; align-items: center; gap: 22px; margin: 22px 0 12px; }
.heart { width: 56px; height: 56px; fill: var(--v-heart); opacity: 0.35; flex: none; }
.heart.on { opacity: 1; animation: beat 1s infinite ease-out; }
@keyframes beat { 0% { transform: scale(1); } 12% { transform: scale(1.12); } 30% { transform: scale(1); } }
.bpm { display: flex; align-items: baseline; gap: 10px; }
.bpm .v4-num { font-size: 96px; letter-spacing: -0.04em; }
.unit { color: var(--v-muted); font-size: 16px; font-weight: 500; }
.meaning { margin: 4px 0 0; font-size: 16px; color: var(--v-muted); line-height: 1.5; }
.meaning strong { color: var(--v-text); }
.chart { margin-bottom: 14px; }
.device { color: var(--v-faint); font-size: 12px; text-align: center; margin: 0; }
@media (prefers-reduced-motion: reduce) { .heart.on { animation: none; } }
</style>
