<script setup lang="ts">
/* The v4 desktop shell: a quiet sidebar and one scrolling page.
   It keeps the wiring the old shell did (sync controller, backend-ready gate, UI scale,
   tray hint) and drops the rest (letters, tour, collection box, page morphs, updater). */
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import { RouterLink, RouterView, useRoute } from 'vue-router';
import { backend, backendLate, isDesktop, whenBackendReady } from '../lib/bridge';
import { useSyncController } from '../composables/useSyncController';
import { useUiScale } from '../composables/useUiScale';
import { setSystemRegion } from '../lib/dateTime';
import { clock } from './data';
import Symbol from './components/Symbol.vue';
import { installLiquidTabs } from './liquidTabs';
import './v4.css';

const route = useRoute();
/* Theme: dark, light, or follow macOS. Stored per computer. */
type ThemePref = 'system' | 'dark' | 'light';
const THEME_KEY = 'zebra.theme';
const readPref = (): ThemePref => { try { const v = window.localStorage.getItem(THEME_KEY); return v === 'dark' || v === 'light' ? v : 'system'; } catch { return 'system'; } };
const themePref = ref<ThemePref>(readPref());
const media = window.matchMedia('(prefers-color-scheme: light)');
const systemLight = ref(media.matches);
media.addEventListener?.('change', (e) => { systemLight.value = e.matches; });
const theme = computed(() => (themePref.value === 'system' ? (systemLight.value ? 'light' : 'dark') : themePref.value));
const COLLAPSE_KEY = 'zebra.sidebar';
const collapsed = ref((() => { try { return window.localStorage.getItem(COLLAPSE_KEY) === 'collapsed'; } catch { return false; } })());
const toggleSidebar = () => {
  collapsed.value = !collapsed.value;
  try { window.localStorage.setItem(COLLAPSE_KEY, collapsed.value ? 'collapsed' : 'open'); } catch { /* this session only */ }
};
const cycleTheme = () => {
  themePref.value = themePref.value === 'system' ? 'dark' : themePref.value === 'dark' ? 'light' : 'system';
  try { window.localStorage.setItem(THEME_KEY, themePref.value); } catch { /* this session only */ }
};
watch(theme, (t) => { document.documentElement.dataset.theme = t; }, { immediate: true });
const desktop = isDesktop();
const backendReady = ref(!desktop);
const {
  appStatus, statusError, isSyncing, syncDeferred, syncMessage, canIncrementalSync,
  initialize, dispose, refreshStatus, markDataChanged, runSync,
} = useSyncController();
const { initializeScale, bumpScale, resetScale } = useUiScale();

const nav = [
  { to: '/', label: 'Today', icon: 'today' },
  { to: '/insights', label: 'Insights', icon: 'insights' },
  { to: '/live', label: 'Live', icon: 'live' },
  { to: '/recovery', label: 'Recovery', icon: 'recovery' },
  { to: '/sleep', label: 'Sleep', icon: 'sleep' },
  { to: '/heart', label: 'Heart', icon: 'heart' },
  { to: '/activity', label: 'Activity', icon: 'activity' },
  { to: '/workouts', label: 'Workouts', icon: 'workouts' },
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
  const shellEl = document.querySelector<HTMLElement>('.v4.shell');
  if (shellEl) disposeTabs = installLiquidTabs(shellEl);
  // Pull data straight from the band on start and every hour, then run the health watch (it buzzes on new alerts).
  if (desktop) {
    const strapSync = () => { void backend.strapSyncNow().then(() => markDataChanged()).catch(() => undefined); };
    window.setTimeout(strapSync, 15000);
    strapTimer = window.setInterval(strapSync, 3600 * 1000);
  }
});
let strapTimer = 0;
let disposeTabs: (() => void) | null = null;
onUnmounted(() => {
  disposeTabs?.();
  window.clearInterval(strapTimer);
  document.removeEventListener('keydown', onKey);
  dispose();
});
</script>

