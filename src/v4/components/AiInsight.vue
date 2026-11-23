<script setup lang="ts">
/* "Zebra AI" card: Sarvam writes a short read of one area from the person's own numbers.
   Runs only on a click, then the answer is kept for the rest of the day. */
import { computed, onMounted, ref } from 'vue';
import { backend, todayKey } from '../data';
import { bodyAge, loadSnapshot, readBirthYear } from '../health';

const props = defineProps<{ topic: 'today' | 'sleep' | 'heart' | 'activity' | 'age'; title?: string }>();

const PROMPTS: Record<typeof props.topic, string> = {
  today: 'Give my read for today: a bold one-line headline, then the 2 things that matter most today.',
  sleep: 'Read my sleep: a bold headline, then the 2 changes that would raise my sleep score most.',
  heart: 'Read my resting heart rate: a bold headline, what is driving it, and 1 action.',
  activity: 'Read my activity: a bold headline, then one realistic target for this week.',
  age: 'Explain my body age: a bold headline, then the 2 changes that would lower it fastest.',
};
const SYSTEM = [
  'You are Zebra AI inside a personal health app for an Amazfit Helio Strap.',
  'You get the person\'s numbers as JSON. They are correct: use them as given, never recompute or invent numbers. Null means missing: say so.',
  'Compare only with the person\'s own history. Plain, direct English. Start with a one-line headline in bold.',
  'Then at most 3 short sentences or 3 one-line bullets. Under 70 words in total. Never use em dashes. No tables, no headings. Not a doctor: do not diagnose.',
].join(' ');

const key = computed(() => `zebra.ai.${props.topic}.${todayKey()}`);
const text = ref<string | null>(null);
const busy = ref(false);
const error = ref<string | null>(null);
const hasKey = ref(true);

onMounted(async () => {
  try { text.value = window.localStorage.getItem(key.value); } catch { /* no cache */ }
  hasKey.value = await backend.aiCoachKeyStatus().catch(() => false);
});

const run = async () => {
  if (busy.value) return;
  busy.value = true;
  error.value = null;
  try {
    const snap = await loadSnapshot();
    const year = readBirthYear();
    const age = year ? new Date().getFullYear() - year : null;
    const ba = age != null ? bodyAge(age, snap.last30) : null;
    const data = { ...snap, realAge: age, bodyAge: ba ? { age: Math.round(ba.age * 10) / 10, deltaYears: Math.round(ba.delta * 10) / 10, factors: ba.factors.map((f) => ({ habit: f.label, value: f.value, years: f.years })) } : null };
    const reply = await backend.aiCoachAsk([
      { role: 'system', content: `${SYSTEM}\n\nData:\n${JSON.stringify(data)}` },
      { role: 'user', content: PROMPTS[props.topic] },
    ]);
    text.value = reply.content;
    try { window.localStorage.setItem(key.value, reply.content); } catch { /* kept on screen only */ }
  } catch (e) {
    error.value = (e as { message?: string })?.message ?? String(e);
  } finally {
    busy.value = false;
  }
};

const html = computed(() => {
  if (!text.value) return '';
  const esc = text.value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  return esc.split(/\n{2,}/).map((block) => {
    const lines = block.split('\n');
    if (lines.every((l) => /^\s*([-*•]|\d+\.)\s+/.test(l))) return `<ul>${lines.map((l) => `<li>${l.replace(/^\s*([-*•]|\d+\.)\s+/, '')}</li>`).join('')}</ul>`;
    return `<p>${lines.join('<br>')}</p>`;
  }).join('').replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>').replace(/^#+\s*/gm, '');
});
</script>

<template>
  <section class="ai" :aria-label="title ?? 'Zebra AI'">
    <header class="ai-head">
      <span class="ai-badge" aria-hidden="true">
        <svg viewBox="60 220 1120 760" width="16" height="11"><g fill="currentColor"><path d="M426 242H632L270 955H78Z" /><path d="M684 242H893L530 955H323Z" /><path d="M946 242H1160L797 955H584Z" /></g></svg>
      </span>
      <span class="ai-title">{{ title ?? 'Zebra AI' }}</span>
      <span class="ai-meta">{{ text ? 'Written today from your data' : 'Takes about a second' }}</span>
      <button v-if="hasKey" type="button" class="ai-btn glass-btn primary" :disabled="busy" @click="run">{{ busy ? 'Analysing…' : text ? 'Refresh' : 'Analyse' }}</button>
    </header>
    <div v-if="text" class="ai-body" v-html="html" />
    <div v-else-if="busy" class="ai-body thinking"><span /><span /><span /> Reading your numbers…</div>
    <p v-else-if="!hasKey" class="ai-empty">Add a Sarvam API key on the Coach page to turn on Zebra AI.</p>
    <p v-else class="ai-empty">Get a plain-English read of this page, written from your own numbers.</p>
    <p v-if="error" class="ai-err" role="alert">{{ error }}</p>
  </section>
</template>

<style scoped>
.ai {
  border-radius: var(--v-radius-lg); padding: 18px 22px 20px;
  background: var(--v-surface); border: 1px solid var(--v-line);
  position: relative; overflow: hidden;
}
.ai-head { display: flex; align-items: center; gap: 10px; }
.ai-badge { display: grid; place-items: center; width: 26px; height: 26px; border-radius: 8px; background: var(--v-surface-2); color: #a3d13f; }
.ai-title { font-weight: 700; font-size: 14px; }
.ai-meta { color: var(--v-faint); font-size: 12px; }
.ai-btn { margin-left: auto; height: 30px; padding: 0 14px; font-size: 12.5px; }
.ai-btn:disabled { opacity: 0.6; cursor: default; }
.ai-body { margin-top: 12px; font-size: 15px; line-height: 1.6; color: var(--v-text); }
.ai-body :deep(p) { margin: 0 0 8px; }
.ai-body :deep(p:last-child) { margin-bottom: 0; }
.ai-body :deep(strong) { color: var(--v-text); }
.ai-body :deep(ul) { margin: 4px 0 8px; padding-left: 20px; }
.ai-body :deep(li) { margin: 3px 0; }
.ai-empty { margin: 10px 0 0; color: var(--v-muted); font-size: 14px; }
.ai-err { margin: 10px 0 0; color: var(--v-heart); font-size: 13px; }
.thinking { color: var(--v-muted); display: flex; align-items: center; gap: 4px; }
.thinking span { width: 6px; height: 6px; border-radius: 50%; background: var(--v-brand); animation: pulse 1.2s infinite ease-in-out; }
.thinking span:nth-child(2) { animation-delay: 0.15s; }
.thinking span:nth-child(3) { animation-delay: 0.3s; margin-right: 8px; }
@keyframes pulse { 0%, 100% { opacity: 0.25; } 50% { opacity: 1; } }
</style>
