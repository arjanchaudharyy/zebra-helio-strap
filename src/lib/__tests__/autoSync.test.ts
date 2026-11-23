import { describe, expect, it } from 'vitest';
import { autoSyncBackoffElapsed, autoSyncRetryMinutes, launchSyncIsDue, nextAutoSyncFailures } from '../autoSync';

describe('auto sync backoff', () => {
  it('retries after 1 / 2 / 4 / 8 minutes, capped at the interval', () => {
    expect([0, 1, 2, 3, 4, 5, 9].map((n) => autoSyncRetryMinutes(n, 15))).toEqual([15, 1, 2, 4, 8, 8, 8]);
    expect(autoSyncRetryMinutes(4, 60)).toBe(8);
  });

  it('waits the backoff after a failure instead of a whole interval', () => {
    const now = Date.UTC(2026, 10, 3, 8, 0);
    expect(autoSyncBackoffElapsed(0, 0, 15, now)).toBe(true);
    expect(autoSyncBackoffElapsed(now - 60_000, 1, 15, now)).toBe(true);
    expect(autoSyncBackoffElapsed(now - 60_000, 2, 15, now)).toBe(false);
    expect(autoSyncBackoffElapsed(now - 10 * 60_000, 0, 15, now)).toBe(false);
    expect(autoSyncBackoffElapsed(now - 15 * 60_000, 0, 15, now)).toBe(true);
  });

  it('only a failed run counts as a failure', () => {
    expect(nextAutoSyncFailures(2, 'failed')).toBe(3);
    for (const outcome of ['success', 'no_new_data', 'cloud_stale', 'deferred', 'cancelled', null]) {
      expect(nextAutoSyncFailures(2, outcome)).toBe(0);
    }
  });
});

describe('launchSyncIsDue', () => {
  const now = new Date('2026-09-04T12:00:00Z').getTime();
  const minutesAgo = (minutes: number) =>
    new Date(now - minutes * 60_000).toISOString();

  it('syncs when it never synced before (the first run after connecting)', () => {
    expect(launchSyncIsDue(null, 15, now)).toBe(true);
    expect(launchSyncIsDue(undefined, 15, now)).toBe(true);
    expect(launchSyncIsDue('', 15, now)).toBe(true);
  });

  it('does not run a second sync right after a fresh sync', () => {
    expect(launchSyncIsDue(minutesAgo(0), 15, now)).toBe(false);
    expect(launchSyncIsDue(minutesAgo(5), 15, now)).toBe(false);
    expect(launchSyncIsDue(minutesAgo(14.9), 15, now)).toBe(false);
  });

  it('syncs once one auto sync interval has passed', () => {
    expect(launchSyncIsDue(minutesAgo(15), 15, now)).toBe(true);
    expect(launchSyncIsDue(minutesAgo(60), 15, now)).toBe(true);
    expect(launchSyncIsDue(minutesAgo(60 * 24), 15, now)).toBe(true);
  });

  it('uses the interval the user set as the threshold', () => {
    expect(launchSyncIsDue(minutesAgo(30), 15, now)).toBe(true);
    expect(launchSyncIsDue(minutesAgo(30), 60, now)).toBe(false);
  });

  it('uses the default 15 minutes for an unknown interval, not never sync', () => {
    expect(launchSyncIsDue(minutesAgo(20), Number.NaN, now)).toBe(true);
    expect(launchSyncIsDue(minutesAgo(20), 99999, now)).toBe(true);
    expect(launchSyncIsDue(minutesAgo(5), 99999, now)).toBe(false);
  });

  it('a bad timestamp or a clock jump back does not block sync forever', () => {
    expect(launchSyncIsDue('not a timestamp', 15, now)).toBe(true);
    const future = new Date(now + 60 * 60_000).toISOString();
    expect(launchSyncIsDue(future, 15, now)).toBe(true);
  });
});
