<script setup lang="ts">
/* A whole day from the strap's own memory: heart rate (from per-second records), steps and stress,
   plus PAI. Covers the hours Zebra was closed, once the strap history is synced. */
import { computed, ref, watch } from 'vue';
import LineChart from './LineChart.vue';
import Bars from './Bars.vue';
import { backend, clock } from '../data';
import type { StrapDetail } from '../../lib/bridge/types';

const offset = ref(0); // 0 = today, 1 = yesterday...
const data = ref<StrapDetail | null>(null);
const dayKey = computed(() => { const d = new Date(); d.setDate(d.getDate() - offset.value); return d.toLocaleDateString('en-CA'); });
let first = true;
const load = async () => {
  data.value = await backend.strapDetail(dayKey.value).catch(() => null);
  // Early in the day there is little to show: open on yesterday instead.
  if (first && offset.value === 0 && data.value && data.value.heartRate.length < 60) { first = false; offset.value = 1; return; }
  first = false;
};
watch(dayKey, load, { immediate: true });

const hr = computed(() => (data.value?.heartRate ?? []).map((p) => ({ t: p.t, v: p.bpm })));
const stress = computed(() => (data.value?.stress ?? []).map((p) => ({ t: p.t, v: p.v })));
const stats = computed(() => {
  const v = hr.value.map((p) => p.v);
  return v.length ? { lo: Math.min(...v), hi: Math.max(...v), avg: Math.round(v.reduce((a, b) => a + b, 0) / v.length) } : null;
});
const hourly = computed(() => {
  const out = Array.from({ length: 24 }, (_, h) => ({ key: String(h), label: h % 6 === 0 ? `${String(h).padStart(2, '0')}:00` : '', value: 0 as number | null }));
  for (const s of data.value?.steps ?? []) out[new Date(s.t).getHours()].value = (out[new Date(s.t).getHours()].value ?? 0) + s.steps;
  return out;
});
const label = computed(() => (offset.value === 0 ? 'Today' : offset.value === 1 ? 'Yesterday' : new Date(dayKey.value).toLocaleDateString('en-GB', { weekday: 'long', day: 'numeric', month: 'short' })));
const skin = computed(() => (data.value?.skinTemp ?? []).map((p) => ({ t: p.t, v: p.c })));
const skinRange = computed(() => { const v = skin.value.map((p) => p.v); return v.length ? { lo: Math.min(...v).toFixed(1), hi: Math.max(...v).toFixed(1) } : null; });
const mins = (n: number) => (n >= 60 ? `${Math.floor(n / 60)} h ${n % 60} min` : `${n} min`);
const moves = computed(() => {
  const m = data.value?.minutes; if (!m) return [];
  return [
    { k: 'Sitting', v: m.sit, c: '#8e8e93' }, { k: 'Standing', v: m.stand, c: '#64d2ff' },
    { k: 'Walking', v: m.walk, c: '#30d158' }, { k: 'Running', v: m.run, c: '#ff9f0a' },
    { k: 'Fat burn', v: m.fatBurn, c: '#ff375f' },
  ].filter((x) => x.v > 0);
});
const t = (ms: number) => clock(new Date(ms).toISOString());
</script>

