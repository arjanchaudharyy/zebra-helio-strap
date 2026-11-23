<script setup lang="ts">
/* Health watch: illness early warning plus recent wrist alerts from the strap. */
import { computed, onMounted, ref } from 'vue';
import { backend, clock } from '../data';

type Watch = Awaited<ReturnType<typeof backend.strapAlerts>>;
const data = ref<Watch | null>(null);
onMounted(async () => { data.value = await backend.strapAlerts().catch(() => null); });

const ill = computed(() => data.value?.illness ?? null);
const tone = computed(() => ({ warning: 'poor', watch: 'fair', normal: 'good' } as Record<string, string>)[ill.value?.state ?? ''] ?? 'none');
const word = computed(() => ({ warning: 'Possible illness', watch: 'Watch', normal: 'All normal', learning: 'Learning', no_data: 'No data' } as Record<string, string>)[ill.value?.state ?? 'no_data']);
const recent = computed(() => (data.value?.alerts ?? []).slice(0, 4));
const label: Record<string, string> = { high_hr: 'Heart rate', stress: 'Stress', spo2: 'Blood oxygen', resting_hr: 'Resting HR', illness: 'Illness watch' };
const when = (ts: number) => {
  const d = new Date(ts * 1000);
  const days = Math.floor((Date.now() - d.getTime()) / 86400000);
  return `${days === 0 ? 'Today' : days === 1 ? 'Yesterday' : d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' })}, ${clock(d.toISOString())}`;
};
</script>

<template>
  <section class="v4-card watch">
    <div class="v4-card-head">
      <h2 class="v4-card-title">Health watch</h2>
      <span class="v4-card-note">Checked every hour. New alerts buzz your strap.</span>
    </div>
    <div class="ill">
      <div class="ill-top">
        <span class="ill-name">Illness early warning</span>
        <span :class="['v4-pill', tone]">{{ word }}</span>
      </div>
      <p class="ill-text">{{ ill?.text ?? 'Unlock your strap on the Live page to start.' }}</p>
      <div v-if="ill && ill.temp != null && ill.baseTemp != null" class="ill-nums">
        <span>Last night {{ ill.temp.toFixed(1) }} °C</span><span>Your normal {{ ill.baseTemp.toFixed(1) }} °C</span>
      </div>
    </div>
    <div class="alerts">
      <div v-if="!recent.length" class="none">No alerts. Your heart rate, stress and blood oxygen stayed in your normal range.</div>
      <div v-for="a in recent" :key="a.id" class="alert">
        <span :class="['dot', a.kind]" />
        <div>
          <div class="a-top"><strong>{{ label[a.kind] ?? a.kind }}</strong><span>{{ when(a.at) }}</span></div>
          <div class="a-text">{{ a.text }}</div>
        </div>
      </div>
    </div>
    <p class="buzz-key">On your wrist: 3 quick, pause, 3 quick = health alert. 1 buzz = Claude Code finished. 2 buzzes = Claude Code needs you.</p>
  </section>
</template>

<style scoped>
.ill { padding: 14px 16px; border-radius: 12px; background: var(--v-bg); border: 1px solid var(--v-line); }
.ill-top { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
.ill-name { font-weight: 600; }
.ill-text { margin: 8px 0 0; color: var(--v-muted); font-size: 14px; }
.ill-nums { display: flex; gap: 18px; margin-top: 8px; font-size: 12px; color: var(--v-faint); }
.alerts { display: grid; gap: 12px; margin-top: 16px; }
.none { color: var(--v-muted); font-size: 14px; }
.alert { display: grid; grid-template-columns: 10px 1fr; gap: 12px; }
.dot { width: 8px; height: 8px; border-radius: 50%; margin-top: 6px; background: var(--v-move); }
.dot.illness, .dot.spo2 { background: var(--v-heart); }
.a-top { display: flex; justify-content: space-between; gap: 10px; font-size: 13px; }
.a-top span { color: var(--v-faint); }
.a-text { color: var(--v-muted); font-size: 13px; margin-top: 2px; }
.buzz-key { color: var(--v-faint); font-size: 12px; margin: 16px 0 0; }
</style>
