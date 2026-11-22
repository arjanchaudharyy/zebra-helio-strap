<script setup lang="ts">
/* The v4 desktop shell: a quiet sidebar and one scrolling page.
   It keeps the wiring the old shell did (sync controller, backend-ready gate, UI scale,
   tray hint) and drops the rest (letters, tour, collection box, page morphs, updater). */
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { RouterLink, RouterView, useRoute } from 'vue-router';
import { backend, backendLate, isDesktop, whenBackendReady } from '../lib/bridge';
import { useSyncController } from '../composables/useSyncController';
import { useUiScale } from '../composables/useUiScale';
import { setSystemRegion } from '../lib/dateTime';
import { clock } from './data';
import PixelIcon from './components/PixelIcon.vue';
import './v4.css';

const route = useRoute();
const desktop = isDesktop();
const backendReady = ref(!desktop);
const {
  appStatus, statusError, isSyncing, syncDeferred, syncMessage, canIncrementalSync,
  initialize, dispose, refreshStatus, markDataChanged, runSync,
} = useSyncController();
const { initializeScale, bumpScale, resetScale } = useUiScale();

const nav = [
  { to: '/', label: 'Today', icon: 'today' },
  { to: '/sleep', label: 'Sleep', icon: 'sleep' },
  { to: '/heart', label: 'Heart', icon: 'heart' },
  { to: '/activity', label: 'Activity', icon: 'activity' },
  { to: '/age', label: 'Body age', icon: 'age' },
  { to: '/coach', label: 'Coach', icon: 'coach' },
] as const;
const isActive = (to: string) => (to === '/' ? route.path === '/' : route.path === to || route.path.startsWith(`${to}/`));

const lastSync = computed(() => clock(appStatus.value?.last_cloud_sync_at ?? appStatus.value?.last_sync ?? null));
const syncLabel = computed(() => {
  if (isSyncing.value) return 'Syncing…';
  if (syncDeferred.value) return syncMessage.value || 'Waiting to sync';
  return lastSync.value ? `Synced ${lastSync.value}` : 'Not synced yet';
});
const sync = () => { if (canIncrementalSync.value && !isSyncing.value) void runSync('incremental'); };

const onKey = (event: KeyboardEvent) => {
  const target = event.target as HTMLElement | null;
  if (target?.closest('input, textarea, select, [contenteditable]')) return;
  if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
  if (event.key === '=' || event.key === '+') { event.preventDefault(); bumpScale(1); }
  else if (event.key === '-') { event.preventDefault(); bumpScale(-1); }
  else if (event.key === '0') { event.preventDefault(); resetScale(); }
};

onMounted(() => {
  initializeScale();
  document.documentElement.dataset.theme = 'dark';
  if (desktop) {
    void backend.setTrayLocale('en').catch(() => undefined);
    void backend.systemRegionLocale().then(setSystemRegion).catch(() => undefined);
  }
  void whenBackendReady().then(() => {
    backendReady.value = true;
    if (backendLate()) { void refreshStatus(); markDataChanged(); }
  });
  void initialize();
  document.addEventListener('keydown', onKey);
});
onUnmounted(() => {
  document.removeEventListener('keydown', onKey);
  dispose();
});
</script>

<template>
  <div class="v4 shell">
    <aside class="side">
      <div class="brand">
        <svg class="brand-mark" viewBox="60 220 1120 760" width="30" height="20" aria-hidden="true"><g fill="#c6f05c"><path d="M426 242H632L270 955H78Z" /><path d="M684 242H893L530 955H323Z" /><path d="M946 242H1160L797 955H584Z" /></g></svg>
        <span>Zebra</span>
      </div>
      <nav class="nav" aria-label="Main">
        <RouterLink v-for="item in nav" :key="item.to" :to="item.to" :class="['nav-item', { active: isActive(item.to) }]" :aria-current="isActive(item.to) ? 'page' : undefined">
          <PixelIcon :name="item.icon" :size="17" class="nav-icon" />
          <span>{{ item.label }}</span>
        </RouterLink>
      </nav>
      <div class="side-foot">
        <RouterLink to="/settings" :class="['nav-item', { active: isActive('/settings') }]">
          <PixelIcon name="settings" :size="17" class="nav-icon" />
          <span>Settings</span>
        </RouterLink>
        <button type="button" class="sync" :disabled="!canIncrementalSync || isSyncing" @click="sync">
          <PixelIcon name="sync" :size="15" :class="['nav-icon', { spin: isSyncing }]" />
          <span>{{ syncLabel }}</span>
        </button>
      </div>
    </aside>

    <main id="main-content" class="main" tabindex="-1">
      <div v-if="!backendReady" class="banner">Preparing your data…</div>
      <div v-if="statusError" class="banner error" role="alert">{{ statusError }}</div>
      <RouterView v-slot="{ Component }">
        <KeepAlive :include="['Today', 'Coach']">
          <component :is="Component" />
        </KeepAlive>
      </RouterView>
    </main>
  </div>
</template>

<style scoped>
.shell { position: fixed; inset: 0; display: grid; grid-template-columns: 224px minmax(0, 1fr); }
.side { display: flex; flex-direction: column; gap: 24px; padding: 22px 14px 16px; border-right: 1px solid var(--v-line); background: var(--v-bg); }
.brand { display: flex; align-items: center; gap: 10px; padding: 0 10px; font-weight: 700; font-size: 16px; letter-spacing: -0.01em; }
.brand { font-size: 18px; letter-spacing: -0.02em; }
.nav { display: grid; gap: 2px; }
.nav-item {
  display: flex; align-items: center; gap: 12px; height: 40px; padding: 0 12px; border-radius: 10px;
  color: var(--v-muted); font-weight: 600; font-size: 14px;
}
.nav-item:hover { color: var(--v-text); background: var(--v-surface); }
.nav-item.active { color: var(--v-text); background: var(--v-surface-2); box-shadow: inset 3px 0 0 var(--v-brand); }
.nav-item.active .nav-icon { color: var(--v-brand); }
.nav-icon { flex: none; opacity: 0.85; }
.nav-item:hover .nav-icon, .nav-item.active .nav-icon { opacity: 1; }
.side-foot { margin-top: auto; display: grid; gap: 6px; }
.sync {
  display: flex; align-items: center; gap: 10px; height: 38px; padding: 0 12px; border-radius: 10px;
  border: 1px solid var(--v-line); background: transparent; cursor: pointer; color: var(--v-muted); font-size: 13px; font-weight: 600; text-align: left;
}
.sync:hover:not(:disabled) { color: var(--v-text); border-color: var(--v-line-strong); }
.sync:disabled { cursor: default; }
.spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.main { overflow-y: auto; outline: none; }
.banner { margin: 16px 40px 0; padding: 10px 14px; border-radius: 10px; background: var(--v-surface); border: 1px solid var(--v-line); color: var(--v-muted); font-size: 13px; }
.banner.error { color: var(--v-heart); }
@media (max-width: 760px) {
  .shell { grid-template-columns: 64px minmax(0, 1fr); }
  .brand span, .nav-item span, .sync span { display: none; }
}
</style>
