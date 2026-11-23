<script setup lang="ts">
defineOptions({ name: 'Coach' });
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import { backend } from '../data';
import { bodyAge, loadSnapshot, readBirthYear, type Snapshot } from '../health';

interface Turn { role: 'user' | 'assistant'; content: string; at: string }
interface Chat { id: string; title: string; updated: string; turns: Turn[] }

/* Chats are saved on this computer, so they come back after a restart. */
const STORE = 'zebra.coach.chats';
const loadChats = (): Chat[] => {
  try { const v = JSON.parse(window.localStorage.getItem(STORE) ?? '[]'); return Array.isArray(v) ? v : []; } catch { return []; }
};
const chats = ref<Chat[]>(loadChats());
const activeId = ref<string | null>(chats.value[0]?.id ?? null);
watch(chats, (v) => { try { window.localStorage.setItem(STORE, JSON.stringify(v.slice(0, 100))); } catch { /* not saved */ } }, { deep: true });
const active = computed(() => chats.value.find((c) => c.id === activeId.value) ?? null);

const snap = ref<Snapshot | null>(null);
const hasKey = ref<boolean | null>(null);
const keyDraft = ref('');
const busy = ref(false);
const error = ref<string | null>(null);
const question = ref('');
const end = ref<HTMLElement | null>(null);
const input = ref<HTMLTextAreaElement | null>(null);

const SYSTEM = [
  'You are Zebra Coach, a sleep, recovery and fitness coach for a person with an Amazfit Helio Strap.',
  'Their numbers are in the JSON below. They are correct: use them as given. Null means missing.',
  'Answer in 2 to 4 short sentences, in plain everyday English. One idea per sentence.',
  'Only use a list if the person asks for a plan; then at most 4 bullets of one line each.',
  'Never use em dashes. No headings. No filler. Compare only with their own history. Do not diagnose.',
].join(' ');

const context = computed(() => {
  if (!snap.value) return null;
  const year = readBirthYear();
  const age = year ? new Date().getFullYear() - year : null;
  const ba = age != null ? bodyAge(age, snap.value.last30) : null;
  return { ...snap.value, realAge: age, bodyAge: ba ? { age: Math.round(ba.age * 10) / 10, deltaYears: Math.round(ba.delta * 10) / 10, factors: ba.factors.map((f) => ({ habit: f.label, value: f.value, years: f.years })) } : null };
});

const newChat = () => { activeId.value = null; question.value = ''; error.value = null; void nextTick(() => input.value?.focus()); };
const removeChat = (id: string) => { chats.value = chats.value.filter((c) => c.id !== id); if (activeId.value === id) activeId.value = null; };

const send = async (text: string) => {
  const q = text.trim();
  if (busy.value || !q) return;
  error.value = null;
  if (!snap.value) snap.value = await loadSnapshot().catch(() => null);
  let chat = active.value;
  if (!chat) {
    const id = `${Date.now()}`;
    chats.value.unshift({ id, title: q.length > 48 ? `${q.slice(0, 46)}…` : q, updated: new Date().toISOString(), turns: [] });
    activeId.value = id;
    // Use the reactive copy from the list, so the reply shows up on screen.
    chat = chats.value.find((c) => c.id === id)!;
  }
  chat.turns.push({ role: 'user', content: q, at: new Date().toISOString() });
  question.value = '';
  busy.value = true;
  await scrollDown();
  try {
    const messages = [
      { role: 'system', content: `${SYSTEM}\n\nData:\n${JSON.stringify(context.value)}` },
      ...chat.turns.slice(-12).map((t) => ({ role: t.role, content: t.content })),
    ];
    const reply = await backend.aiCoachAsk(messages);
    chat.turns.push({ role: 'assistant', content: reply.content, at: new Date().toISOString() });
    chat.updated = new Date().toISOString();
  } catch (e) {
    error.value = (e as { message?: string })?.message ?? String(e);
    chat.turns.pop();
    question.value = q;
  } finally {
    busy.value = false;
    await scrollDown();
  }
};
const scrollDown = async () => { await nextTick(); end.value?.scrollIntoView({ block: 'end', behavior: 'smooth' }); };
/* Edit: put your last question back in the box and remove it and its answer, so you can change and resend it. */
const editLast = () => {
  const chat = active.value;
  if (!chat || busy.value) return;
  const i = chat.turns.map((t) => t.role).lastIndexOf('user');
  if (i < 0) return;
  question.value = chat.turns[i].content;
  chat.turns.splice(i);
  void nextTick(() => input.value?.focus());
};
const onKey = (e: KeyboardEvent) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); void send(question.value); } };

const suggestions = [
  'How did I sleep last night?',
  'Why is my sleep score low?',
  'What is pushing my body age up?',
  'Give me a plan for this week.',
];

const saveKey = async () => {
  if (!keyDraft.value.trim()) return;
  hasKey.value = await backend.aiCoachSetKey(keyDraft.value.trim());
  keyDraft.value = '';
};

