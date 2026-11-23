<script setup lang="ts">
/* The strap's own settings, read live over Bluetooth. Choice settings can be changed here;
   the strap refuses any value it did not offer. */
import { computed, onMounted, ref } from 'vue';
import { backend, isDesktop } from '../../lib/bridge';
import { alarms, loadAlarms, saveAlarms } from '../wakeAlarm';

type Raw = Record<string, { value: number | null; list: number[] | null; choices: number[] | null }>;
const raw = ref<Raw | null>(null);
const error = ref<string | null>(null);
const busy = ref<string | null>(null);

const friendly = (e: unknown) => {
  const m = String((e as { message?: string })?.message ?? e);
  return /timeout|not found|no device|bluetooth/i.test(m) ? 'Could not reach your strap. Keep it close to this Mac and try again.' : m;
};
const load = async () => {
  error.value = null;
  if (!isDesktop()) {
    raw.value = { '08:01': { value: 254, list: null, choices: [0, 254, 1, 5, 10, 30] }, '08:02': { value: 130, list: null, choices: [0, 100, 110, 120, 130, 140, 150] },
      '08:03': { value: 50, list: null, choices: [0, 40, 45, 50] }, '08:31': { value: 1, list: null, choices: null }, '08:32': { value: 80, list: null, choices: [0, 80, 85, 90] },
      '09:05': { value: null, list: [80, 100, 120, 140, 160, 180], choices: null } };
    return;
  }
  try { raw.value = await backend.strapSettings(); } catch {
    // The strap can be busy for a few seconds after another session; try once more.
    await new Promise((r) => window.setTimeout(r, 4000));
    try { raw.value = await backend.strapSettings(); } catch (e) { error.value = friendly(e); }
  }
};
onMounted(load);

/* Short labels for a segmented control; the unit sits once beside the control. */
const label = (key: string, v: number) => {
  if (key === '08:01') return v === 254 ? 'Always' : v === 0 ? 'Off' : `${v} min`;
  return v === 0 ? 'Off' : String(v);
};
const unit = (key: string) => (key === '08:01' ? '' : key === '08:32' ? '%' : 'bpm');
const ordered = (key: string, cs: readonly number[]) => {
  const xs = [...cs].sort((a, b) => a - b);
  return key === '08:01' ? xs : [0, ...xs.filter((x) => x !== 0)].filter((x) => cs.includes(x));
};
const ZONE_COLORS = ['#64d2ff', '#30d158', '#ffd60a', '#ff9f0a', '#ff375f'];
const ZONE_NAMES = ['Warm-up', 'Fat burn', 'Aerobic', 'Anaerobic', 'Max'];
const ROWS = [
  { key: '08:01', name: 'Heart rate measurement', note: 'How often the strap measures your heart rate.' },
  { key: '08:02', name: 'High heart rate alert', note: 'Buzz when you are resting above this.' },
  { key: '08:03', name: 'Low heart rate alert', note: 'Buzz when your heart rate drops below this.' },
  { key: '08:32', name: 'Low blood oxygen alert', note: 'Buzz when blood oxygen drops below this.' },
];
const rows = computed(() => ROWS.map((r) => ({ ...r, s: raw.value?.[r.key] })).filter((r) => r.s && r.s.choices?.length));
const zones = computed(() => raw.value?.['09:06']?.list ?? raw.value?.['09:05']?.list ?? null);
const maxHr = computed(() => zones.value?.[zones.value.length - 1] ?? null);

const field = <K extends 'on' | 'time' | 'bedOn' | 'bedTime'>(k: K) =>
  computed({ get: () => alarms.value[k], set: (v) => { if (v !== '') void saveAlarms({ [k]: v }); } });
const alarmOn = field('on'); const alarmTime = field('time'); const bedOn = field('bedOn'); const bedTime = field('bedTime');
onMounted(loadAlarms);
const buzzed = ref<string | null>(null);
const buzz = async (pattern: 'find' | 'breathe') => {
  busy.value = pattern; error.value = null;
  try {
    if (isDesktop()) await backend.strapBuzz(pattern);
    buzzed.value = pattern; window.setTimeout(() => (buzzed.value = null), 3000);
  } catch (e) { error.value = friendly(e); }
  finally { busy.value = null; }
};
const change = async (key: string, value: number) => {
  busy.value = key; error.value = null;
  try {
    if (isDesktop()) await backend.strapSet(key, value);
    if (raw.value?.[key]) raw.value[key].value = value;
  } catch (e) { error.value = friendly(e); }
  finally { busy.value = null; }
};
</script>

