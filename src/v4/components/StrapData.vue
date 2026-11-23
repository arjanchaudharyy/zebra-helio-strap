<script setup lang="ts">
/* "From your strap": metrics downloaded straight from the band over Bluetooth. */
import { computed, onMounted, ref } from 'vue';
import { RouterLink } from 'vue-router';
import LineChart from './LineChart.vue';
import { backend, median, shortDay } from '../data';

type Day = Awaited<ReturnType<typeof backend.strapDaily>>[number];
const days = ref<Day[]>([]);
const loaded = ref(false);
onMounted(async () => { days.value = await backend.strapDaily().catch(() => []); loaded.value = true; });

const latest = (k: keyof Day) => { for (let i = days.value.length - 1; i >= 0; i--) { const v = days.value[i][k]; if (typeof v === 'number') return v; } return null; };
const usual = (k: keyof Day) => median(days.value.slice(-31, -1).map((d) => d[k]).filter((v): v is number => typeof v === 'number'));
const tiles = computed(() => [
  { k: 'hrvRmssd' as const, label: 'HRV (RMSSD)', unit: 'ms', up: 'good' },
  { k: 'skinTempC' as const, label: 'Skin temperature', unit: '°C', up: 'flat' },
  { k: 'spo2Avg' as const, label: 'Blood oxygen', unit: '%', up: 'good' },
  { k: 'stressAvg' as const, label: 'Stress', unit: '/100', up: 'bad' },
  { k: 'breathRate' as const, label: 'Sleep breathing', unit: '/min', up: 'flat' },
].map((t) => {
  const v = latest(t.k); const u = usual(t.k);
  const d = v != null && u != null ? v - u : null;
  return { ...t, value: v, delta: d };
}));
const hrvPoints = computed(() => days.value.filter((d) => d.hrvRmssd != null).map((d) => ({ t: new Date(`${d.date}T12:00:00`).getTime(), v: d.hrvRmssd as number })));
const dayLabel = (t: number) => shortDay(new Date(t).toISOString().slice(0, 10));
const fmt = (v: number, unit: string) => (unit === '°C' ? v.toFixed(1) : Math.round(v).toString());
</script>

<template>
  <section class="v4-card strap">
    <div class="v4-card-head">
      <h2 class="v4-card-title">From your strap</h2>
      <span class="v4-card-note">Downloaded over Bluetooth, not from the cloud</span>
    </div>
    <template v-if="days.length">
      <div class="tiles">
        <div v-for="t in tiles" :key="t.k" class="tile">
          <div class="v4-stat-label">{{ t.label }}</div>
          <div class="v4-stat-value">{{ t.value == null ? '–' : fmt(t.value, t.unit) }}<span v-if="t.value != null" class="v4-unit">{{ t.unit }}</span></div>
          <div v-if="t.delta != null" class="delta">{{ t.delta > 0 ? '+' : '' }}{{ t.unit === '°C' ? t.delta.toFixed(1) : Math.round(t.delta) }} vs usual</div>
        </div>
      </div>
      <LineChart v-if="hrvPoints.length > 1" :points="hrvPoints" color="var(--v-good)" :height="160" :gap-ms="3 * 86400000" :x-label="dayLabel" unit="ms" show-dots />
    </template>
    <div v-else-if="loaded" class="v4-empty">
      <strong>Not downloaded yet</strong>
      Your strap keeps HRV, skin temperature every minute, SpO₂, stress and sleep breathing. Unlock it on the
      <RouterLink to="/live" class="lnk">Live page</RouterLink> to bring them here.
    </div>
  </section>
</template>

<style scoped>
.strap { margin-top: 16px; }
.tiles { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 16px; margin-bottom: 20px; }
.delta { color: var(--v-faint); font-size: 12px; margin-top: 4px; }
.lnk { color: var(--v-brand); font-weight: 600; }
@media (max-width: 900px) { .tiles { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