const render = (text: string) => {
  const esc = text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  return esc.split(/\n{2,}/).map((block) => {
    const lines = block.split('\n');
    if (lines.every((l) => /^\s*([-*•]|\d+\.)\s+/.test(l))) return `<ul>${lines.map((l) => `<li>${l.replace(/^\s*([-*•]|\d+\.)\s+/, '')}</li>`).join('')}</ul>`;
    return `<p>${lines.join('<br>')}</p>`;
  }).join('').replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>').replace(/^#+\s*/gm, '');
};
const when = (iso: string) => {
  const d = new Date(iso);
  const days = Math.floor((Date.now() - d.getTime()) / 86400000);
  return days === 0 ? 'Today' : days === 1 ? 'Yesterday' : d.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });
};

onMounted(async () => {
  hasKey.value = await backend.aiCoachKeyStatus().catch(() => false);
  snap.value = await loadSnapshot().catch(() => null);
  await scrollDown();
});
</script>

<template>
  <div class="coach">
    <aside class="history" aria-label="Past chats">
      <button type="button" class="new glass-btn" @click="newChat">
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M8 3v10M3 8h10" /></svg>
        New chat
      </button>
      <div v-if="chats.length" class="h-list">
        <div v-for="c in chats" :key="c.id" :class="['h-item', { on: c.id === activeId }]">
          <button type="button" class="h-open" @click="activeId = c.id; scrollDown()">
            <span class="h-title">{{ c.title }}</span>
            <span class="h-when">{{ when(c.updated) }}</span>
          </button>
          <button type="button" class="h-del" :aria-label="`Delete ${c.title}`" @click="removeChat(c.id)">×</button>
        </div>
      </div>
      <p v-else class="h-empty">Your chats are saved here.</p>
    </aside>

    <section class="main">
      <div v-if="hasKey === false" class="center">
        <h1 class="hero-title">Add your Sarvam API key</h1>
        <p class="hero-sub">Coach runs on DeepSeek V4.1 Flash through Sarvam. The key is saved in a private file on this computer.</p>
        <form class="key-form" @submit.prevent="saveKey">
          <input v-model="keyDraft" type="password" placeholder="sk_…" aria-label="Sarvam API key" autocomplete="off" />
          <button type="submit" class="v4-btn primary">Save key</button>
        </form>
      </div>

      <template v-else>
        <div class="thread">
          <div v-if="!active || !active.turns.length" class="turn assistant tail greet">
            <div class="bubble">Hi! I know your sleep, heart, recovery and activity. Ask me anything about them.</div>
          </div>
          <template v-else>
            <div v-for="(t, i) in active.turns" :key="i" :class="['turn', t.role, { tail: i === active.turns.length - 1 || active.turns[i + 1]?.role !== t.role }]">
              <div class="bubble" v-html="render(t.content)" />
              <button v-if="t.role === 'user' && i === active.turns.map((x) => x.role).lastIndexOf('user') && !busy" type="button" class="edit" @click="editLast">Edit</button>
            </div>
          </template>
          <div v-if="busy" class="turn assistant tail"><div class="bubble typing"><span /><span /><span /></div></div>
          <p v-if="error" class="err" role="alert">{{ error }}</p>
          <div ref="end" />
        </div>

        <div v-if="!active || !active.turns.length" class="quick">
          <button v-for="q in suggestions" :key="q" type="button" class="glass-btn qchip" :disabled="busy" @click="send(q)">{{ q }}</button>
        </div>
        <form class="composer" @submit.prevent="send(question)">
          <textarea ref="input" v-model="question" rows="1" placeholder="Ask Coach anything" aria-label="Question" @keydown="onKey" />
          <button type="submit" class="send glass-btn primary" :disabled="busy || !question.trim()" aria-label="Send">
            <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M8 13V3M3.5 7.5 8 3l4.5 4.5" /></svg>
          </button>
        </form>
        <p class="note">Your numbers, not your name or account, go to Sarvam only when you ask. Coach is not a doctor.</p>
      </template>
    </section>
  </div>
</template>

