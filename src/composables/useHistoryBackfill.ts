import { computed, onMounted, ref, watch } from 'vue';
import { useSyncController } from './useSyncController';
import { backend, isDesktop, toUserMessage } from '../lib/bridge';
import { isCancelledSyncError, isDeferredSyncError } from '../lib/syncDeferred';
import type { CoverageLedger, FailedChunk, StorageEstimate, UserPrefs } from '../types';
import { syncStreamLabel } from '../lib/syncStreams';
import { messagesOf, useMessages } from '../i18n';
import { failedChunkText } from '../lib/failedChunkText';
import { storageEstimateText, storageStopReasonText } from '../lib/storageEstimateText';
import { formatBytes, localDateString } from '../lib/format';
import {
  ALL_HISTORY_YEARS, backfillStartDate, defaultCustomFrom, earliestBackfillDate, type BackfillChoice,
} from '../lib/backfillRange';
import { createLoadSeq } from '../lib/loadSeq';
import { archiveMessages } from '../components/HistoryArchivePanel.i18n';

const ledger = ref<CoverageLedger | null>(null);
const busy = ref(false);
const error = ref<string | null>(null);
const message = ref<string | null>(null);
const autoContinue = ref(true);
const stopRequested = ref(false);
let loopGeneration = 0;
let backfillCallInFlight = false;
const DATA_CHANGED_EVERY_MS = 30_000;
const remaining = computed(() => {
  const value = ledger.value;
  if (!value) return 0;
  return Math.max(0, value.total_chunks - value.completed_chunks);
});

