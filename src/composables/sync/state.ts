import { computed, ref, shallowRef } from 'vue';
import { backend, isDesktop, toUserMessage } from '../../lib/bridge';
import { readAutoSyncSettings } from '../../lib/autoSync';
import type { AppStatus, LoginStatus, SyncOutcome, SyncProgress, SyncReport } from '../../types';
import { IDLE, type DataReady } from '../../lib/dataReady';
import { createLoadSeq } from '../../lib/loadSeq';
import { messagesOf } from '../../i18n';
import { syncControllerMessages } from '../useSyncController.i18n';


export type SyncUiState = 'idle' | 'syncing' | SyncOutcome;

export const copy = () => messagesOf(syncControllerMessages);

export type SyncNotice =
  | { kind: 'none' }
  | { kind: 'backend'; text: string }
  | { kind: 'progress'; code: string; stream: string; month: string | null; text: string }
  | { kind: 'syncingRecent'; days?: number }
  | { kind: 'backfilling'; days: number }
  | { kind: 'alreadySyncing' }
  | { kind: 'desktopOnly' }
  | { kind: 'reauthNeeded' }
  | { kind: 'verifyFirst' }
  | { kind: 'connectFirst' }
  | { kind: 'cancelling' }
  | {
    kind: 'report';
    outcome: SyncOutcome;
    failedStreams: string[];
    latestAt?: string;
    backendCode?: string;
    backendMessage?: string;
    cleanupFailed?: boolean;
    cloudLatestAt?: string;
  };

export const appStatus = ref<AppStatus | null>(null);
export const statusError = ref<string | null>(null);
export const statusErrorFromSync = ref(false);
export const syncState = ref<SyncUiState>('idle');
export const notice = ref<SyncNotice>({ kind: 'none' });
export const syncReport = ref<SyncReport | null>(null);
export const cloudStaleTip = ref<{ latestAt: string | null } | null>(null);
export const syncProgress = ref<SyncProgress | null>(null);
export const loginStatus = ref<LoginStatus>({ state: 'idle', message: '', page_url: '' });
export const dataRevision = ref(0);
export const streamUpdate = ref({ stream: '', revision: 0 });
export const streamRevisions = shallowRef<Readonly<Record<string, number>>>({});
export const compactionPending = ref(0);
export const compactingEvent = ref(false);
export const compacting = computed(() => compactingEvent.value || appStatus.value?.compacting === true);
export const compactionSaved = ref<number | null>(null);
export const dataReady = ref<DataReady>(IDLE);
export const autoSyncEnabled = ref(readAutoSyncSettings().enabled);
export const autoSyncInterval = ref(readAutoSyncSettings().intervalMinutes);

export const applyLoginStatus = (status: LoginStatus) => {
  loginStatus.value = status;
  if (status.state === 'connected') void refreshStatus();
};

const statusLoadSeq = createLoadSeq();

export const refreshStatus = async (opts?: { preserveError?: boolean }): Promise<AppStatus | null> => {
  if (!isDesktop()) return null;
  const seq = statusLoadSeq.next();
  try {
    if (!opts?.preserveError) {
      statusError.value = null;
      statusErrorFromSync.value = false;
    }
    const status = await backend.getAppStatus();
    if (!statusLoadSeq.isCurrent(seq)) return appStatus.value;
    appStatus.value = status;
    return status;
  } catch (error) {
    if (!statusLoadSeq.isCurrent(seq)) return appStatus.value;
    statusError.value = toUserMessage(error, copy().statusUnavailable);
    return null;
  }
};
