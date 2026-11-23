import { displayDateTimeFormatter } from '../lib/dateTime';
import { computed, ref } from 'vue';
import { backend, isDesktop, toUserMessage } from '../lib/bridge';
import { DEVICE_READ_PREFIXES, forgetReads } from '../lib/readCache';
import { deviceImageFor } from '../lib/deviceImages';
import type { DeviceCacheMetadata, DeviceProfile, DeviceProfilesResult } from '../types';
import { messagesOf } from '../i18n';
import { devicesMessages } from './useDevices.i18n';
import { errorTextFor } from '../i18n/errors';
import { backendText } from '../i18n/backendText';

/**
 * The device catalog is deliberately treated as account data, not as a list
 * of products we happen to ship assets for.  Views consume this normalized
 * model so an empty/unknown account never falls back to a made-up watch.
 */
export type DeviceState = 'account' | 'user_assigned' | 'recent_data' | 'cached' | 'unknown';


const copy = () => messagesOf(devicesMessages);

const DEVICE_STATE_KEYS = {
  account: 'stateAccount',
  user_assigned: 'stateUserAssigned',
  recent_data: 'stateRecentData',
  cached: 'stateCached',
  unknown: 'stateUnknown',
} as const;

export const deviceStateLabel = (state: DeviceState): string => copy()[DEVICE_STATE_KEYS[state]];

export interface DeviceCardModel {
  profile: DeviceProfile;
  canonicalName: string;
  displayName: string;
  image: string;
  kind: string;
  state: DeviceState;
  firmware: string;
  lastData: string;
  hasLocalData: boolean;
  deviceKey: string;
  userAssigned: boolean;
  matchStatus: string;
  hidden: boolean;
}

const profiles = ref<DeviceProfile[]>([]);
const hiddenProfiles = ref<DeviceProfile[]>([]);
const cache = ref<DeviceCacheMetadata | null>(null);
const loading = ref(false);
const error = ref<string | null>(null);
const initialized = ref(false);
let requestId = 0;
// App and Overview mount together, and Settings can request a manual refresh
// while the startup read is still settling. Share identical bridge calls so
// one component cannot overwrite another with an older response.
const profileRequests = new Map<boolean, Promise<DeviceProfilesResult>>();
let backgroundRefreshAttempted = false;
let backgroundRefreshInFlight: Promise<DeviceProfilesResult> | null = null;

