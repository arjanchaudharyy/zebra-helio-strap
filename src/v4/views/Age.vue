<script setup lang="ts">
defineOptions({ name: 'BodyAge' });
import { computed, ref } from 'vue';
import AiInsight from '../components/AiInsight.vue';
import { useReload } from '../data';
import DitherOrb from '../components/DitherOrb.vue';
import { birthYearRef, bodyAge, loadSnapshot, saveBirthYear, type Snapshot } from '../health';

const snap = ref<Snapshot | null>(null);
const { loading } = useReload(async () => { snap.value = await loadSnapshot(); });

const birthYear = birthYearRef;
const draftYear = ref('');
const saveYear = () => {
  const y = Number(draftYear.value);
  if (Number.isInteger(y) && y > 1900 && y <= new Date().getFullYear() - 5) saveBirthYear(y);
};
const realAge = computed(() => (birthYear.value == null ? null : new Date().getFullYear() - birthYear.value));

const now = computed(() => (snap.value && realAge.value != null ? bodyAge(realAge.value, snap.value.last30) : null));
const before = computed(() => (snap.value && realAge.value != null ? bodyAge(realAge.value, snap.value.prev30) : null));
const change = computed(() => (now.value && before.value && before.value.counted >= 3 ? now.value.delta - before.value.delta : null));
const enough = computed(() => (now.value?.counted ?? 0) >= 3);
const fmt = (v: number) => (Math.round(v * 10) / 10).toFixed(1);
const signed = (v: number) => `${v > 0 ? '+' : v < 0 ? '−' : ''}${fmt(Math.abs(v))}`;

/* The scale under the number: −6 (younger) to +6 (older) years. */
const SCALE = 6;
const markerPct = computed(() => (now.value ? ((Math.max(-SCALE, Math.min(SCALE, now.value.delta)) + SCALE) / (2 * SCALE)) * 100 : 50));
const ticks = Array.from({ length: 49 }, (_, i) => i);
const maxAbs = computed(() => Math.max(2, ...(now.value?.factors.map((f) => Math.abs(f.years ?? 0)) ?? [0])));
</script>

<template>
  <div class="v4-page">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Body age</h1>
        <p class="v4-sub">How old your habits make your body look, from the last 30 days</p>
      </div>
    </header>

    <section v-if="birthYear == null" class="v4-card ask">
      <h2 class="v4-card-title">What year were you born?</h2>
      <p class="v4-card-note">Body age starts from your real age. Zepp did not sync a birth date, so enter it once. It stays on this computer.</p>
      <form class="year-form" @submit.prevent="saveYear">
        <input v-model="draftYear" inputmode="numeric" maxlength="4" placeholder="2008" aria-label="Birth year" />
        <button type="submit" class="v4-btn primary">Save</button>
      </form>
    </section>

    <template v-else-if="snap && now">
      <section class="v4-card hero">
        <div class="hero-main">
          <div class="orb-wrap">
            <DitherOrb :size="300" :dot="4" class="orb" />
            <div class="orb-text">
              <div class="v4-num age">{{ enough ? fmt(now.age) : '–' }}</div>
              <div class="age-label">Body age</div>
            </div>
          </div>
          <div v-if="enough" :class="['verdict', now.delta > 0.25 ? 'older' : now.delta < -0.25 ? 'younger' : '']">
            {{ Math.abs(now.delta) < 0.25 ? `Same as your real age, ${realAge}` : `${fmt(Math.abs(now.delta))} years ${now.delta > 0 ? 'older' : 'younger'} than your real age, ${realAge}` }}
          </div>
          <div v-else class="verdict">Not enough data yet. Wear your band for 14 days.</div>
        </div>

        <div v-if="enough" class="scale" aria-hidden="true">
          <div class="scale-ticks">
            <span v-for="i in ticks" :key="i" :class="{ major: i % 8 === 0, mid: i === 24 }" />
          </div>
          <div class="scale-marker" :style="{ left: `${markerPct}%` }" />
          <div class="scale-labels"><span>6 years younger</span><span>Real age</span><span>6 years older</span></div>
        </div>

        <div class="hero-stats">
          <div>
            <div class="v4-stat-label">Change vs the month before</div>
            <div class="v4-stat-value">{{ change == null ? '–' : signed(change) }} <span class="v4-unit">years</span></div>
          </div>
          <div>
            <div class="v4-stat-label">Habits counted</div>
            <div class="v4-stat-value">{{ now.counted }} <span class="v4-unit">of {{ now.factors.length }}</span></div>
          </div>
          <div>
            <div class="v4-stat-label">Nights used</div>
            <div class="v4-stat-value">{{ snap.last30.nightsWithData }} <span class="v4-unit">of 30</span></div>
          </div>
        </div>
      </section>

