<script setup lang="ts">
/* Connect Zebra's MCP server to an AI tool: pick the tool, copy one line, paste it in Terminal (or the config). */
import { computed, onMounted, ref } from 'vue';
import { backend, isDesktop } from '../../lib/bridge';

const FALLBACK = '/Applications/Zebra.app/Contents/MacOS/zeppbridge-mcp';
const path = ref(FALLBACK);
onMounted(async () => {
  if (!isDesktop()) return;
  const s = await backend.getMcpSidecar().catch(() => null);
  if (s?.path && s.path.startsWith('/')) path.value = s.path;
});

const q = (p: string) => (/\s/.test(p) ? `"${p}"` : p);
const json = (extra: Record<string, unknown> = {}) =>
  JSON.stringify({ mcpServers: { zebra: { ...extra, command: path.value } } }, null, 2);

const TOOLS = computed(() => [
  { id: 'claude-code', name: 'Claude Code', how: 'Paste in Terminal.', text: `claude mcp add --scope user zebra -- ${q(path.value)}` },
  { id: 'codex', name: 'Codex', how: 'Paste in Terminal.', text: `codex mcp add zebra -- ${q(path.value)}` },
  { id: 'gemini', name: 'Gemini CLI', how: 'Paste in Terminal.', text: `gemini mcp add -s user zebra ${q(path.value)}` },
  { id: 'claude-desktop', name: 'Claude Desktop', how: 'Claude Desktop, Settings, Developer, Edit Config. Add this, then restart Claude.', text: json() },
  { id: 'cursor', name: 'Cursor', how: 'Cursor, Settings, MCP, Add new server. Or put this in ~/.cursor/mcp.json.', text: json() },
  { id: 'other', name: 'Other apps', how: 'Any MCP client (Windsurf, LM Studio, Zed, VS Code...). Add this to its MCP config.', text: json({ type: 'stdio' }) },
]);
const pick = ref('claude-code');
const current = computed(() => TOOLS.value.find((t) => t.id === pick.value) ?? TOOLS.value[0]);
const copied = ref(false);
const copy = async () => {
  try { await navigator.clipboard.writeText(current.value.text); copied.value = true; window.setTimeout(() => (copied.value = false), 1600); } catch { /* clipboard blocked */ }
};
const ASK = [
  'How did I sleep this week compared with last week?',
  'What was my heart rate between 2 and 4 pm today?',
  'Is my recovery off? Check HRV and resting heart rate against my baseline.',
  'Summarise my workouts this month.',
];
</script>

<template>
  <div class="mcp">
    <p class="lead">Let Claude, ChatGPT's Codex, Gemini and other AI tools read your health data. Zebra's MCP server runs on this Mac, reads your local database, and needs no key.</p>

    <div class="v4-tabs tools" role="group" aria-label="AI tool">
      <button v-for="t in TOOLS" :key="t.id" type="button" :aria-pressed="pick === t.id" @click="pick = t.id">{{ t.name }}</button>
    </div>

    <div class="box">
      <div class="box-head">
        <span class="how">{{ current.how }}</span>
        <button type="button" class="glass-btn copy" @click="copy">{{ copied ? 'Copied' : 'Copy' }}</button>
      </div>
      <pre><code>{{ current.text }}</code></pre>
    </div>

    <div class="ask">
      <div class="ask-title">Then ask things like</div>
      <ul><li v-for="a in ASK" :key="a">"{{ a }}"</li></ul>
    </div>
    <p class="note">29 tools: sleep, heart rate, HRV, recovery, workouts, baselines, and your strap's every-second heart rate and health alerts. Health data is read-only.</p>
  </div>
</template>

<style scoped>
.mcp { display: grid; gap: 14px; max-width: 720px; }
.lead { margin: 0; color: var(--v-muted); font-size: 13.5px; line-height: 1.5; }
.tools { flex-wrap: wrap; justify-self: start; }
.box { background: var(--v-surface); border-radius: 14px; overflow: hidden; }
.box-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 10px 12px 10px 16px; border-bottom: 1px solid var(--v-line); }
.how { color: var(--v-muted); font-size: 12.5px; }
.copy { height: 28px; padding: 0 14px; font-size: 12.5px; flex: none; }
pre { margin: 0; padding: 14px 16px; overflow-x: auto; font: 12.5px/1.55 ui-monospace, SFMono-Regular, Menlo, monospace; color: var(--v-text); white-space: pre; }
pre code { background: none; padding: 0; color: inherit; font: inherit; }
.ask { background: var(--v-surface); border-radius: 14px; padding: 12px 16px; }
.ask-title { font-size: 13px; font-weight: 600; margin-bottom: 6px; }
.ask ul { margin: 0; padding-left: 18px; color: var(--v-muted); font-size: 13px; line-height: 1.7; }
.note { margin: 0; color: var(--v-faint); font-size: 12px; }
</style>
