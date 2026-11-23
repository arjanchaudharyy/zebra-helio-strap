import { describe, expect, it } from 'vitest';
import { failedStreamKeys, isCancelledSyncError, isDeferredSyncError, isLocalMaintenanceError } from '../syncDeferred';

describe('failedStreamKeys', () => {
  it('lists only failed streams; unavailable and unverified stay neutral', () => {
    expect(failedStreamKeys([
      { stream: 'heart_rate', status: 'success' },
      { stream: 'sleep', status: 'failed' },
      { stream: 'hrv', status: 'unavailable' },
      { stream: 'wellness', status: 'unverified' },
    ])).toEqual(['sleep']);
  });
});

describe('isDeferredSyncError', () => {
  it('treats busy and local maintenance as deferred, so the UI retries instead of showing an error', () => {
    expect(isDeferredSyncError({ code: 'err.sync.deferred_replay' })).toBe(true);
    expect(isDeferredSyncError({ code: 'err.sync.deferred_compaction' })).toBe(true);
    expect(isDeferredSyncError({ code: 'err.sync.deferred_busy' })).toBe(true);
    expect(isDeferredSyncError({ code: 'err.core.busy' })).toBe(true);
    expect(isDeferredSyncError({ code: 'err.sync.not_connected' })).toBe(false);
    expect(isDeferredSyncError('busy')).toBe(false);
  });
});

describe('isCancelledSyncError', () => {
  it('accepts only the backend cancel code', () => {
    expect(isCancelledSyncError({ code: 'err.core.cancelled' })).toBe(true);
    expect(isCancelledSyncError({ code: 'err.sync.deferred_busy' })).toBe(false);
  });
});

describe('isLocalMaintenanceError', () => {
  it('recognises the read-side maintenance codes and nothing else', () => {
    expect(isLocalMaintenanceError({ code: 'err.local.maintenance_replay' })).toBe(true);
    expect(isLocalMaintenanceError({ code: 'err.local.maintenance_compaction' })).toBe(true);
    expect(isLocalMaintenanceError({ code: 'err.core.database' })).toBe(false);
    expect(isLocalMaintenanceError('err.local.maintenance_replay')).toBe(false);
  });
});