<template>
  <section class="v4-card strap-day">
    <div class="v4-card-head">
      <h2 class="v4-card-title">Your day from the strap</h2>
      <div class="nav">
        <button type="button" class="glass-btn" aria-label="Previous day" @click="offset++">‹</button>
        <span class="day">{{ label }}</span>
        <button type="button" class="glass-btn" aria-label="Next day" :disabled="offset === 0" @click="offset--">›</button>
      </div>
    </div>
    <p v-if="data && !hr.length" class="v4-empty">No strap data for this day yet. Zebra downloads the strap's memory every hour.</p>
    <template v-else-if="data">
      <div class="v4-stats">
        <div><div class="v4-stat-label">Heart rate</div><div class="v4-stat-value">{{ stats?.lo }}–{{ stats?.hi }} <span class="v4-unit">bpm</span></div></div>
        <div><div class="v4-stat-label">Average</div><div class="v4-stat-value">{{ stats?.avg }} <span class="v4-unit">bpm</span></div></div>
        <div><div class="v4-stat-label">Steps</div><div class="v4-stat-value">{{ Math.round(data.totalSteps).toLocaleString('en-GB') }}</div></div>
        <div><div class="v4-stat-label">Worn</div><div class="v4-stat-value">{{ Math.floor(data.wornMinutes / 60) }} h {{ data.wornMinutes % 60 }} min</div></div>
      </div>
      <p class="sub">Heart rate, from {{ data.heartRateReadings.toLocaleString('en-GB') }} readings stored on the strap</p>
      <LineChart :points="hr" color="var(--v-heart)" :height="200" :gap-ms="10 * 60000" :x-label="t" unit="bpm" />
      <p class="sub">Steps each hour</p>
      <Bars :bars="hourly" color="var(--v-move)" :height="110" />
      <template v-if="stress.length > 1">
        <p class="sub">Stress (0 to 100, under 40 is relaxed)</p>
        <LineChart :points="stress" color="var(--v-move)" :height="120" :gap-ms="10 * 60000" :x-label="t" />
      </template>
      <template v-if="skin.length > 1">
        <p class="sub">Skin temperature, {{ skinRange?.lo }} to {{ skinRange?.hi }} °C (it drops when you take the strap off or get cold)</p>
        <LineChart :points="skin" color="#bf5af2" :height="110" :gap-ms="30 * 60000" :x-label="t" unit="°C" />
      </template>
      <template v-if="moves.length">
        <p class="sub">How you spent your time<span v-if="data.distanceM">, {{ (data.distanceM / 1000).toFixed(1) }} km walked</span></p>
        <div class="moves">
          <div v-for="m in moves" :key="m.k"><i :style="{ background: m.c }" /><span class="k">{{ m.k }}</span><span class="mv">{{ mins(m.v) }}</span></div>
        </div>
      </template>
      <div v-if="data.pai" class="pai">
        <div><span class="k">PAI, last 7 days</span><span class="v">{{ Math.round(data.pai.total ?? 0) }}</span><span class="hint">100 or more is linked to a healthier heart</span></div>
        <div><span class="k">Today</span><span class="v">+{{ (data.pai.daily ?? 0).toFixed(1) }}</span></div>
        <div><span class="k">Minutes above {{ data.pai.lowFrom }} / {{ data.pai.mediumFrom }} / {{ data.pai.highFrom }} bpm</span><span class="v">{{ data.pai.lowMinutes }} / {{ data.pai.mediumMinutes }} / {{ data.pai.highMinutes }}</span></div>
      </div>
    </template>
  </section>
</template>

<style scoped>
.nav { display: flex; align-items: center; gap: 8px; }
.nav .glass-btn { width: 30px; height: 28px; padding: 0; font-size: 16px; }
.day { min-width: 92px; text-align: center; font-size: 13px; font-weight: 600; }
.sub { margin: 18px 0 6px; color: var(--v-muted); font-size: 12.5px; }
.moves { display: grid; grid-template-columns: repeat(auto-fill, minmax(120px, 1fr)); gap: 10px; }
.moves > div { display: grid; grid-template-columns: 8px 1fr; column-gap: 8px; align-items: center; }
.moves i { width: 8px; height: 8px; border-radius: 4px; grid-row: span 2; }
.mv { font-size: 15px; font-weight: 700; font-variant-numeric: tabular-nums; }
.pai { margin-top: 18px; display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 12px; border-top: 1px solid var(--v-line); padding-top: 14px; }
.pai > div { display: grid; gap: 2px; }
.k { color: var(--v-muted); font-size: 12px; }
.v { font-size: 20px; font-weight: 700; font-variant-numeric: tabular-nums; }
.hint { color: var(--v-faint); font-size: 11.5px; }
@media (max-width: 800px) { .pai { grid-template-columns: 1fr; } }
</style>
