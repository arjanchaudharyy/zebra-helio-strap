
const DEFERRED_CODES = new Set([
  'err.sync.deferred_compaction',
  'err.sync.deferred_replay',
  'err.sync.deferred_busy',
  'err.core.busy',
]);

export const isDeferredSyncError = (error: unknown): boolean => {
  if (typeof error !== 'object' || error === null) return false;
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' && DEFERRED_CODES.has(code);
};

export const isLocalMaintenanceError = (error: unknown): boolean => {
  if (typeof error !== 'object' || error === null) return false;
  const code = (error as { code?: unknown }).code;
  return code === 'err.local.maintenance_compaction' || code === 'err.local.maintenance_replay';
};

export const isCancelledSyncError = (error: unknown): boolean => {
  if (typeof error !== 'object' || error === null) return false;
  const code = (error as { code?: unknown }).code;
  return code === 'err.core.cancelled';
};

export const failedStreamKeys = (
  streams: Array<{ stream: string; status: string }>,
): string[] => streams
  .filter((stream) => stream.status === 'failed')
  .map((stream) => stream.stream);
