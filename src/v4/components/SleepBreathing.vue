<script setup lang="ts">
/* Breathing during sleep from the strap: the nightly oxygen-dip screen (the same check the
   iPhone app calls breathing quality) and last night's blood oxygen. A screen, not a diagnosis. */
import { computed, onMounted, ref } from 'vue';
import { backend } from '../data';
import type { StrapDetail } from '../../lib/bridge/types';

const data = ref<StrapDetail | null>(null);
onMounted(async () => { data.value = await backend.strapDetail().catch(() => null); });
const nights = computed(() => (data.value?.breathing ?? []).filter((n) => (n.hoursChecked ?? 0) > 0));
const latest = computed(() => nights.value[0] ?? null);
const perHour = (n: { dips: number | null; hoursChecked: number | null }) => (n.hoursChecked ? (n.dips ?? 0) / n.hoursChecked : null);
const verdict = computed(() => {
  const r = latest.value ? perHour(latest.value) : null;
  if (r == null) return null;
  return r < 5 ? { text: 'Normal', tone: 'good' } : r < 15 ? { text: 'Some dips', tone: 'fair' } : { text: 'Many dips', tone: 'poor' };
});
const day = (ms: number) => new Date(ms).toLocaleDateString('en-GB', { weekday: 'short', day: 'numeric', month: 'short' });
</script>

<template>
  <section v-if="latest || data?.nightOxygen" class="v4-card">
    <div class="v4-card-head">
      <h2 class="v4-card-title">Breathing during sleep</h2>
      <span v-if="verdict" :class="['v4-pill', verdict.tone]">{{ verdict.text }}</span>
    </div>
    <div class="v4-stats">
      <div v-if="latest"><div class="v4-stat-label">Oxygen dips last night</div><div class="v4-stat-value">{{ latest.dips ?? 0 }} <span class="v4-unit">in {{ latest.hoursChecked }} h</span></div></div>
      <div v-if="latest"><div class="v4-stat-label">Breathing score</div><div class="v4-stat-value">{{ latest.score ?? '–' }}</div></div>
      <div v-if="data?.nightOxygen"><div class="v4-stat-label">Night oxygen, average</div><div class="v4-stat-value">{{ data.nightOxygen.avg }} <span class="v4-unit">%</span></div></div>
      <div v-if="data?.nightOxygen"><div class="v4-stat-label">Lowest</div><div class="v4-stat-value">{{ data.nightOxygen.min }} <span class="v4-unit">%</span></div></div>
    </div>
    <div v-if="nights.length > 1" class="rows">
      <div v-for="n in nights.slice(0, 7)" :key="n.night" class="row">
        <span>{{ day(n.night) }}</span><span>{{ n.dips ?? 0 }} dips in {{ n.hoursChecked }} h</span><span class="score">{{ n.score }}</span>
      </div>
    </div>
    <p class="note">Under 5 oxygen dips an hour is normal. This is a screen from your strap, not a diagnosis. If you snore loudly or wake up tired, talk to a doctor.</p>
  </section>
</template>

<style scoped>
.rows { margin-top: 14px; border-top: 1px solid var(--v-line); }
.row { display: grid; grid-template-columns: 120px 1fr auto; gap: 12px; padding: 8px 0; font-size: 13px; border-bottom: 1px solid var(--v-line); }
.row span:first-child { color: var(--v-muted); }
.score { font-weight: 700; font-variant-numeric: tabular-nums; }
.note { margin: 12px 0 0; color: var(--v-faint); font-size: 12px; }
</style>