const formatDeviceDate = (value?: string | null): string => {
  if (!value) return copy().notFetchedYet;
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return copy().timeUnknown;
  return displayDateTimeFormatter({
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  }).format(date).replace(/\//g, '-');
};

const isGenericScale = (profile: DeviceProfile): boolean => profile.kind === 'scale' && !profile.canonical_name;

const stateFor = (profile: DeviceProfile): DeviceState => {
  if (profile.has_local_data) return 'recent_data';
  if (cache.value?.status === 'stale' || cache.value?.status === 'refresh_failed') return 'cached';
  if (profile.match_status === 'user_assigned') return 'user_assigned';
  if (profile.canonical_name || profile.match_status === 'exact' || profile.match_status === 'alias') return 'account';
  if (isGenericScale(profile)) return 'account';
  return 'unknown';
};

const normalizeResult = (result: DeviceProfilesResult | DeviceProfile[]): DeviceProfilesResult => {
  if (Array.isArray(result)) {
    return {
      profiles: result,
      cache: { status: 'fresh', refreshed: false },
    };
  }
  return {
    profiles: Array.isArray(result?.profiles) ? result.profiles : [],
    hidden_profiles: Array.isArray(result?.hidden_profiles) ? result.hidden_profiles : [],
    cache: result?.cache ?? { status: 'missing', refreshed: false },
  };
};

const requestProfiles = (refresh: boolean): Promise<DeviceProfilesResult> => {
  const existing = profileRequests.get(refresh);
  if (existing) return existing;

  const request = backend.getDeviceProfiles(refresh).then(normalizeResult);
  profileRequests.set(refresh, request);
  const clear = () => {
    if (profileRequests.get(refresh) === request) profileRequests.delete(refresh);
  };
  // Handle both outcomes so a transient failure does not leave a rejected
  // promise cached forever, while avoiding an unhandled rejection.
  void request.then(clear, clear);
  return request;
};

const refreshErrorText = (meta: DeviceCacheMetadata): string | null => {
  if (!meta.refresh_error && !meta.refresh_error_code) return null;
  return errorTextFor(meta.refresh_error_code)
    ?? backendText(meta.refresh_error, copy().identifyUnavailable);
};

const applyResult = (result: DeviceProfilesResult): void => {
  profiles.value = result.profiles;
  hiddenProfiles.value = result.hidden_profiles ?? [];
  cache.value = result.cache;
  error.value = refreshErrorText(result.cache);
};

const setLoadFailure = (cause: unknown, refresh: boolean): void => {
  const message = toUserMessage(cause, refresh ? copy().identifyUnavailable : copy().cacheUnavailable);
  error.value = message;
  const status = refresh ? 'refresh_failed' : 'unavailable';
  cache.value = cache.value
    ? { ...cache.value, status, refreshed: false, refresh_error: message }
    : { status, refreshed: false, refresh_error: message };
};

const hasCanonicalMatch = (profile: DeviceProfile): boolean => Boolean(
  profile.canonical_name?.trim()
  || profile.match_status === 'exact'
  || profile.match_status === 'alias'
  || profile.match_status === 'user_assigned',
);

const needsBackgroundRefresh = (result: DeviceProfilesResult): boolean => {
  if (backgroundRefreshAttempted || result.cache.status === 'refresh_failed') return false;
  if (result.cache.status === 'missing' || result.cache.status === 'stale') return true;
  return result.profiles.length === 0 || result.profiles.every((profile) => !hasCanonicalMatch(profile));
};

const startBackgroundRefresh = (triggerRequest: number): void => {
  if (!isDesktop() || backgroundRefreshAttempted || backgroundRefreshInFlight) return;
  backgroundRefreshAttempted = true;
  const request = requestProfiles(true);
  backgroundRefreshInFlight = request;

  // Do not toggle `loading`: the cache/list is already visible and this
  // refresh is deliberately best-effort. A later explicit load shares this
  // promise and applies the same result under its newer request id.
  void request
    .then(
      (result) => {
        if (requestId === triggerRequest) {
          applyResult(result);
          initialized.value = true;
        }
      },
      (cause) => {
        if (requestId === triggerRequest) setLoadFailure(cause, true);
      },
    )
    .finally(() => {
      if (backgroundRefreshInFlight === request) backgroundRefreshInFlight = null;
    })
    .catch(() => undefined);
};

const load = async (refresh = false): Promise<void> => {
  const currentRequest = ++requestId;
  // A background refresh must not turn an already-rendered cache back into a
  // blocking spinner. Explicit refreshes still expose their normal loading
  // state in Settings.
  const waitingForBackground = !refresh && Boolean(backgroundRefreshInFlight);
  if (!waitingForBackground) loading.value = true;
  error.value = null;

  if (!isDesktop()) {
    profiles.value = [];
    cache.value = { status: 'unavailable', refreshed: false };
    initialized.value = true;
    loading.value = false;
    return;
  }

  try {
    const result = refresh
      ? await requestProfiles(true)
      : await (backgroundRefreshInFlight || requestProfiles(false));
    if (currentRequest !== requestId) return;
    applyResult(result);
    if (!refresh && needsBackgroundRefresh(result)) startBackgroundRefresh(currentRequest);
  } catch (cause) {
    if (currentRequest !== requestId) return;
    // Keep the previous list visible during a transient failure and expose a
    // cache status that prevents an automatic retry loop.
    setLoadFailure(cause, refresh || waitingForBackground);
  } finally {
    if (currentRequest === requestId) {
      initialized.value = true;
      loading.value = false;
    }
  }
};

const reloadAfterLocalWrite = async (opts?: { quiet?: boolean }): Promise<void> => {
  if (!isDesktop()) return;
  const currentRequest = ++requestId;
  if (!opts?.quiet) loading.value = true;
  error.value = null;
  try {
    const result = normalizeResult(await backend.getDeviceProfiles(false));
    if (currentRequest !== requestId) return;
    applyResult(result);
  } catch (cause) {
    if (currentRequest !== requestId) return;
    setLoadFailure(cause, false);
  } finally {
    if (currentRequest === requestId) {
      initialized.value = true;
      loading.value = false;
    }
  }
};

const toModel = (profile: DeviceProfile): DeviceCardModel => ({
  profile,
  canonicalName: profile.canonical_name?.trim() || profile.name?.trim()
    || (isGenericScale(profile) ? copy().genericScale : copy().unidentifiedDevice),
  displayName: profile.display_name?.trim() || copy().notProvided,
  image: deviceImageFor(profile.kind, profile.image_key),
  kind: profile.kind || 'unknown',
  state: stateFor(profile),
  firmware: profile.firmware?.trim() || copy().notFetchedYet,
  lastData: formatDeviceDate(profile.last_data_at),
  hasLocalData: profile.has_local_data === true,
  deviceKey: (profile.device_id || profile.serial || '').trim(),
  userAssigned: profile.match_status === 'user_assigned',
  matchStatus: profile.match_status || 'unknown',
  hidden: profile.hidden === true,
});

const models = computed<DeviceCardModel[]>(() => profiles.value.map(toModel));
const hiddenModels = computed<DeviceCardModel[]>(() => hiddenProfiles.value.map(toModel));

const maskIdentifier = (value?: string | null): string => {
  const trimmed = value?.trim();
  if (!trimmed) return copy().notProvided;
  if (trimmed.length <= 4) return '•'.repeat(trimmed.length);
  return `••••${trimmed.slice(-4)}`;
};

const assignBusy = ref(false);
const assignError = ref<string | null>(null);
const assignMessage = ref<string | null>(null);

const assignModel = async (deviceKey: string, catalogId: string): Promise<void> => {
  if (!deviceKey) {
    assignError.value = copy().noLocalIdentifier;
    return;
  }
  assignBusy.value = true;
  assignError.value = null;
  assignMessage.value = null;
  try {
    await backend.setDeviceModelOverride(deviceKey, catalogId || null);
    forgetReads(...DEVICE_READ_PREFIXES);
    await reloadAfterLocalWrite();
    if (!catalogId) {
      assignMessage.value = copy().assignmentCleared;
      return;
    }
    assignMessage.value = copy().assignmentSaved;
  } catch (cause) {
    assignError.value = toUserMessage(cause, copy().assignmentFailed);
  } finally {
    assignBusy.value = false;
  }
};

const hideBusy = ref(false);
const hideError = ref<string | null>(null);

const setHidden = async (deviceKey: string, hidden: boolean): Promise<boolean> => {
  if (!deviceKey) {
    hideError.value = copy().noLocalIdentifier;
    return false;
  }
  hideBusy.value = true;
  hideError.value = null;
  try {
    await backend.setDeviceHidden(deviceKey, hidden);
    forgetReads(...DEVICE_READ_PREFIXES);
    await reloadAfterLocalWrite({ quiet: true });
    return true;
  } catch (cause) {
    hideError.value = toUserMessage(cause, copy().hideFailed);
    return false;
  } finally {
    hideBusy.value = false;
  }
};

export const useDeviceHiding = () => ({ hideBusy, hideError, setHidden });

const clearAssignFeedback = () => {
  assignError.value = null;
  assignMessage.value = null;
};

export const useDeviceAssignment = () => ({
  assignBusy,
  assignError,
  assignMessage,
  assignModel,
  clearAssignFeedback,
});

export const useDevices = () => ({
  profiles,
  models,
  hiddenModels,
  cache,
  loading,
  error,
  initialized,
  load,
  reloadAfterDataChange: () => reloadAfterLocalWrite({ quiet: true }),
  maskIdentifier,
  formatDeviceDate,
});
