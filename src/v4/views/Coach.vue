<script setup lang="ts">
defineOptions({ name: 'Coach' });
import { computed, nextTick, onMounted, ref } from 'vue';
import { backend } from '../data';
import { bodyAge, loadSnapshot, readBirthYear, type Snapshot } from '../health';

interface Turn { role: 'user' | 'assistant'; content: string }

/* Kept at module level so the conversation survives moving between pages. */
const turns = ref<Turn[]>([]);
const snap = ref<Snapshot | null>(null);

const hasKey = ref<boolean | null>(null);
const keyDraft = ref('');
const busy = ref(false);
const error = ref<string | null>(null);
const question = ref('');
const log = ref<HTMLElement | null>(null);

const SYSTEM = [
  'You are Zebra Coach, a sleep, recovery and fitness coach inside a desktop app for an Amazfit Helio Strap.',
  'You get the person\'s numbers as JSON. They were computed in code and are correct: use them, do not recompute or invent numbers.',
  'If a number is null, that data is missing. Say so; never guess it.',
  'Compare the person only with their own history (last 30 days vs the 30 before, and the last 7 days).',
  'Write plain, warm English. Short paragraphs and short bullet lists. No tables. No headings bigger than bold text.',
  'Give 2 to 4 concrete actions when it helps. You are not a doctor: do not diagnose. Suggest a doctor only for clear warning signs.',
].join(' ');

const context = computed(() => {
  if (!snap.value) return null;
  const year = readBirthYear();
  const age = year ? new Date().getFullYear() - year : null;
  const ba = age != null ? bodyAge(age, snap.value.last30) : null;
  return {
    ...snap.value,
    realAge: age,
    bodyAge: ba ? { age: Math.round(ba.age * 10) / 10, deltaYears: Math.round(ba.delta * 10) / 10, factors: ba.factors.map((f) => ({ habit: f.label, value: f.value, years: f.years })) } : null,
  };
});

const send = async (text: string, effort: 'high' | 'max' = 'max') => {
  if (busy.value || !text.trim()) return;
  error.value = null;
  if (!snap.value) snap.value = await loadSnapshot();
  turns.value.push({ role: 'user', content: text.trim() });
  question.value = '';
  busy.value = true;
  await scrollDown();
  try {
    const messages = [
      { role: 'system', content: `${SYSTEM}\n\nThe person's data:\n${JSON.stringify(context.value)}` },
      ...turns.value.slice(-12),
    ];
    const reply = await backend.aiCoachAsk(messages, effort);
    turns.value.push({ role: 'assistant', content: reply.content });
  } catch (e) {
    error.value = e instanceof Error ? e.message : (e as { message?: string })?.message ?? String(e);
    turns.value.pop();
    question.value = text;
  } finally {
    busy.value = false;
    await scrollDown();
  }
};
const scrollDown = async () => { await nextTick(); log.value?.scrollIntoView({ block: 'end', behavior: 'smooth' }); };

const briefing = () => send('Write my briefing for today: how I slept, how my body is doing, and what to do today.');
const suggestions = [
  'Why is my sleep score low?',
  'What is pushing my body age up?',
  'Is my resting heart rate a concern?',
  'Plan a week to improve my sleep.',
];

const saveKey = async () => {
  if (!keyDraft.value.trim()) return;
  hasKey.value = await backend.aiCoachSetKey(keyDraft.value.trim());
  keyDraft.value = '';
};

/* Escape first, then allow **bold** and simple bullet lists. */
const render = (text: string) => {
  const esc = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  const blocks = esc.split(/\n{2,}/).map((block) => {
    const lines = block.split('\n');
    if (lines.every((l) => /^\s*([-*•]|\d+\.)\s+/.test(l))) {
      return `<ul>${lines.map((l) => `<li>${l.replace(/^\s*([-*•]|\d+\.)\s+/, '')}</li>`).join('')}</ul>`;
    }
    return `<p>${lines.join('<br>')}</p>`;
  });
  return blocks.join('').replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>').replace(/^#+\s*/gm, '');
};

onMounted(async () => {
  hasKey.value = await backend.aiCoachKeyStatus().catch(() => false);
  snap.value = await loadSnapshot().catch(() => null);
});
</script>

