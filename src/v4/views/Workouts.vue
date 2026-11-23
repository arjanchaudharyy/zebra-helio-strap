<script setup lang="ts">
/* Workouts: every workout from your band, newest first, with the numbers Zepp records. */
defineOptions({ name: 'WorkoutList' });
import { computed, ref } from 'vue';
import { RouterLink } from 'vue-router';
import Symbol from '../components/Symbol.vue';
import { backend, clock, duration, shortDay, useReload } from '../data';
import type { Workout } from '../../types';

const all = ref<Workout[]>([]);
const { loading } = useReload(async () => {
  const out: Workout[] = [];
  for (let offset = 0; offset < 2000; offset += 200) {
    const page = await backend.getWorkoutPage(200, offset);
    out.push(...page.items);
    if (page.items.length < 200) break;
  }
  all.value = out;
});

const pretty = (t: string) => (/^unknown/i.test(t) ? 'Other' : t.replace(/[_-]+/g, ' ').replace(/\b\w/g, (c) => c.toUpperCase()));
const minutes = (w: Workout) => (new Date(w.end_time).getTime() - new Date(w.start_time).getTime()) / 60000;
const filter = ref<string>('all');
const types = computed(() => {
  const counts = new Map<string, number>();
  for (const w of all.value) counts.set(w.effective_type, (counts.get(w.effective_type) ?? 0) + 1);
  return [...counts.entries()].sort((a, b) => b[1] - a[1]);
});
const shown = computed(() => (filter.value === 'all' ? all.value : all.value.filter((w) => w.effective_type === filter.value)));
const last30 = computed(() => {
  const cut = Date.now() - 30 * 86400000;
  const ws = all.value.filter((w) => new Date(w.start_time).getTime() >= cut);
  return {
    count: ws.length,
    minutes: ws.reduce((s, w) => s + minutes(w), 0),
    km: ws.reduce((s, w) => s + (w.distance_meters ?? 0), 0) / 1000,
    load: ws.reduce((s, w) => s + (w.training_load ?? 0), 0),
    hr: (() => { const h = ws.filter((w) => w.avg_hr); return h.length ? h.reduce((s, w) => s + (w.avg_hr ?? 0), 0) / h.length : null; })(),
  };
});
const visible = ref(60);
</script>

<template>
  <div class="v4-page">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Workouts</h1>
        <p class="v4-sub">{{ all.length }} workouts from your band</p>
      </div>
    </header>

    <div v-if="!loading && !all.length" class="v4-card v4-empty"><strong>No workouts yet</strong>Start a workout from the Zepp app and sync.</div>

    <div v-else class="v4-stack">
      <section class="v4-card">
        <div class="v4-card-head"><h2 class="v4-card-title">Last 30 days</h2></div>
        <div class="v4-stats">
          <div><div class="v4-stat-label">Workouts</div><div class="v4-stat-value">{{ last30.count }}</div></div>
          <div><div class="v4-stat-label">Time</div><div class="v4-stat-value">{{ duration(last30.minutes) || '0 min' }}</div></div>
          <div v-if="last30.km > 0.05"><div class="v4-stat-label">Distance</div><div class="v4-stat-value">{{ last30.km.toFixed(1) }}<span class="v4-unit">km</span></div></div>
          <div v-else><div class="v4-stat-label">Average heart rate</div><div class="v4-stat-value">{{ last30.hr ? Math.round(last30.hr) : '–' }}<span v-if="last30.hr" class="v4-unit">bpm</span></div></div>
          <div><div class="v4-stat-label">Training load</div><div class="v4-stat-value">{{ Math.round(last30.load) }}</div></div>
        </div>
      </section>

      <div v-if="types.length > 1" class="v4-tabs filters" role="group" aria-label="Type">
        <button type="button" :aria-pressed="filter === 'all'" @click="filter = 'all'">All</button>
        <button v-for="[t, n] in types.slice(0, 6)" :key="t" type="button" :aria-pressed="filter === t" @click="filter = t">{{ pretty(t) }} · {{ n }}</button>
      </div>

      <section class="v4-card list">
        <RouterLink v-for="w in shown.slice(0, visible)" :key="w.workout_id" :to="`/workouts/${encodeURIComponent(w.workout_id)}`" class="row">
          <span class="icon"><Symbol name="workouts" :size="18" /></span>
          <div class="main">
            <div class="name">{{ w.custom_label || pretty(w.effective_type) }}</div>
            <div class="when">{{ shortDay(w.start_time.slice(0, 10)) }}, {{ clock(w.start_time) }}</div>
          </div>
          <div class="nums">
            <div><span class="k">Time</span><span class="v">{{ duration(minutes(w)) }}</span></div>
            <div v-if="w.distance_meters"><span class="k">Distance</span><span class="v">{{ (w.distance_meters / 1000).toFixed(2) }} km</span></div>
            <div v-if="w.avg_hr"><span class="k">Avg HR</span><span class="v">{{ w.avg_hr }} bpm</span></div>
            <div v-if="w.calories"><span class="k">Calories</span><span class="v">{{ Math.round(w.calories) }}</span></div>
            <div v-if="w.training_load"><span class="k">Load</span><span class="v">{{ Math.round(w.training_load) }}</span></div>
          </div>
        </RouterLink>
        <button v-if="shown.length > visible" type="button" class="v4-btn more" @click="visible += 60">Show more</button>
      </section>
    </div>
  </div>
</template>

<style scoped>
.filters { flex-wrap: wrap; align-self: start; }
.list { padding: 6px 8px; }
.row { display: grid; grid-template-columns: 36px minmax(0, 1fr) auto; align-items: center; gap: 12px; padding: 12px 10px; border-radius: 12px; }
.row + .row { border-top: 1px solid var(--v-line); }
.row:hover { background: var(--v-surface-2); }
.icon { width: 36px; height: 36px; border-radius: 50%; display: grid; place-items: center; background: rgba(255, 159, 10, 0.18); color: var(--v-move); }
.name { font-weight: 600; letter-spacing: -0.01em; }
.when { color: var(--v-muted); font-size: 12.5px; }
.nums { display: flex; gap: 20px; }
.nums > div { display: grid; text-align: right; }
.k { color: var(--v-faint); font-size: 11px; }
.v { font-weight: 600; font-size: 13.5px; font-variant-numeric: tabular-nums; }
.more { margin: 10px auto 6px; display: flex; }
@media (max-width: 900px) { .nums > div:nth-child(n + 3) { display: none; } }
</style>
