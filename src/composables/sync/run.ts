import { backend, isDesktop, toUserMessage } from '../../lib/bridge';
import type { OfficialStatus, SyncReport } from '../../types';
import { readyOnReport, readyOnStart } from '../../lib/dataReady';
import { failedStreamKeys, isCancelledSyncError, isDeferredSyncError } from '../../lib/syncDeferred';
import { noticeForReport } from './notice';
import {
  appStatus, cloudStaleTip, copy, dataReady, dataRevision, notice, refreshStatus, statusError, statusErrorFromSync, streamUpdate,
  syncProgress, syncReport, syncState,
} from './state';


/**
 * Come back once the raw-payload replay has had another minute.
 *
 * The replay runs for as long as a quarter of an hour on a large library, and
 * a sync that gave up permanently would leave the user looking at stale data
 * with no way back except restarting the app. `runSync` already refuses to
 * stack, so a retry landing on a running sync is a no-op.
 */
const DEFERRED_RETRY_MS = 60_000;
let deferredRetryTimer = 0;
let pendingRetry: (() => void) | null = null;
let firstRunTimer = 0;
let runningSync: Promise<SyncReport | null> | null = null;
let lastAttemptAt = 0;

const scheduleDeferredRetry = (mode: 'incremental' | 'initial' | 'history', days?: number, quick?: boolean) => {
  window.clearTimeout(deferredRetryTimer);
  const retry = () => {
    pendingRetry = null;
    window.clearTimeout(deferredRetryTimer);
    deferredRetryTimer = 0;
    void runSync(mode, days, { silent: true, quick });
  };
  pendingRetry = retry;
  deferredRetryTimer = window.setTimeout(retry, DEFERRED_RETRY_MS);
};

export const retryDeferredNow = () => {
  pendingRetry?.();
};

const FIRST_RUN_BACKFILL_DAYS = 180;

const scheduleFirstRunBackfill = () => {
  if (appStatus.value?.storage && !appStatus.value.storage.allow_long_history) return;
  window.clearTimeout(firstRunTimer);
  firstRunTimer = window.setTimeout(() => {
    void runSync('history', FIRST_RUN_BACKFILL_DAYS, { silent: true });
  }, 0);
};

const settleReady = (report: SyncReport | null, firstRun = false) => {
  dataReady.value = readyOnReport(dataReady.value, report, report ? failedStreamKeys(report.streams) : [], firstRun);
};