<template>
  <div class="v4-page coach">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Coach</h1>
        <p class="v4-sub">Ask about your sleep, heart and activity. Answers use your own numbers.</p>
      </div>
      <button v-if="hasKey && turns.length" type="button" class="v4-btn" :disabled="busy" @click="turns = []">New chat</button>
    </header>

    <section v-if="hasKey === false" class="v4-card setup">
      <h2 class="v4-card-title">Add your Sarvam API key</h2>
      <p class="v4-card-note">Coach uses Sarvam's GLM 5.3 model. The key is saved in a private file on this computer and is never shown again.</p>
      <form class="key-form" @submit.prevent="saveKey">
        <input v-model="keyDraft" type="password" placeholder="sk_…" aria-label="Sarvam API key" autocomplete="off" />
        <button type="submit" class="v4-btn primary">Save key</button>
      </form>
    </section>

    <template v-else-if="hasKey">
      <section v-if="!turns.length" class="v4-card start">
        <h2 class="start-title">Start with today's briefing</h2>
        <p class="v4-card-note">Coach reads your last 60 days of sleep, resting heart rate, steps and PAI, then tells you what matters today.</p>
        <button type="button" class="v4-btn primary" :disabled="busy" @click="briefing">Write today's briefing</button>
        <div class="chips">
          <button v-for="s in suggestions" :key="s" type="button" class="chip" :disabled="busy" @click="send(s)">{{ s }}</button>
        </div>
      </section>

      <div class="thread">
        <div v-for="(t, i) in turns" :key="i" :class="['turn', t.role]">
          <div v-if="t.role === 'assistant'" class="avatar" aria-hidden="true">Σ</div>
          <div class="bubble" v-html="t.role === 'assistant' ? render(t.content) : render(t.content)" />
        </div>
        <div v-if="busy" class="turn assistant">
          <div class="avatar" aria-hidden="true">Σ</div>
          <div class="bubble thinking"><span /><span /><span /> Thinking it through. Deep answers can take a minute.</div>
        </div>
        <div ref="log" />
      </div>

      <p v-if="error" class="err" role="alert">{{ error }}</p>

      <form class="ask" @submit.prevent="send(question)">
        <input v-model="question" :disabled="busy" placeholder="Ask Coach anything about your health" aria-label="Question" />
        <button type="submit" class="v4-btn primary" :disabled="busy || !question.trim()">Ask</button>
      </form>
      <p class="privacy">Your numbers (not your name or account) are sent to Sarvam only when you ask. Coach is not a doctor.</p>
    </template>
  </div>
</template>

<style scoped>
.coach { max-width: 860px; }
.setup .v4-card-note, .start .v4-card-note { display: block; margin: 8px 0 18px; }
.key-form, .ask { display: flex; gap: 10px; }
.key-form input, .ask input {
  flex: 1; height: 44px; border-radius: 12px; border: 1px solid var(--v-line-strong); background: var(--v-surface);
  color: var(--v-text); padding: 0 14px; font: 500 15px var(--v-font);
}
.key-form input:focus, .ask input:focus { outline: none; border-color: var(--v-muted); }
.ask .v4-btn { height: 44px; padding: 0 20px; }
.start { margin-bottom: 20px; }
.start-title { font-size: 20px; font-weight: 700; margin: 0; }
.chips { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 18px; }
.chip { border: 1px solid var(--v-line-strong); background: transparent; color: var(--v-muted); border-radius: 999px; padding: 8px 14px; font-size: 13px; font-weight: 600; cursor: pointer; }
.chip:hover:not(:disabled) { color: var(--v-text); border-color: var(--v-muted); }
.thread { display: grid; gap: 18px; margin-bottom: 18px; }
.turn { display: flex; gap: 12px; align-items: flex-start; }
.turn.user { justify-content: flex-end; }
.avatar { flex: none; width: 30px; height: 30px; border-radius: 9px; background: #000; color: #fff; display: grid; place-items: center; font-weight: 700; font-size: 15px; border: 1px solid var(--v-line-strong); }
.bubble { max-width: 640px; font-size: 15px; line-height: 1.6; }
.turn.user .bubble { background: var(--v-surface-2); border-radius: 16px 16px 4px 16px; padding: 10px 16px; }
.turn.assistant .bubble { padding-top: 3px; }
.bubble :deep(p) { margin: 0 0 10px; }
.bubble :deep(p:last-child) { margin-bottom: 0; }
.bubble :deep(ul) { margin: 0 0 10px; padding-left: 20px; }
.bubble :deep(li) { margin: 4px 0; }
.thinking { color: var(--v-muted); display: flex; align-items: center; gap: 4px; }
.thinking span { width: 6px; height: 6px; border-radius: 50%; background: var(--v-muted); animation: pulse 1.2s infinite ease-in-out; }
.thinking span:nth-child(2) { animation-delay: 0.15s; }
.thinking span:nth-child(3) { animation-delay: 0.3s; margin-right: 8px; }
@keyframes pulse { 0%, 100% { opacity: 0.25; } 50% { opacity: 1; } }
.err { color: var(--v-heart); font-size: 14px; }
.privacy { color: var(--v-faint); font-size: 12px; margin-top: 10px; }
</style>
