const AUTO_SYNC_ENABLED_KEY = 'zeppbridge-auto-sync-enabled';
const AUTO_SYNC_INTERVAL_KEY = 'zeppbridge-auto-sync-interval-minutes';

export const AUTO_SYNC_INTERVALS = [15, 30, 60] as const;

export interface AutoSyncSettings {
  enabled: boolean;
  intervalMinutes: number;
}

const normalizeInterval = (value: unknown): number => {
  const parsed = typeof value === 'number' ? value : Number(value);
  return AUTO_SYNC_INTERVALS.includes(parsed as (typeof AUTO_SYNC_INTERVALS)[number]) ? parsed : 15;
};

export const readAutoSyncSettings = (): AutoSyncSettings => {
  if (typeof window === 'undefined') return { enabled: true, intervalMinutes: 15 };
  return {
    enabled: window.localStorage.getItem(AUTO_SYNC_ENABLED_KEY) !== 'false',
    intervalMinutes: normalizeInterval(window.localStorage.getItem(AUTO_SYNC_INTERVAL_KEY)),
  };
};

export const writeAutoSyncSettings = (settings: AutoSyncSettings): AutoSyncSettings => {
  const normalized = { enabled: Boolean(settings.enabled), intervalMinutes: normalizeInterval(settings.intervalMinutes) };
  if (typeof window !== 'undefined') {
    window.localStorage.setItem(AUTO_SYNC_ENABLED_KEY, String(normalized.enabled));
    window.localStorage.setItem(AUTO_SYNC_INTERVAL_KEY, String(normalized.intervalMinutes));
  }
  return normalized;
};

export const autoSyncRetryMinutes = (failures: number, intervalMinutes: number): number => {
  const interval = normalizeInterval(intervalMinutes);
  if (!(failures > 0)) return interval;
  return Math.min(2 ** Math.min(Math.floor(failures) - 1, 3), interval);
};

export const autoSyncBackoffElapsed = (
  lastAttemptAt: number,
  failures: number,
  intervalMinutes: number,
  now: number = Date.now(),
): boolean => !lastAttemptAt || now - lastAttemptAt >= autoSyncRetryMinutes(failures, intervalMinutes) * 60_000;

export const nextAutoSyncFailures = (previous: number, outcome: string | null | undefined): number =>
  outcome === 'failed' ? previous + 1 : 0;

export const launchSyncIsDue = (
  lastCloudSyncAt: string | null | undefined,
  intervalMinutes: number,
  now: number = Date.now(),
): boolean => {
  if (!lastCloudSyncAt) return true;
  const at = new Date(lastCloudSyncAt).getTime();
  if (Number.isNaN(at)) return true;
  const elapsedMinutes = (now - at) / 60_000;
  if (elapsedMinutes < 0) return true;
  return elapsedMinutes >= normalizeInterval(intervalMinutes);
};
