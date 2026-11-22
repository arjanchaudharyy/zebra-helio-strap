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
  { to: '/activity', label: 'Activity', icon: 'move' },
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
        <img src="./icon.png" alt="" width="28" height="28" />
        <span>ZeppBridge</span>
      </div>
      <nav class="nav" aria-label="Main">
        <RouterLink v-for="item in nav" :key="item.to" :to="item.to" :class="['nav-item', { active: isActive(item.to) }]" :aria-current="isActive(item.to) ? 'page' : undefined">
          <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" class="nav-icon">
            <template v-if="item.icon === 'today'"><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" /></template>
            <template v-else-if="item.icon === 'sleep'"><path d="M19.5 14.5A8 8 0 0 1 9.5 4.5a8 8 0 1 0 10 10Z" /></template>
            <template v-else-if="item.icon === 'heart'"><path d="M3.5 12h4l2-4 3 8 2-4h6" /></template>
            <template v-else><path d="M5 19l4-6 4 3 6-9" /><path d="M15 7h4v4" /></template>
          </svg>
          <span>{{ item.label }}</span>
        </RouterLink>
      </nav>
      <div class="side-foot">
        <RouterLink to="/settings" :class="['nav-item', { active: isActive('/settings') }]">
          <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" class="nav-icon"><circle cx="12" cy="12" r="3" /><path d="M12 3v2.5M12 18.5V21M3 12h2.5M18.5 12H21M5.6 5.6l1.8 1.8M16.6 16.6l1.8 1.8M5.6 18.4l1.8-1.8M16.6 7.4l1.8-1.8" /></svg>
          <span>Settings</span>
        </RouterLink>
        <button type="button" class="sync" :disabled="!canIncrementalSync || isSyncing" @click="sync">
          <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" :class="['nav-icon', { spin: isSyncing }]"><path d="M20 12a8 8 0 0 1-14 5.3M4 12a8 8 0 0 1 14-5.3" /><path d="M18 3v4h-4M6 21v-4h4" /></svg>
          <span>{{ syncLabel }}</span>
        </button>
      </div>
    </aside>

    <main id="main-content" class="main" tabindex="-1">
      <div v-if="!backendReady" class="banner">Preparing your data…</div>
      <div v-if="statusError" class="banner error" role="alert">{{ statusError }}</div>
      <RouterView v-slot="{ Component }">
        <KeepAlive :include="['Today']">
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
.brand img { border-radius: 8px; }
.nav { display: grid; gap: 2px; }
.nav-item {
  display: flex; align-items: center; gap: 12px; height: 40px; padding: 0 12px; border-radius: 10px;
  color: var(--v-muted); font-weight: 600; font-size: 14px;
}
.nav-item:hover { color: var(--v-text); background: var(--v-surface); }
.nav-item.active { color: var(--v-text); background: var(--v-surface-2); }
.nav-icon { fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; flex: none; }
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
