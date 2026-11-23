import { computed } from 'vue';
import { displayDateTimeFormatter } from '../../lib/dateTime';
import { syncStreamLabel } from '../../lib/syncStreams';
import { failedStreamKeys } from '../../lib/syncDeferred';
import { errorTextFor } from '../../i18n/errors';
import { backendText } from '../../i18n/backendText';
import type { SyncOutcome, SyncReport } from '../../types';
import { appStatus, copy, notice, statusError, statusErrorFromSync, type SyncNotice } from './state';


export const formatTime = (value?: string): string => {
  if (!value) return copy().timeUnknown;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return copy().timeUnknown;
  return displayDateTimeFormatter({
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  }).format(date);
};

export const formatClock = (value?: string): string | null => {
  if (!value) return null;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  return displayDateTimeFormatter({ hour: '2-digit', minute: '2-digit' }).format(date);
};

const latestHeartRateAt = (report?: SyncReport | null): string | undefined =>
  report?.streams.find((stream) => stream.stream === 'heart_rate')?.newest_sample_at
  ?? appStatus.value?.streams.find((stream) => stream.stream === 'heart_rate')?.newest_sample_at;

const KNOWN_SYNC_OUTCOMES: readonly SyncOutcome[] = [
  'updated',
  'no_new_data',
  'partial',
  'failed',
  'cancelled',
  'deferred',
  'cloud_stale',
];

export const syncOutcomeLabel = (outcome: string | null | undefined): string | null => {
  if (!outcome || !(KNOWN_SYNC_OUTCOMES as readonly string[]).includes(outcome)) return null;
  return renderReport(outcome as SyncOutcome, []);
};

const renderReport = (
  outcome: SyncOutcome,
  failedStreams: string[],
  latestAt?: string,
  backendCode?: string,
  backendMessage?: string,
  cloudLatestAt?: string,
): string => {
  const t = copy();
  const latest = latestAt ? formatTime(latestAt) : null;
  if (outcome === 'updated') return latest ? t.updatedWithLatest(latest) : t.updated;
  if (outcome === 'no_new_data') return latest ? t.noNewDataWithLatest(latest) : t.noNewData;
  if (outcome === 'partial') {
    const names = failedStreams.map((stream) => syncStreamLabel(stream));
    return names.length ? t.partialWithStreams(names.join(t.streamSeparator)) : t.partial;
  }
  if (outcome === 'cancelled') return t.cancelled;
  if (outcome === 'deferred') return errorTextFor(backendCode) ?? backendText(backendMessage, t.deferred);
  if (outcome === 'cloud_stale') {
    const clock = cloudLatestAt ? formatTime(cloudLatestAt) : null;
    return clock ? t.cloudStale(clock) : t.cloudStaleNoTime;
  }
  return t.failed;
};

const incrementalSyncDays = () => appStatus.value?.incremental_sync_days ?? 30;

const renderNotice = (value: SyncNotice): string => {
  const t = copy();
  switch (value.kind) {
    case 'none': return t.notSyncedYet;
    case 'backend': return backendText(value.text, t.syncingRecent(incrementalSyncDays()));
    case 'progress': {
      const stream = syncStreamLabel(value.stream);
      if (value.code === 'backfilling' && value.month) return t.backfillingStream(stream, value.month);
      if (value.code === 'backfilling') return t.syncingStream(stream);
      if (value.code === 'syncing' || value.code === 'stream_completed') return t.syncingStream(stream);
      return backendText(value.text, t.syncingRecent(incrementalSyncDays()));
    }
    case 'syncingRecent': return t.syncingRecent(value.days ?? incrementalSyncDays());
    case 'backfilling': return t.backfilling(value.days);
    case 'alreadySyncing': return t.alreadySyncing;
    case 'desktopOnly': return t.desktopOnly;
    case 'reauthNeeded': return t.reauthNeeded;
    case 'verifyFirst': return t.verifyFirst;
    case 'connectFirst': return t.connectFirst;
    case 'cancelling': return t.cancelling;
    case 'report': {
      const text = renderReport(value.outcome, value.failedStreams, value.latestAt, value.backendCode, value.backendMessage, value.cloudLatestAt);
      return value.cleanupFailed ? `${text} · ${t.cleanupFailed}` : text;
    }
  }
};

export const syncMessage = computed(() => renderNotice(notice.value));

export const statusErrorText = computed(() =>
  statusError.value ?? (statusErrorFromSync.value ? syncMessage.value : null));

export const noticeForReport = (report: SyncReport): SyncNotice => ({
  kind: 'report',
  outcome: report.outcome,
  failedStreams: failedStreamKeys(report.streams),
  latestAt: latestHeartRateAt(report),
  backendCode: report.outcome === 'deferred' ? report.message_code ?? undefined : undefined,
  backendMessage: report.outcome === 'deferred' ? report.message ?? undefined : undefined,
  cleanupFailed: report.notice_code === 'ui.sync.cleanup_failed',
  cloudLatestAt: report.message_code === 'ui.sync.cloud_stale' ? report.cloud_latest_at ?? undefined : undefined,
});

export const lastOutcomeLabel = computed(() => {
  const outcome = appStatus.value?.last_cloud_sync_outcome;
  if (!outcome) return null;
  const latest = latestHeartRateAt();
  if (outcome === 'no_new_data' && latest) return copy().noNewDataWithLatest(formatTime(latest));
  return copy().lastCloudSync(formatTime(appStatus.value?.last_cloud_sync_at));
});