<AiInsight topic="age" class="ai-block" />
      <section class="v4-card">
        <div class="v4-card-head">
          <h2 class="v4-card-title">What changes your body age</h2>
          <span class="v4-card-note">Years added or taken away</span>
        </div>
        <div class="factors">
          <div v-for="f in now.factors" :key="f.key" class="factor">
            <div class="f-text">
              <div class="f-label">{{ f.label }}</div>
              <div class="f-value">{{ f.value }}<template v-if="f.years != null && f.years > 0"> · {{ f.advice }}</template></div>
            </div>
            <div class="f-bar" aria-hidden="true">
              <span class="axis" />
              <span v-if="f.years" :class="['fill', f.years > 0 ? 'up' : 'down']" :style="{ width: `${(Math.abs(f.years) / maxAbs) * 50}%` }" />
            </div>
            <div :class="['f-years', f.years == null ? 'none' : f.years > 0 ? 'up' : f.years < 0 ? 'down' : '']">
              {{ f.years == null ? 'Not enough data' : f.years === 0 ? '0' : `${signed(f.years)} y` }}
            </div>
          </div>
        </div>
        <p class="hint">
          This is an estimate from your own band data, not a medical test. Each habit adds or removes a fixed number of years.
          HRV, VO₂max and strength are not counted, because your band has not synced them yet.
          <button type="button" class="linkish" @click="saveBirthYear(null)">Change birth year</button>
        </p>
      </section>
    </template>

    <div v-else-if="!loading" class="v4-card v4-empty"><strong>No data yet</strong>Sync your band first.</div>
  </div>
</template>

<style scoped>
.ai-block { margin-bottom: 16px; }
.ask { max-width: 560px; }
.ask .v4-card-note { display: block; margin: 8px 0 16px; }
.year-form { display: flex; gap: 10px; }
.year-form input {
  width: 120px; height: 40px; border-radius: 10px; border: 1px solid var(--v-line-strong); background: var(--v-bg);
  color: var(--v-text); padding: 0 12px; font: 600 18px var(--v-num); letter-spacing: 0.04em;
}
.hero { margin-bottom: 16px; padding: 36px 32px 28px; }
.hero-main { text-align: center; }
.orb-wrap { position: relative; width: 300px; height: 300px; margin: 0 auto; display: grid; place-items: center; }
.orb { position: absolute; inset: 0; opacity: 0.55; }
.orb-text { position: relative; text-align: center; text-shadow: 0 2px 24px rgba(0, 0, 0, 0.9), 0 0 2px #000; }
.age { font-size: 104px; letter-spacing: -0.01em; }
.age-label { color: var(--v-text); font-weight: 700; margin-top: 2px; }
.verdict { margin-top: 10px; font-size: 16px; color: var(--v-muted); }
.verdict.older { color: var(--v-move); }
.verdict.younger { color: #4fd18b; }
.scale { position: relative; max-width: 560px; margin: 32px auto 8px; }
.scale-ticks { display: flex; justify-content: space-between; align-items: flex-end; height: 22px; }
.scale-ticks span { width: 2px; height: 10px; background: var(--v-line-strong); border-radius: 1px; }
.scale-ticks span.major { height: 16px; background: var(--v-faint); }
.scale-ticks span.mid { height: 22px; background: var(--v-muted); }
.scale-marker { position: absolute; top: -6px; width: 4px; height: 34px; border-radius: 2px; background: var(--v-text); transform: translateX(-50%); box-shadow: 0 0 0 3px var(--v-surface); }
.scale-labels { display: flex; justify-content: space-between; color: var(--v-faint); font-size: 12px; margin-top: 8px; }
.hero-stats { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 16px; margin-top: 28px; padding-top: 22px; border-top: 1px solid var(--v-line); }
.factors { display: grid; }
.factor { display: grid; grid-template-columns: minmax(0, 1.4fr) minmax(120px, 1fr) 110px; gap: 16px; align-items: center; padding: 14px 0; border-top: 1px solid var(--v-line); }
.factor:first-child { border-top: 0; }
.f-label { font-weight: 600; }
.f-value { color: var(--v-muted); font-size: 13px; }
.f-bar { position: relative; height: 10px; }
.f-bar .axis { position: absolute; left: 50%; top: -4px; bottom: -4px; width: 1px; background: var(--v-line-strong); }
.f-bar .fill { position: absolute; top: 0; bottom: 0; border-radius: 5px; }
.f-bar .fill.up { left: 50%; background: var(--v-move); }
.f-bar .fill.down { right: 50%; background: #4fd18b; }
.f-years { text-align: right; font-family: var(--v-num); font-weight: 600; font-size: 20px; }
.f-years.up { color: var(--v-move); }
.f-years.down { color: #4fd18b; }
.f-years.none { font-family: var(--v-font); font-size: 12px; color: var(--v-faint); font-weight: 500; }
.hint { color: var(--v-muted); font-size: 13px; margin: 16px 0 0; }
.linkish { border: 0; background: none; color: var(--v-text); text-decoration: underline; cursor: pointer; padding: 0; font-size: 13px; }
@media (max-width: 760px) { .factor { grid-template-columns: 1fr 90px; } .f-bar { display: none; } .hero-stats { grid-template-columns: 1fr; } }
</style>
