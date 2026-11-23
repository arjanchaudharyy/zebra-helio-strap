<script setup lang="ts">
/* Pick the AI behind Coach and Zebra AI: a hosted API with your key, or a model on your own machine. */
import { computed, onMounted, ref } from 'vue';
import { backend } from '../../lib/bridge';

const emit = defineEmits<{ saved: [ready: boolean] }>();

const PROVIDERS = [
  { id: 'sarvam', name: 'Sarvam', model: 'deepseekv4.1-flash-1017-interactive', key: true, hint: 'Fast and cheap. Get a key at dashboard.sarvam.ai.' },
  { id: 'openai', name: 'OpenAI', model: 'gpt-4o-mini', key: true, hint: 'An API key from platform.openai.com. A ChatGPT Plus plan does not include API access.' },
  { id: 'anthropic', name: 'Anthropic (Claude)', model: 'claude-haiku-5-5', key: true, hint: 'An API key from console.anthropic.com.' },
  { id: 'openrouter', name: 'OpenRouter', model: 'openrouter/auto', key: true, hint: 'One key for hundreds of models. Type any model name from openrouter.ai/models.' },
  { id: 'gemini', name: 'Google Gemini', model: 'gemini-2.5-flash', key: true, hint: 'An API key from aistudio.google.com.' },
  { id: 'groq', name: 'Groq', model: 'llama-3.3-70b-versatile', key: true, hint: 'Very fast open models. Key from console.groq.com.' },
  { id: 'ollama', name: 'Ollama (on this Mac)', model: 'llama3.2', key: false, hint: 'Free and private. Install Ollama, then run "ollama pull llama3.2".' },
  { id: 'lmstudio', name: 'LM Studio (on this Mac)', model: 'local-model', key: false, hint: 'Free and private. Start the local server in LM Studio.' },
  { id: 'custom', name: 'Other server (OpenAI-compatible)', model: '', key: false, hint: 'Any server that speaks the OpenAI chat API: vLLM, LiteLLM, Together, your own box.' },
] as const;

const provider = ref<string>('sarvam');
const model = ref('');
const baseUrl = ref('');
const key = ref('');
const savedKeys = ref<string[]>([]);
const ready = ref(false);
const busy = ref(false);
const message = ref<string | null>(null);

const current = computed(() => PROVIDERS.find((p) => p.id === provider.value) ?? PROVIDERS[0]);
const hasSavedKey = computed(() => savedKeys.value.includes(provider.value));

onMounted(async () => {
  const s = await backend.aiCoachStatus().catch(() => null);
  if (!s) return;
  provider.value = s.provider; model.value = s.model; baseUrl.value = s.baseUrl; savedKeys.value = s.keys; ready.value = s.ready;
});

const pick = (id: string) => { provider.value = id; model.value = ''; baseUrl.value = ''; key.value = ''; message.value = null; };

const save = async () => {
  busy.value = true; message.value = null;
  try {
    const s = await backend.aiCoachConfigure(provider.value, model.value, baseUrl.value, key.value ? key.value : null);
    savedKeys.value = s.keys; ready.value = s.ready; key.value = '';
    message.value = s.ready ? 'Saved. Coach and Zebra AI now use this.' : current.value.key ? 'Saved. Add a key to turn it on.' : 'Saved. Add the server address to turn it on.';
    emit('saved', s.ready);
  } catch (e) {
    message.value = String((e as { message?: string })?.message ?? e);
  } finally { busy.value = false; }
};
</script>

<template>
  <form class="ai-provider" @submit.prevent="save">
    <div class="field">
      <span class="label">Provider</span>
      <div class="chips">
        <button v-for="p in PROVIDERS" :key="p.id" type="button" :class="['chip', { on: p.id === provider }]" @click="pick(p.id)">
          {{ p.name }}<span v-if="savedKeys.includes(p.id)" class="dot" title="Key saved" />
        </button>
      </div>
      <p class="hint">{{ current.hint }}</p>
    </div>

    <label v-if="current.key" class="field">
      <span class="label">API key</span>
      <input v-model="key" type="password" autocomplete="off" :placeholder="hasSavedKey ? 'Saved. Type a new key to replace it.' : 'Paste your key'" />
    </label>

    <label class="field">
      <span class="label">Model</span>
      <input v-model="model" type="text" autocomplete="off" :placeholder="current.model || 'Model name'" />
    </label>

    <label v-if="current.id === 'custom' || current.id === 'ollama' || current.id === 'lmstudio'" class="field">
      <span class="label">Server address</span>
      <input v-model="baseUrl" type="url" autocomplete="off" :placeholder="current.id === 'ollama' ? 'http://localhost:11434/v1 (default)' : current.id === 'lmstudio' ? 'http://localhost:1234/v1 (default)' : 'https://your-server/v1'" />
    </label>
    <label v-if="current.id === 'custom'" class="field">
      <span class="label">API key (if the server needs one)</span>
      <input v-model="key" type="password" autocomplete="off" :placeholder="hasSavedKey ? 'Saved' : 'Optional'" />
    </label>

    <div class="actions">
      <button type="submit" class="v4-btn primary" :disabled="busy">{{ busy ? 'Saving…' : 'Save' }}</button>
      <span v-if="ready && !message" class="ok">In use</span>
      <span v-if="message" class="msg">{{ message }}</span>
    </div>
    <p class="hint">Keys are saved in a private file on this computer. Only your health numbers go to the provider, and only when you ask.</p>
  </form>
</template>

<style scoped>
.ai-provider { display: grid; gap: 16px; max-width: 640px; }
.field { display: grid; gap: 6px; }
.label { font-size: 13px; font-weight: 600; }
.chips { display: flex; flex-wrap: wrap; gap: 6px; }
.chip { height: 30px; padding: 0 12px; border-radius: 999px; border: 1px solid var(--v-line); background: var(--v-surface); color: var(--v-text); font: inherit; font-size: 13px; cursor: pointer; display: inline-flex; align-items: center; gap: 6px; }
.chip.on { background: var(--v-accent); border-color: var(--v-accent); color: #fff; }
.dot { width: 6px; height: 6px; border-radius: 50%; background: var(--v-good); }
.chip.on .dot { background: #fff; }
input { height: 34px; border-radius: 8px; border: 1px solid var(--v-line); background: var(--v-surface); color: var(--v-text); padding: 0 12px; font: inherit; font-size: 13px; }
.hint { margin: 0; color: var(--v-muted); font-size: 12.5px; }
.actions { display: flex; align-items: center; gap: 12px; }
.ok { color: var(--v-good); font-size: 13px; font-weight: 600; }
.msg { color: var(--v-muted); font-size: 13px; }
</style>
