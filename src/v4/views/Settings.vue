<script setup lang="ts">
/* v4 Settings: one flat page with a section list on the left.
   The section bodies are the existing, working components (connect, sync, backup, MCP);
   only the frame is new. Language, theme, beta lab and the guided tour are gone. */
defineOptions({ name: 'Settings' });
import { computed, onMounted, onUnmounted } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import CardBody from '../../views/settings/CardBody.vue';
import { provideSettingsContext } from '../../composables/settings/context';
import { useDevices } from '../../composables/useDevices';
import { useSyncController } from '../../composables/useSyncController';

const SECTIONS = [
  { id: 'account', label: 'Account and band', note: 'Connect Zepp, see your band' },
  { id: 'sync', label: 'Sync', note: 'Automatic sync and history' },
  { id: 'archive', label: 'Storage and backup', note: 'Backups, restore, disk use' },
  { id: 'data', label: 'What is synced', note: 'Every data stream and its state' },
  { id: 'ai', label: 'AI and MCP', note: 'Connect Claude and other AI tools' },
  { id: 'privacy', label: 'Privacy', note: 'What leaves this computer' },
  { id: 'advanced', label: 'Advanced', note: 'Maintenance and repair' },
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
          <span class="s-label">{{ s.label }}</span>
          <span class="s-note">{{ s.note }}</span>
        </button>
      </nav>

      <section class="v4-card body" :aria-label="current.label">
        <h2 class="body-title">{{ current.label }}</h2>
        <KeepAlive>
          <CardBody :id="active" :key="active" />
        </KeepAlive>
      </section>
    </div>
  </div>
</template>

<style src="../../views/settings/settings-base.css"></style>
<style scoped>
.layout { display: grid; grid-template-columns: 220px minmax(0, 1fr); gap: 20px; align-items: start; }
.sections { display: grid; gap: 2px; position: sticky; top: 24px; }
.section {
  display: grid; gap: 2px; text-align: left; padding: 10px 12px; border-radius: 10px;
  border: 0; background: transparent; cursor: pointer;
}
.section:hover { background: var(--v-surface); }
.section.active { background: var(--v-surface-2); }
.s-label { font-weight: 600; font-size: 14px; }
.s-note { color: var(--v-muted); font-size: 12px; }
.body { padding: 24px 28px; }
.body-title { font-size: 18px; font-weight: 700; margin: 0 0 16px; }
.msg { margin-bottom: 16px; padding: 12px 16px; border-radius: 12px; background: var(--v-surface); border: 1px solid var(--v-line); color: var(--v-muted); display: flex; gap: 12px; align-items: center; justify-content: space-between; }
.msg.error { color: var(--v-heart); }
@media (max-width: 900px) {
  .layout { grid-template-columns: minmax(0, 1fr); }
  .sections { position: static; grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
</style>
