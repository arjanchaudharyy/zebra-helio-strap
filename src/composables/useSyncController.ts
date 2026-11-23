import { computed, readonly, ref } from 'vue';
import { backend, isDesktop } from '../lib/bridge';
import { autoSyncBackoffElapsed, launchSyncIsDue, nextAutoSyncFailures, writeAutoSyncSettings } from '../lib/autoSync';
import type { LoginStatus, OfficialStatus, SyncProgress, SyncReport } from '../types';
import { readyOnPickUp } from '../lib/dataReady';
import { formatClock, formatTime, lastOutcomeLabel, statusErrorText, syncMessage } from './sync/notice';
import {
  applyOfficialStatus, cancelSync, clearRunTimers, isRunningSync, lastSyncAttemptAt, retryDeferredNow, runSync,
} from './sync/run';
import {
  appStatus, applyLoginStatus, autoSyncEnabled, cloudStaleTip, autoSyncInterval, compacting, compactingEvent, compactionPending,
  compactionSaved, copy, dataReady, dataRevision, loginStatus, notice, refreshStatus, streamRevisions, streamUpdate,
  syncProgress, syncReport, syncState,
} from './sync/state';

export type { SyncUiState } from './sync/state';
export { syncOutcomeLabel } from './sync/notice';

const unlisteners: Array<() => void> = [];
let autoSyncTimer: number | null = null;
let removeWakeListeners: (() => void) | null = null;

let autoSyncFailures = 0;

const maybeAutoSync = () => {
  if (!autoSyncEnabled.value || appStatus.value?.connection_state !== 'connected' || isRunningSync()) return;
  if (!autoSyncBackoffElapsed(lastSyncAttemptAt(), autoSyncFailures, autoSyncInterval.value)) return;
  if (!launchSyncIsDue(appStatus.value?.last_cloud_sync_at, autoSyncInterval.value)) return;
  void runSync('incremental', undefined, { silent: true, quick: true }).then((report) => {
    autoSyncFailures = nextAutoSyncFailures(autoSyncFailures, report?.outcome ?? (syncState.value === 'failed' ? 'failed' : null));
  });
};

const shareAutoSyncPrefs = () => {
  if (!isDesktop()) return;
  void Promise.resolve()
    .then(() => backend.setAutoSyncPrefs(autoSyncEnabled.value, autoSyncInterval.value))
    .catch(() => undefined);
};
let initializeEpoch = 0;
let compactionSavedTimer = 0;
let launchSyncTimer = 0;
const LAUNCH_SYNC_DELAY_MS = 4_000;

const setAutoSyncEnabled = (enabled: boolean) => {
  autoSyncEnabled.value = Boolean(enabled);
  writeAutoSyncSettings({ enabled: autoSyncEnabled.value, intervalMinutes: autoSyncInterval.value });
  shareAutoSyncPrefs();
};

const setAutoSyncInterval = (minutes: number) => {
  autoSyncInterval.value = minutes;
  writeAutoSyncSettings({ enabled: autoSyncEnabled.value, intervalMinutes: minutes });
  shareAutoSyncPrefs();
};

const clearHeldResources = () => {
  for (const unlisten of unlisteners.splice(0)) unlisten();
  clearRunTimers();
  window.clearTimeout(compactionSavedTimer);
  compactionSavedTimer = 0;
  window.clearTimeout(launchSyncTimer);
  launchSyncTimer = 0;
  if (autoSyncTimer !== null) {
    window.clearInterval(autoSyncTimer);
    autoSyncTimer = null;
  }
  removeWakeListeners?.();
  removeWakeListeners = null;
};

const dispose = () => {
  initializeEpoch += 1;
  clearHeldResources();
};