<template>
  <div class="strap-settings">
    <div class="group actions">
      <div class="act">
        <div class="text"><div class="name">Find my strap</div><div class="note">It buzzes four times so you can feel or hear where it is.</div></div>
        <button type="button" class="glass-btn" :disabled="busy != null" @click="buzz('find')">{{ busy === 'find' ? 'Buzzing...' : buzzed === 'find' ? 'Done' : 'Buzz' }}</button>
      </div>
      <div class="act">
        <div class="text"><div class="name">Wake-up alarm</div><div class="note">A silent alarm on your wrist. The strap buzzes at this time every day while Zebra is running and your Mac is awake.</div></div>
        <div class="alarm">
          <input v-model="alarmTime" type="time" class="time" aria-label="Alarm time" :disabled="!alarmOn" />
          <div class="v4-tabs seg" role="group" aria-label="Wake-up alarm">
            <button type="button" :aria-pressed="!alarmOn" @click="alarmOn = false">Off</button>
            <button type="button" :aria-pressed="alarmOn" @click="alarmOn = true">On</button>
          </div>
        </div>
      </div>
      <div class="act">
        <div class="text"><div class="name">Bedtime reminder</div><div class="note">Three gentle buzzes to tell you it is time to wind down.</div></div>
        <div class="alarm">
          <input v-model="bedTime" type="time" class="time" aria-label="Bedtime" :disabled="!bedOn" />
          <div class="v4-tabs seg" role="group" aria-label="Bedtime reminder">
            <button type="button" :aria-pressed="!bedOn" @click="bedOn = false">Off</button>
            <button type="button" :aria-pressed="bedOn" @click="bedOn = true">On</button>
          </div>
        </div>
      </div>
      <div class="act">
        <div class="text"><div class="name">Breathe with your strap</div><div class="note">Three slow breaths: breathe in while it buzzes, out while it is still. About 25 seconds.</div></div>
        <button type="button" class="glass-btn" :disabled="busy != null" @click="buzz('breathe')">{{ busy === 'breathe' ? 'Breathing...' : buzzed === 'breathe' ? 'Done' : 'Start' }}</button>
      </div>
    </div>
    <p v-if="!raw && !error" class="muted">Reading your strap over Bluetooth. This takes about half a minute.</p>
    <p v-if="error" class="err">{{ error }} <button type="button" class="v4-btn" @click="load">Try again</button></p>
    <div v-if="rows.length" class="group">
      <div v-for="r in rows" :key="r.key" class="row">
        <div class="text"><div class="name">{{ r.name }}</div><div class="note">{{ r.note }}</div></div>
        <div class="control">
          <div class="v4-tabs seg" role="group" :aria-label="r.name" :class="{ busy: busy === r.key }">
            <button v-for="c in ordered(r.key, r.s!.choices!)" :key="c" type="button" :aria-pressed="(r.s!.value ?? 0) === c" :disabled="busy != null" @click="change(r.key, c)">{{ label(r.key, c) }}</button>
          </div>
          <span v-if="unit(r.key)" class="unit">{{ unit(r.key) }}</span>
        </div>
      </div>
    </div>
    <div v-if="zones && zones.length >= 6" class="group zones-card">
      <div class="text"><div class="name">Heart rate zones</div><div class="note">Your workout zones in beats a minute: 50 to 100% of your maximum heart rate ({{ maxHr }} bpm).</div></div>
      <div class="zbar">
        <div v-for="(n, i) in ZONE_NAMES" :key="n" class="zone">
          <i :style="{ background: ZONE_COLORS[i] }" />
          <span class="zname">{{ n }}</span>
          <span class="zrange">{{ zones[i] }}–{{ zones[i + 1] }}</span>
        </div>
      </div>
    </div>
    <p v-if="raw" class="muted foot">Changes go straight to the strap over Bluetooth. The strap only accepts the values listed.</p>
  </div>
</template>

<style scoped>
.group { background: var(--v-surface); border-radius: 14px; overflow: hidden; margin-bottom: 16px; }
.row { display: grid; gap: 10px; padding: 14px 16px; }
.row + .row { border-top: 1px solid var(--v-line); }
.name { font-weight: 600; font-size: 14px; }
.note, .muted { color: var(--v-muted); font-size: 12.5px; }
.err { color: var(--v-heart); font-size: 13px; }
.control { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
.seg button { min-width: 44px; padding: 0 12px; font-variant-numeric: tabular-nums; }
.act { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 14px 16px; }
.act + .act { border-top: 1px solid var(--v-line); }
.act .glass-btn { flex: none; height: 30px; padding: 0 16px; font-size: 13px; }
.alarm { display: flex; align-items: center; gap: 10px; flex: none; }
.time { height: 30px; padding: 0 10px; border-radius: 9px; border: 1px solid var(--v-line); background: var(--v-surface-2, transparent); color: var(--v-text); font: 600 13px/1 system-ui; font-variant-numeric: tabular-nums; color-scheme: light dark; }
.time:disabled { opacity: 0.5; }
.seg.busy { opacity: 0.6; }
.unit { color: var(--v-muted); font-size: 13px; font-weight: 600; }
.zones-card { padding: 14px 16px 16px; display: grid; gap: 12px; }
.zbar { display: grid; grid-template-columns: repeat(5, minmax(0, 1fr)); gap: 6px; }
.zone { display: grid; gap: 4px; }
.zone i { display: block; height: 8px; border-radius: 4px; }
.zname { font-size: 12px; font-weight: 600; }
.zrange { font-size: 12px; color: var(--v-muted); font-variant-numeric: tabular-nums; }
.foot { margin-top: 4px; }
</style>