<template>
  <div :class="['v4', 'shell', { collapsed }]" :data-theme="theme">
    <aside class="side">
      <div class="brand">
        <svg class="brand-mark" viewBox="60 220 1120 760" width="30" height="20" aria-hidden="true"><g fill="#c6f05c"><path d="M426 242H632L270 955H78Z" /><path d="M684 242H893L530 955H323Z" /><path d="M946 242H1160L797 955H584Z" /></g></svg>
        <span>Zebra</span>
        <button type="button" class="collapse" :aria-label="collapsed ? 'Expand sidebar' : 'Collapse sidebar'" :title="collapsed ? 'Expand' : 'Collapse'" @click="toggleSidebar">
          <Symbol name="sidebar" :size="17" />
        </button>
      </div>
      <nav class="nav" aria-label="Main">
        <RouterLink v-for="item in nav" :key="item.to" :to="item.to" :class="['nav-item', { active: isActive(item.to) }]" :aria-current="isActive(item.to) ? 'page' : undefined">
          <Symbol :name="item.icon" :size="18" class="nav-icon" />
          <span>{{ item.label }}</span>
        </RouterLink>
      </nav>
      <div class="side-foot">
        <RouterLink to="/settings" :class="['nav-item', { active: isActive('/settings') }]">
          <Symbol name="settings" :size="18" class="nav-icon" />
          <span>Settings</span>
        </RouterLink>
        <button type="button" class="theme" :title="`Theme: ${themePref}`" @click="cycleTheme">
          <Symbol name="theme" :size="16" class="nav-icon" />
          <span>{{ themePref === 'system' ? 'System theme' : themePref === 'dark' ? 'Dark' : 'Light' }}</span>
        </button>
        <button type="button" class="sync" :disabled="!canIncrementalSync || isSyncing" @click="sync">
          <Symbol name="sync" :size="16" :class="['nav-icon', { spin: isSyncing }]" />
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
.shell { position: fixed; inset: 0; display: grid; grid-template-columns: 232px minmax(0, 1fr); transition: grid-template-columns 0.2s ease; }
.side {
  display: flex; flex-direction: column; gap: 20px; padding: 16px 10px 12px; min-width: 0;
  background: var(--v-sidebar); border-right: 1px solid var(--v-line);
}
.collapse { margin-left: auto; display: grid; place-items: center; width: 28px; height: 28px; border-radius: 8px; border: 0; background: transparent; color: var(--v-faint); cursor: pointer; }
.collapse:hover { color: var(--v-text); background: var(--v-surface-2); }
.collapse svg { fill: none; stroke: currentColor; stroke-width: 1.4; }
.shell.collapsed { grid-template-columns: 68px minmax(0, 1fr); }
.shell.collapsed .brand > span, .shell.collapsed .nav-item span, .shell.collapsed .sync span, .shell.collapsed .theme span:not(.theme-dot) { display: none; }
.shell.collapsed .brand { flex-direction: column; gap: 10px; padding: 0; }
.shell.collapsed .collapse { margin-left: 0; }
.shell.collapsed .nav-item, .shell.collapsed .sync, .shell.collapsed .theme { justify-content: center; padding: 0; }
.theme { display: flex; align-items: center; gap: 10px; height: 34px; padding: 0 12px; border-radius: 10px; border: 0; background: transparent; color: var(--v-muted); font-size: 12.5px; font-weight: 600; cursor: pointer; text-align: left; }
.theme:hover { color: var(--v-text); background: var(--v-surface-2); }
.theme-dot { width: 14px; height: 14px; border-radius: 50%; border: 1.5px solid currentColor; background: linear-gradient(90deg, currentColor 50%, transparent 50%); }
.brand { display: flex; align-items: center; gap: 10px; padding: 0 10px; font-weight: 700; font-size: 16px; letter-spacing: -0.01em; }
.brand { font-size: 18px; letter-spacing: -0.02em; }
.nav { display: grid; gap: 1px; }
.nav-item {
  display: flex; align-items: center; gap: 10px; height: 32px; padding: 0 10px; border-radius: 8px;
  color: var(--v-text); font-weight: 500; font-size: 13.5px; letter-spacing: -0.01em;
}
.nav-item:hover { background: var(--v-surface-2); }
.nav-item.active { color: #fff; background: var(--v-accent); }
.nav-item .nav-icon { color: var(--v-accent); }
.nav-item.active .nav-icon { color: #fff; }

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