export const useHistoryBackfill = (
  prefs: () => UserPrefs | null,
  onPrefsChanged: (prefs: UserPrefs) => void,
) => {
  const t = useMessages(archiveMessages);

  const streamLabel = (stream: string): string => syncStreamLabel(stream);


  const { isSyncing, markDataChanged } = useSyncController();

  const estimate = ref<StorageEstimate | null>(null);
  const startChoice = ref<BackfillChoice>('1y');
  const customFrom = ref(defaultCustomFrom(new Date()));
  const customMin = computed(() => earliestBackfillDate(new Date()));
  const customMax = computed(() => localDateString(new Date()));

  const START_CHOICES = computed(() => [
    { value: '1y', label: t.value.range1y },
    { value: '2y', label: t.value.range2y },
    { value: '3y', label: t.value.range3y },
    { value: 'all', label: t.value.rangeAll(ALL_HISTORY_YEARS) },
    { value: 'custom', label: t.value.rangeCustom },
  ]);

  const fromDate = computed(() => backfillStartDate(startChoice.value, new Date(), customFrom.value));

  const requestedDays = computed(() => {
    if (!fromDate.value) return 0;
    const start = Date.parse(fromDate.value);
    if (!Number.isFinite(start)) return 0;
    return Math.max(0, Math.round((Date.now() - start) / 86_400_000));
  });

  const wouldBeCleanedUp = computed(() => {
    const current = prefs();
    return Boolean(current && !current.archive_enabled && requestedDays.value > current.retention_days);
  });

  const measuredStreams = computed(() => estimate.value?.streams.filter((item) => item.measured) ?? []);
  const unmeasuredStreams = computed(() => estimate.value?.streams.filter((item) => !item.measured) ?? []);

  const estimateLoadSeq = createLoadSeq();

  const loadEstimate = async () => {
    if (!isDesktop() || !requestedDays.value) return;
    const seq = estimateLoadSeq.next();
    const days = requestedDays.value;
    let result: StorageEstimate | null;
    try {
      result = await backend.getStorageEstimate(days);
    } catch {
      result = null;
    }
    // `requestedDays` can change again while this request is in flight; an
    // out-of-order response must not overwrite the estimate for whatever
    // range is currently selected with a stale one from an earlier range.
    if (!estimateLoadSeq.isCurrent(seq)) return;
    estimate.value = result;
  };

  const loadLedger = async () => {
    if (!isDesktop()) return;
    try {
      ledger.value = await backend.getCoverageLedger();
    } catch {
      ledger.value = null;
    }
  };

  onMounted(() => {
    void loadLedger();
    void loadEstimate();
  });

  watch(requestedDays, () => { void loadEstimate(); });

  const toggleArchive = async () => {
    const current = prefs();
    if (!current) return;
    const next = !current.archive_enabled;
    if (!next && !window.confirm(t.value.confirmDisableArchive)) return;
    busy.value = true;
    error.value = null;
    message.value = null;
    try {
      const updated = await backend.setUserPrefs(
        current.retention_days,
        current.history_sync_days,
        next,
      );
      onPrefsChanged(updated);
      message.value = next ? t.value.archiveEnabled : t.value.archiveDisabled;
    } catch (cause) {
      error.value = toUserMessage(cause, t.value.archiveSaveFailed);
    } finally {
      busy.value = false;
    }
  };

  const estimateText = computed(() => storageEstimateText(estimate.value));
  const stopReasonText = computed(() => storageStopReasonText(estimate.value));

  const chunkErrorText = (item: FailedChunk): string => failedChunkText(item);

  const sleepBackfill = async (ms: number, gen: number): Promise<boolean> => {
    const step = 200;
    let waited = 0;
    while (waited < ms) {
      if (stopRequested.value || gen !== loopGeneration) return false;
      await new Promise((resolve) => window.setTimeout(resolve, step));
      waited += step;
    }
    return !stopRequested.value && gen === loopGeneration;
  };

  const stopBackfill = async () => {
    stopRequested.value = true;
    loopGeneration += 1;
    if (!isDesktop() || !backfillCallInFlight) return;
    try {
      await backend.cancelSync();
    } catch {
    }
  };

  const runBackfill = async () => {
    if (!fromDate.value) {
      error.value = t.value.pickStartFirst;
      return;
    }
    if (estimate.value?.stop_reason) {
      error.value = stopReasonText.value;
      return;
    }
    if (wouldBeCleanedUp.value) {
      error.value = t.value.outOfRetention;
      return;
    }
    if (busy.value) return;
    const gen = loopGeneration;
    const from = fromDate.value;
    const copy = () => messagesOf(archiveMessages);
    busy.value = true;
    stopRequested.value = false;
    error.value = null;
    message.value = null;
    let unflushed = false;
    let lastFlushAt = Date.now();
    const flushDataChanged = () => {
      unflushed = false;
      lastFlushAt = Date.now();
      markDataChanged();
    };
    try {
      for (;;) {
        if (gen !== loopGeneration || stopRequested.value) {
          message.value = copy().stoppedByUser(remaining.value);
          break;
        }
        const before = remaining.value;
        try {
          backfillCallInFlight = true;
          let next: CoverageLedger;
          try {
            next = await backend.startHistoryBackfill(from);
          } finally {
            backfillCallInFlight = false;
          }
          if (gen !== loopGeneration) {
            message.value = copy().stoppedByUser(remaining.value);
            break;
          }
          ledger.value = next;
          unflushed = true;
          if (Date.now() - lastFlushAt >= DATA_CHANGED_EVERY_MS) flushDataChanged();
        } catch (cause) {
          if (gen !== loopGeneration || stopRequested.value || isCancelledSyncError(cause)) {
            message.value = copy().stoppedByUser(remaining.value);
            break;
          }
          if (isDeferredSyncError(cause) && autoContinue.value) {
            message.value = copy().deferredRetry;
            const keepGoing = await sleepBackfill(15_000, gen);
            if (!keepGoing) {
              message.value = copy().stoppedByUser(remaining.value);
              break;
            }
            message.value = copy().roundProgress(
              ledger.value?.completed_chunks ?? 0,
              ledger.value?.total_chunks ?? 0,
            );
            continue;
          }
          throw cause;
        }

        if (remaining.value <= 0) {
          message.value = copy().allChunksDone;
          break;
        }
        if (!autoContinue.value) {
          message.value = copy().roundDone(remaining.value);
          break;
        }
        if (stopRequested.value || gen !== loopGeneration) {
          message.value = copy().stoppedByUser(remaining.value);
          break;
        }
        if (before > 0 && remaining.value >= before) {
          message.value = copy().stalled(remaining.value);
          break;
        }
        message.value = copy().roundProgress(
          ledger.value?.completed_chunks ?? 0,
          ledger.value?.total_chunks ?? 0,
        );
        await new Promise((resolve) => window.setTimeout(resolve, 0));
      }
    } catch (cause) {
      error.value = toUserMessage(cause, copy().backfillFailed);
      await loadLedger();
    } finally {
      if (unflushed) flushDataChanged();
      busy.value = false;
      stopRequested.value = false;
    }
  };

  const fillFrom = async (month: string) => {
    const from = `${month}-01`;
    startChoice.value = 'custom';
    customFrom.value = from < customMin.value ? customMin.value : from;
    await runBackfill();
  };

  const retryFailed = async () => {
    busy.value = true;
    error.value = null;
    let requeued = false;
    try {
      ledger.value = await backend.retryFailedBackfillChunks();
      message.value = t.value.retryFailedDone;
      requeued = true;
    } catch (cause) {
      error.value = toUserMessage(cause, t.value.retryFailedFailed);
    } finally {
      busy.value = false;
    }
    if (requeued) await runBackfill();
  };

  const resetLedger = async () => {
    if (!window.confirm(t.value.confirmResetLedger)) return;
    busy.value = true;
    error.value = null;
    try {
      ledger.value = await backend.resetCoverageLedger();
      message.value = t.value.ledgerReset;
    } catch (cause) {
      error.value = toUserMessage(cause, t.value.ledgerResetFailed);
    } finally {
      busy.value = false;
    }
  };

  return {
    t,
    streamLabel,
    ledger,
    busy,
    error,
    message,
    estimate,
    startChoice,
    customFrom,
    customMin,
    customMax,
    START_CHOICES,
    fromDate,
    requestedDays,
    wouldBeCleanedUp,
    remaining,
    formatBytes,
    measuredStreams,
    unmeasuredStreams,
    estimateText,
    stopReasonText,
    chunkErrorText,
    autoContinue,
    stopRequested,
    isSyncing,
    toggleArchive,
    stopBackfill,
    runBackfill,
    fillFrom,
    retryFailed,
    resetLedger,
  };
};
