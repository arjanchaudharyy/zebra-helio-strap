<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import Icon from '../../../components/Icon.vue';
import { backend, isDesktop, toUserMessage } from '../../../lib/bridge';
import type { McpClientId, McpSidecar } from '../../../lib/bridge/types';
import { COMMAND_PRESETS, factoryConfig, MCP_CLIENT_NAMES } from '../../../lib/mcpClients';
import { useMessages } from '../../../i18n';
import { aiCardMessages } from './ai.i18n';

const a = useMessages(aiCardMessages);
const emit = defineEmits<{ sidecar: [McpSidecar | null] }>();
const sidecar = ref<McpSidecar | null>(null);
const note = ref<{ text: string; ok: boolean } | null>(null);
const busy = ref(false);
const names = (clients: McpClientId[] | undefined) => (clients ?? [])
  .map((client) => MCP_CLIENT_NAMES[client])
  .join(', ');
const staleNames = computed(() => names(sidecar.value?.stale_clients));
const scopeStaleNames = computed(() => names(sidecar.value?.scope_stale_clients));

const refresh = async () => {
  if (!isDesktop()) return;
  try {
    sidecar.value = await backend.getMcpSidecar();
  } catch {
    sidecar.value = null;
  }
  emit('sidecar', sidecar.value);
};
onMounted(refresh);
defineExpose({ refresh });

const copyText = async (text: string | null, done: string) => {
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    note.value = { text: done, ok: true };
  } catch (e) {
    note.value = { text: toUserMessage(e, ''), ok: false };
  }
};
const copyCommands = (build: (s: McpSidecar) => string | null) =>
  copyText(sidecar.value ? build(sidecar.value) : null, a.value.connectCopied);
const copyFactory = () =>
  copyText(sidecar.value?.path ? factoryConfig(sidecar.value, '') : null, a.value.factoryCopied);

const saveBundle = async () => {
  busy.value = true;
  note.value = null;
  try {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const directory = await open({ directory: true, title: a.value.bundlePick });
    if (!directory || Array.isArray(directory)) return;
    const saved = await backend.saveMcpBundle(String(directory));
    note.value = { text: a.value.bundleSaved, ok: true };
    const { revealItemInDir } = await import('@tauri-apps/plugin-opener');
    await revealItemInDir(saved).catch(() => undefined);
  } catch (e) {
    note.value = { text: toUserMessage(e, ''), ok: false };
  } finally {
    busy.value = false;
  }
};
</script>

<template>
  <div class="s-row is-block">
    <div class="s-row-main">
      <span class="s-row-title">{{ a.connectTitle }}</span>
      <span class="s-row-sub">{{ sidecar?.path ? a.connectSub : sidecar?.appimage ? a.sidecarAppImage : a.sidecarMissing }}</span>
    </div>
    <div class="s-actions connect-actions">
      <button v-for="preset in COMMAND_PRESETS" :key="preset.id" class="button secondary" type="button"
        :disabled="!sidecar?.path" @click="copyCommands(preset.build)">
        <Icon name="terminal" :size="14" />{{ MCP_CLIENT_NAMES[preset.id] }}
      </button>
      <button class="button secondary" type="button" :disabled="!sidecar?.path || busy" @click="saveBundle">
        <Icon name="export" :size="14" />{{ MCP_CLIENT_NAMES.claude_desktop }}
      </button>
      <button class="button secondary" type="button" :disabled="!sidecar?.path" @click="copyFactory">
        <Icon name="copy" :size="14" />{{ MCP_CLIENT_NAMES.factory }}
      </button>
    </div>
    <p v-if="sidecar?.outdated" class="hint-line stale-line" role="alert">{{ a.sidecarOutdated }}</p>
    <p v-if="staleNames" class="hint-line stale-line" role="status">{{ a.staleClients(staleNames) }}</p>
    <p v-if="scopeStaleNames" class="hint-line stale-line" role="status">{{ a.scopeStale(scopeStaleNames) }}</p>
    <p v-if="note" class="hint-line" :class="{ ok: note.ok }" :role="note.ok ? 'status' : 'alert'">{{ note.text }}</p>
  </div>
</template>

<style scoped src="../settings-local.css"></style>
<style scoped>
.connect-actions { flex-wrap: wrap; margin-top: 10px; }
.stale-line { color: var(--warning); }
</style>