export const runSync = (
  mode: 'incremental' | 'initial' | 'history' = 'incremental',
  days?: number,
  opts?: { silent?: boolean; waited?: boolean; quick?: boolean; skipProbe?: boolean },
): Promise<SyncReport | null> => {
  const waited = opts?.waited ?? !opts?.silent;
  if (runningSync) {
    if (waited) dataReady.value = readyOnStart(dataReady.value, true);
    if (!opts?.silent) {
      notice.value = { kind: 'alreadySyncing' };
      return Promise.resolve(null);
    }
    return runningSync;
  }
  const promise = (async () => {
    if (!isDesktop()) {
      statusError.value = copy().desktopOnly;
      settleReady(null);
      return null;
    }
    const status = appStatus.value ?? await refreshStatus();
    const wasFirstSync = !status?.last_cloud_sync_at;
    if (status?.connection_state === 'needs_reauth') {
      syncState.value = 'failed';
      notice.value = { kind: 'reauthNeeded' };
      settleReady(null);
      return null;
    }
    if (status?.connection_state !== 'connected') {
      syncState.value = 'failed';
      notice.value = status?.connection_state === 'configured'
        ? { kind: 'verifyFirst' }
        : { kind: 'connectFirst' };
      settleReady(null);
      return null;
    }
    dataReady.value = readyOnStart(dataReady.value, waited);
    lastAttemptAt = Date.now();
    cloudStaleTip.value = null;
    syncState.value = 'syncing';
    syncProgress.value = null;
    notice.value = mode === 'incremental'
      ? { kind: 'syncingRecent', days: opts?.quick ? status?.auto_sync_days : undefined }
      : { kind: 'backfilling', days: days ?? status?.history_sync_days ?? 30 };
    statusError.value = null;
    statusErrorFromSync.value = false;
    const streamRevisionAtStart = streamUpdate.value.revision;
    try {
      const report = mode === 'incremental'
        ? await backend.startIncrementalSync(Boolean(opts?.quick), Boolean(opts?.skipProbe))
        : await backend.startHistorySync(days ?? status?.history_sync_days ?? 30);
      syncReport.value = report;
      syncState.value = report.outcome;
      notice.value = noticeForReport(report);
      await refreshStatus();
      settleReady(report, wasFirstSync && mode === 'incremental');
      if (report.content_changed ?? report.total_records > 0) dataRevision.value += 1;
      if (report.outcome === 'cloud_stale' && waited && !opts?.quick) cloudStaleTip.value = { latestAt: report.cloud_latest_at ?? null };
      if (report.outcome === 'deferred') scheduleDeferredRetry(mode, days, opts?.quick);
      else if (
        wasFirstSync
        && mode === 'incremental'
        && report.total_records > 0
        && report.outcome !== 'failed'
        && report.outcome !== 'cancelled'
      ) {
        scheduleFirstRunBackfill();
      }
      return report;
    } catch (error) {
      const wroteSomething = streamUpdate.value.revision !== streamRevisionAtStart;
      if (isDeferredSyncError(error)) {
        syncState.value = 'deferred';
        const code = (error as { code?: string }).code;
        notice.value = {
          kind: 'report',
          outcome: 'deferred',
          failedStreams: [],
          backendCode: code,
        };
        await refreshStatus();
        if (wroteSomething) dataRevision.value += 1;
        scheduleDeferredRetry(mode, days, opts?.quick);
        return null;
      }
      if (isCancelledSyncError(error)) {
        syncState.value = 'cancelled';
        notice.value = { kind: 'report', outcome: 'cancelled', failedStreams: [] };
        await refreshStatus();
        if (wroteSomething) dataRevision.value += 1;
        settleReady(null);
        return null;
      }
      syncState.value = 'failed';
      notice.value = { kind: 'backend', text: toUserMessage(error, copy().syncDidNotFinish) };
      statusErrorFromSync.value = true;
      await refreshStatus({ preserveError: true });
      if (wroteSomething) dataRevision.value += 1;
      settleReady(null);
      return null;
    } finally {
      syncProgress.value = null;
    }
  })();
  runningSync = promise;
  void promise.finally(() => {
    if (runningSync === promise) runningSync = null;
  });
  return promise;
};

export const cancelSync = async () => {
  if (!isDesktop()) return;
  const target = runningSync;
  try {
    await backend.cancelSync();
    if (target && runningSync === target) notice.value = { kind: 'cancelling' };
  } catch (error) {
    statusError.value = toUserMessage(error, copy().cancelFailed);
  }
};

export const lastSyncAttemptAt = () => lastAttemptAt;

let officialState = '';

export const applyOfficialStatus = (next: OfficialStatus) => {
  const previous = officialState;
  officialState = next.state;
  if (previous === next.state || next.state === 'waiting') return;
  void refreshStatus().then((status) => {
    if (
      next.state === 'connected'
      && previous !== ''
      && status?.connection_state === 'connected'
      && !status.last_cloud_sync_at
    ) {
      void runSync('incremental', undefined, { silent: true, waited: true });
    }
  });
};

export const isRunningSync = () => runningSync !== null;

export const clearRunTimers = () => {
  window.clearTimeout(deferredRetryTimer);
  deferredRetryTimer = 0;
  pendingRetry = null;
  window.clearTimeout(firstRunTimer);
  firstRunTimer = 0;
};