<style scoped>
.coach { display: grid; grid-template-columns: 240px minmax(0, 1fr); height: 100%; min-height: 100vh; }
.history { border-right: 1px solid var(--v-line); padding: 20px 12px; display: flex; flex-direction: column; gap: 14px; position: sticky; top: 0; height: 100vh; overflow-y: auto; }
.new { width: 100%; height: 36px; }
.new svg, .send svg { fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
.h-list { display: grid; gap: 2px; }
.h-item { display: flex; align-items: center; border-radius: 10px; }
.h-item:hover, .h-item.on { background: var(--v-surface-2); }
.h-open { flex: 1; min-width: 0; display: grid; text-align: left; gap: 2px; padding: 8px 10px; background: none; border: 0; cursor: pointer; }
.h-title { font-size: 13px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.h-when { font-size: 11px; color: var(--v-faint); }
.h-del { opacity: 0; border: 0; background: none; color: var(--v-muted); font-size: 18px; width: 28px; cursor: pointer; }
.h-item:hover .h-del { opacity: 1; }
.h-empty { color: var(--v-faint); font-size: 12px; padding: 0 8px; }
.main { display: flex; flex-direction: column; min-width: 0; padding: 0 40px; max-width: 900px; width: 100%; margin: 0 auto; }
.thread { flex: 1; display: flex; flex-direction: column; gap: 14px; padding: 28px 0 16px; }
.center { margin: auto 0; padding: 60px 0 20px; text-align: center; display: grid; justify-items: center; }
.mark { width: 64px; height: 64px; border-radius: 18px; background: var(--v-surface-2); border: 1px solid var(--v-line-strong); color: #c6f05c; display: grid; place-items: center; margin-bottom: 18px; }
.hero-title { font-size: 26px; font-weight: 700; letter-spacing: -0.02em; margin: 0; }
.hero-sub { color: var(--v-muted); margin: 8px 0 22px; }
.chips { display: grid; grid-template-columns: repeat(2, minmax(0, 260px)); gap: 8px; }
.chip { text-align: left; border: 1px solid var(--v-line); background: var(--v-surface); color: var(--v-text); border-radius: 12px; padding: 12px 14px; font-size: 13px; font-weight: 600; cursor: pointer; }
.chip:hover:not(:disabled) { border-color: var(--v-line-strong); background: var(--v-surface-2); }
/* iMessage bubbles: you on the right in blue, Zebra on the left in grey. */
.turn { display: flex; flex-direction: column; max-width: 100%; }
.turn.user { align-items: flex-end; }
.turn.assistant { align-items: flex-start; }
.turn + .turn { margin-top: -10px; }
.turn.user + .turn.assistant, .turn.assistant + .turn.user { margin-top: 2px; }
.bubble { position: relative; max-width: min(560px, 78%); font-size: 15px; line-height: 1.4; padding: 8px 14px; border-radius: 19px; overflow-wrap: anywhere; }
.turn.user .bubble { background: #0a84ff; color: #fff; }
.turn.assistant .bubble { background: #262628; color: var(--v-text); }
.v4[data-theme='light'] .turn.assistant .bubble { background: #e9e9eb; color: #000; }
.v4[data-theme='light'] .turn.user .bubble { background: #007aff; }
.turn.tail.user .bubble { border-bottom-right-radius: 5px; }
.turn.tail.assistant .bubble { border-bottom-left-radius: 5px; }
.bubble :deep(p) { margin: 0 0 6px; }
.bubble :deep(p:last-child) { margin: 0; }
.bubble :deep(ul) { margin: 2px 0; padding-left: 18px; }
.bubble :deep(li) { margin: 2px 0; }
.edit { border: 0; background: none; color: var(--v-faint); font-size: 11px; font-weight: 600; cursor: pointer; padding: 3px 6px; margin-top: 2px; }
.edit:hover { color: var(--v-accent); }
.typing { display: flex; gap: 5px; padding: 13px 16px !important; }
.typing span { width: 7px; height: 7px; border-radius: 50%; background: var(--v-muted); animation: pulse 1s infinite ease-in-out; }
.typing span:nth-child(2) { animation-delay: 0.15s; }
.typing span:nth-child(3) { animation-delay: 0.3s; }
@keyframes pulse { 0%, 100% { opacity: 0.25; transform: translateY(0); } 50% { opacity: 1; transform: translateY(-2px); } }
.err { color: var(--v-heart); font-size: 13px; }
.composer {
  position: sticky; bottom: 12px; display: flex; align-items: flex-end; gap: 8px; padding: 8px 8px 8px 16px;
  background: var(--v-surface); border: 1px solid var(--v-line-strong); border-radius: 18px;
}
.composer:focus-within { border-color: var(--v-muted); }
.composer textarea { flex: 1; resize: none; border: 0; outline: none; background: none; color: var(--v-text); font: 500 15px/1.5 var(--v-font); padding: 8px 0; max-height: 160px; }
.send { width: 34px; height: 34px; padding: 0; }
.note { color: var(--v-faint); font-size: 11px; text-align: center; margin: 10px 0 16px; }
.key-form { display: flex; gap: 10px; }
.key-form input { width: 320px; height: 42px; border-radius: 12px; border: 1px solid var(--v-line-strong); background: var(--v-surface); color: var(--v-text); padding: 0 14px; }
@media (max-width: 900px) { .coach { grid-template-columns: 1fr; } .history { display: none; } .chips { grid-template-columns: 1fr; } }
</style>

<style scoped>
.greet { margin-top: 8px; }
.quick { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 8px; margin: 0 0 10px; }
.qchip { height: 32px; padding: 0 14px; font-size: 13px; font-weight: 500; }
</style>
