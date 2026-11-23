<script setup lang="ts">
/* The strap's own settings, read live over Bluetooth. Choice settings can be changed here;
   the strap refuses any value it did not offer. */
import { computed, onMounted, ref } from 'vue';
import { backend, isDesktop } from '../../lib/bridge';

type Raw = Record<string, { value: number | null; list: number[] | null; choices: number[] | null }>;
const raw = ref<Raw | null>(null);
const error = ref<string | null>(null);
const busy = ref<string | null>(null);

const load = async () => {
  error.value = null;
  if (!isDesktop()) {
    raw.value = { '08:01': { value: 254, list: null, choices: [0, 254, 1, 5, 10, 30] }, '08:02': { value: 130, list: null, choices: [0, 100, 110, 120, 130, 140, 150] },
      '08:03': { value: 50, list: null, choices: [0, 40, 45, 50] }, '08:31': { value: 1, list: null, choices: null }, '08:32': { value: 80, list: null, choices: [0, 80, 85, 90] },
      '09:05': { value: null, list: [80, 100, 120, 140, 160, 180], choices: null } };
    return;
  }
  try { raw.value = await backend.strapSettings(); } catch (e) { error.value = String((e as { message?: string })?.message ?? e); }
};
onMounted(load);

const label = (key: string, v: number) => {
  if (key === '08:01') return v === 254 ? 'Continuous' : v === 0 ? 'Off' : `Every ${v} min`;
  if (v === 0) return 'Off';
  return key === '08:32' ? `${v} %` : `${v} bpm`;
};
const ROWS = [
  { key: '08:01', name: 'Heart rate measurement', note: 'How often the strap measures your heart rate.' },
  { key: '08:02', name: 'High heart rate alert', note: 'Buzz when you are resting above this.' },
  { key: '08:03', name: 'Low heart rate alert', note: 'Buzz when your heart rate drops below this.' },
  { key: '08:32', name: 'Low blood oxygen alert', note: 'Buzz when blood oxygen drops below this.' },
];
const rows = computed(() => ROWS.map((r) => ({ ...r, s: raw.value?.[r.key] })).filter((r) => r.s && r.s.choices?.length));
const zones = computed(() => raw.value?.['09:05']?.list ?? null);

const change = async (key: string, value: number) => {
  busy.value = key; error.value = null;
  try {
    if (isDesktop()) await backend.strapSet(key, value);
    if (raw.value?.[key]) raw.value[key].value = value;
  } catch (e) { error.value = String((e as { message?: string })?.message ?? e); }
  finally { busy.value = null; }
};
</script>

<template>
  <div class="strap-settings">
    <p v-if="!raw && !error" class="muted">Reading your strap…</p>
    <p v-if="error" class="err">{{ error }} <button type="button" class="v4-btn" @click="load">Try again</button></p>
    <div v-if="rows.length" class="group">
      <div v-for="r in rows" :key="r.key" class="row">
        <div class="text"><div class="name">{{ r.name }}</div><div class="note">{{ r.note }}</div></div>
        <select :value="r.s!.value ?? 0" :disabled="busy === r.key" @change="change(r.key, Number(($event.target as HTMLSelectElement).value))">
          <option v-for="c in [...r.s!.choices!].sort((a, b) => a - b)" :key="c" :value="c">{{ label(r.key, c) }}</option>
        </select>
      </div>
    </div>
    <div v-if="zones" class="group">
      <div class="row">
        <div class="text"><div class="name">Heart rate zones</div><div class="note">Zone limits stored on the strap, in bpm.</div></div>
        <div class="zones">{{ zones.join(' · ') }}</div>
      </div>
    </div>
    <p class="muted foot">Changes go straight to the strap over Bluetooth. The strap only accepts the values listed.</p>
  </div>
</template>

<style scoped>
.group { background: var(--v-surface); border-radius: 14px; overflow: hidden; margin-bottom: 16px; }
.row { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 12px 16px; }
.row + .row { border-top: 1px solid var(--v-line); }
.name { font-weight: 600; font-size: 14px; }
.note, .muted { color: var(--v-muted); font-size: 12.5px; }
.err { color: var(--v-heart); font-size: 13px; }
select { background: var(--v-surface-2); color: var(--v-text); border: 1px solid var(--v-line); border-radius: 8px; padding: 5px 10px; font: inherit; font-size: 13px; }
.zones { font-variant-numeric: tabular-nums; font-weight: 600; font-size: 13px; }
.foot { margin-top: 4px; }
</style>