const initialize = async () => {
  const myEpoch = ++initializeEpoch;
  clearHeldResources();
  const stillMine = () => myEpoch === initializeEpoch;
  const keepUnlisten = (unlisten: unknown) => {
    if (typeof unlisten !== 'function') return;
    const fn = unlisten as () => void;
    if (!stillMine()) {
      fn();
      return;
    }
    unlisteners.push(fn);
  };
  const safeListen = async <T>(event: string, handler: (payload: T) => void) => {
    try {
      keepUnlisten(await backend.listen<T>(event, handler));
    } catch {
    }
  };
  if (isDesktop()) {
    const [initialLogin] = await Promise.all([
      backend.getLoginStatus().catch(() => null),
      safeListen<SyncProgress>('sync://progress', (payload) => {
        if (payload.completed) {
          if (payload.changed === false) return;
          streamUpdate.value = { stream: payload.stream, revision: streamUpdate.value.revision + 1 };
          streamRevisions.value = { ...streamRevisions.value, [payload.stream]: (streamRevisions.value[payload.stream] ?? 0) + 1 };
          return;
        }
        if (!isRunningSync()) return;
        syncProgress.value = payload;
        notice.value = payload.code
          ? {
            kind: 'progress',
            code: payload.code,
            stream: payload.stream,
            month: payload.detail ?? null,
            text: payload.message,
          }
          : { kind: 'backend', text: payload.message };
      }),
      safeListen('tray://sync', () => {
        void runSync('incremental');
      }),
      safeListen<LoginStatus>('login://status', applyLoginStatus),
      safeListen('replay://finished', () => {
        void refreshStatus({ preserveError: true });
        retryDeferredNow();
      }),
      safeListen<OfficialStatus>('official://status', applyOfficialStatus),
      safeListen<SyncReport>('sync://background-finished', (report) => {
        void refreshStatus({ preserveError: true });
        if (report?.content_changed ?? (report?.total_records ?? 0) > 0) dataRevision.value += 1;
      }),
      safeListen<number>('compaction://started', (pending) => {
        compactionPending.value = typeof pending === 'number' ? pending : 0;
        compactingEvent.value = true;
        compactionSaved.value = null;
      }),
      safeListen<{ bytesBefore: number; bytesAfter: number }>(
        'compaction://finished',
        (report) => {
          compactingEvent.value = false;
          void refreshStatus({ preserveError: true });
          retryDeferredNow();
          const saved = (report?.bytesBefore ?? 0) - (report?.bytesAfter ?? 0);
          compactionSaved.value = saved > 0 ? saved : null;
          window.clearTimeout(compactionSavedTimer);
          compactionSavedTimer = window.setTimeout(() => { compactionSaved.value = null; }, 12_000);
        },
      ),
    ]);
    if (!stillMine()) return;
    if (initialLogin) applyLoginStatus(initialLogin);
    if (!stillMine()) return;
    autoSyncTimer = window.setInterval(maybeAutoSync, 60_000);
    shareAutoSyncPrefs();
    const onWake = () => {
      if (document.visibilityState === 'visible') maybeAutoSync();
    };
    if (typeof document !== 'undefined' && typeof window.addEventListener === 'function') {
      document.addEventListener('visibilitychange', onWake);
      window.addEventListener('focus', onWake);
      removeWakeListeners = () => {
        document.removeEventListener('visibilitychange', onWake);
        window.removeEventListener('focus', onWake);
      };
    }
  }
  let status = await refreshStatus();
  if (!stillMine()) return;
  if (status?.connection_state === 'configured' && isDesktop()) {
    try {
      await backend.verifyAuth();
      if (!stillMine()) return;
      status = await refreshStatus();
    } catch {
      if (!stillMine()) return;
      await refreshStatus();
    }
  }
  if (!stillMine()) return;
  if (autoSyncEnabled.value && status?.connection_state === 'connected'
    && launchSyncIsDue(status?.last_cloud_sync_at, autoSyncInterval.value)) {
    launchSyncTimer = window.setTimeout(() => {
      if (stillMine()) void runSync('incremental', undefined, { silent: true, waited: true });
    }, LAUNCH_SYNC_DELAY_MS);
  }
};

const markDataChanged = () => {
  dataRevision.value += 1;
};

const lastPickupAt = ref(0);

const pickUpReady = (reason: 'pickup' | 'dismiss' = 'pickup') => {
  const wasReady = dataReady.value.phase === 'ready';
  dataReady.value = readyOnPickUp(dataReady.value);
  if (wasReady && reason === 'pickup') lastPickupAt.value = Date.now();
};

export const useSyncController = () => ({
  appStatus: readonly(appStatus),
  statusError: statusErrorText,
  syncState: readonly(syncState),
  syncMessage,
  syncReport: readonly(syncReport),
  cloudStaleText: computed(() => {
    const tip = cloudStaleTip.value;
    if (!tip) return null;
    return tip.latestAt ? copy().cloudStale(formatTime(tip.latestAt)) : copy().cloudStaleNoTime;
  }),
  dismissCloudStale: () => { cloudStaleTip.value = null; },
  syncProgress: readonly(syncProgress),
  loginStatus: readonly(loginStatus),
  dataRevision: readonly(dataRevision),
  dataReady: readonly(dataReady),
  lastPickupAt: readonly(lastPickupAt),
  streamUpdate: readonly(streamUpdate),
  compacting,
  compactionPending: readonly(compactionPending),
  compactionSaved: readonly(compactionSaved),
  autoSyncEnabled: readonly(autoSyncEnabled),
  autoSyncInterval: readonly(autoSyncInterval),
  isSyncing: computed(() => syncState.value === 'syncing'),
  syncDeferred: computed(() => syncState.value === 'deferred'),
  canIncrementalSync: computed(() => appStatus.value?.connection_state === 'connected'),
  lastCloudSyncLabel: computed(() => {
    const clock = formatClock(appStatus.value?.last_cloud_sync_at);
    return clock ? copy().cloudSyncClock(clock) : copy().cloudSyncClockUnknown;
  }),
  lastOutcomeLabel,
  initialize,
  refreshStatus,
  runSync,
  cancelSync,
  setAutoSyncEnabled,
  setAutoSyncInterval,
  markDataChanged,
  pickUpReady,
  dispose,
});
