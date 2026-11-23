<script setup lang="ts">
/* v4 Settings: one flat page with a section list on the left.
   The section bodies are the existing, working components (connect, sync, backup, MCP);
   only the frame is new. Language, theme, beta lab and the guided tour are gone. */
defineOptions({ name: 'Settings' });
import { computed, onMounted, onUnmounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import CardBody from '../../views/settings/CardBody.vue';
import Symbol from '../components/Symbol.vue';
import StrapSettings from '../components/StrapSettings.vue';
import AiProvider from '../components/AiProvider.vue';
import McpConnect from '../components/McpConnect.vue';
import { provideSettingsContext } from '../../composables/settings/context';
import { useDevices } from '../../composables/useDevices';
import { useSyncController } from '../../composables/useSyncController';

const SECTIONS = [
  { id: 'account', label: 'Account and band', note: 'Your Zepp sign-in and your Helio Strap', icon: 'heart' },
  { id: 'strap', label: 'Strap', note: 'Settings stored on your Helio Strap, changed over Bluetooth', icon: 'live' },
  { id: 'sync', label: 'Sync', note: 'How often Zebra fetches new data', icon: 'sync' },
  { id: 'archive', label: 'Storage and history', note: 'Older history, backups and disk use', icon: 'activity' },
  { id: 'data', label: 'Data streams', note: 'Every kind of data and whether it arrived', icon: 'today' },
  { id: 'ai', label: 'AI and MCP', note: 'Use your data in Claude, Codex, Gemini and more, and choose the AI for Coach', icon: 'coach' },
  { id: 'privacy', label: 'Privacy', note: 'What leaves this computer, and when', icon: 'age' },
  { id: 'advanced', label: 'Advanced', note: 'Maintenance and repair', icon: 'settings' },
] as const;
type SectionId = (typeof SECTIONS)[number]['id'];

const route = useRoute();
const router = useRouter();
const active = computed<SectionId>(() => {
  const card = String(route.params.card ?? '');
  return (SECTIONS.find((s) => s.id === card)?.id ?? 'account');
});
const current = computed(() => SECTIONS.find((s) => s.id === active.value)!);
const open = (id: SectionId) => { void router.replace(`/settings/${id}`); };

const { statusError, refreshStatus } = useSyncController();
const { auth, official, prefs, capability, feedback } = provideSettingsContext();
const { loginError } = auth;
const { dataMessage, dataError } = feedback;
const { load: loadDevices } = useDevices();

let disposed = false;
onMounted(async () => {
  void capability.loadCapabilityOverview();
  void loadDevices();
  await prefs.load();
  if (disposed) return;
  await Promise.all([auth.attach(), official.attach()]);
});
onUnmounted(() => {
  disposed = true;
  auth.detach();
  official.detach();
});
</script>

<template>
  <div class="v4-page settings">
    <header class="v4-head">
      <div>
        <h1 class="v4-title">Settings</h1>
      </div>
    </header>

    <div v-if="statusError" class="msg error" role="alert">
      {{ statusError }} <button type="button" class="v4-btn" @click="() => refreshStatus()">Retry</button>
    </div>
    <div v-if="loginError" class="msg error" role="alert">{{ loginError }}</div>
    <div v-if="dataError" class="msg error" role="alert">{{ dataError }}</div>
    <div v-else-if="dataMessage" class="msg" role="status">{{ dataMessage }}</div>

    <div class="layout">
      <nav class="sections" aria-label="Settings sections">
        <button
          v-for="s in SECTIONS"
          :key="s.id"
          type="button"
          :class="['section', { active: s.id === active }]"
          :aria-current="s.id === active ? 'page' : undefined"
          @click="open(s.id)"
        >
          <Symbol :name="s.icon" :size="16" class="s-icon" />
          <span class="s-label">{{ s.label }}</span>
        </button>
      </nav>

      <section class="body" :aria-label="current.label">
        <header class="body-head">
          <h2 class="body-title">{{ current.label }}</h2>
          <p class="body-note">{{ current.note }}</p>
        </header>
        <StrapSettings v-if="active === 'strap'" />
        <template v-else-if="active === 'ai'">
          <h3 class="sub-title">Connect your AI tools (MCP)</h3>
          <McpConnect />
          <h3 class="sub-title spaced">AI for Coach and Zebra AI</h3>
          <AiProvider />
        </template>
        <KeepAlive v-else>
          <CardBody :id="(active as Exclude<SectionId, 'strap' | 'ai'>)" :key="active" />
        </KeepAlive>
      </section>
    </div>
  </div>
</template>

<style src="../../views/settings/settings-base.css"></style>
<style scoped>
.sub-title { font-size: 15px; font-weight: 700; margin: 8px 0 12px; }
.sub-title.spaced { margin-top: 32px; }
.sub-title + * { margin-bottom: 8px; }
.layout { display: grid; grid-template-columns: 210px minmax(0, 1fr); gap: 36px; align-items: start; }
.sections { display: grid; gap: 2px; position: sticky; top: 24px; }
.section {
  display: flex; align-items: center; gap: 10px; height: 36px; text-align: left; padding: 0 10px; border-radius: 9px;
  border: 0; background: transparent; cursor: pointer; color: var(--v-muted); font-weight: 600; font-size: 13.5px;
}
.section:hover { background: var(--v-surface); color: var(--v-text); }
.section.active { background: var(--v-accent); color: #fff; }
.s-icon { color: var(--v-accent); }
.section.active .s-icon { color: #fff; }
.body { min-width: 0; max-width: 760px; }
.body-head { margin-bottom: 22px; }
.body-title { font-size: 22px; font-weight: 700; letter-spacing: -0.01em; margin: 0; }
.body-note { color: var(--v-muted); margin: 4px 0 0; font-size: 14px; }
.msg { margin-bottom: 16px; padding: 12px 16px; border-radius: 12px; background: var(--v-surface); border: 1px solid var(--v-line); color: var(--v-muted); display: flex; gap: 12px; align-items: center; justify-content: space-between; }
.msg.error { color: var(--v-heart); }
@media (max-width: 900px) {
  .layout { grid-template-columns: minmax(0, 1fr); }
  .sections { position: static; grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
</style>
