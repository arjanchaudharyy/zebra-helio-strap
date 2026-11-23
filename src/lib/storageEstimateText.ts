import { defineMessages, messagesOf } from '../i18n';
import { backendText } from '../i18n/backendText';
import { formatBytes } from './format';

export interface EstimateCopyInput {
  readonly message: string;
  readonly message_code?: string;
  readonly requested_days: number;
  readonly estimated_add_bytes: number;
  readonly free_bytes: number;
  readonly needed_bytes?: number;
  readonly stop_reason?: string | null;
  readonly stop_reason_code?: string | null;
}

const messages = defineMessages(
  {
    stopNoSpace: (needed: string, free: string) =>
      `This backfill needs about ${needed} (incl. safety margin); only ${free} is free. It will not start — free up space or shorten the range.`,
    diskUnknown: 'Could not read free disk space. Check there is enough room before backfilling.',
    diskTooSmall: 'Less than 300 MB free — history longer than 90 days cannot be backfilled.',
    builtinGuess: (days: number, add: string, free: string) =>
      `Too few local samples — rough built-in estimate: ${days} days ≈ ${add}; ${free} free on this drive.`,
    measured: (days: number, add: string, free: string) =>
      `At the rate your own data accumulates, ${days} days ≈ ${add}; ${free} free on this drive.`,
    partial: (days: number, add: string, free: string) =>
      `Only streams with enough local samples counted: ${days} days ≈ ${add} (rest not counted); ${free} free on this drive.`,
    unknownEstimate: 'Cannot estimate this backfill’s size right now.',
  },
);

export const formatEstimateBytes = (bytes: number): string => formatBytes(bytes, '0 KB');

export const storageEstimateText = (estimate: EstimateCopyInput | null | undefined): string => {
  if (!estimate) return '';
  const t = messagesOf(messages);
  const add = formatEstimateBytes(estimate.estimated_add_bytes);
  const free = formatEstimateBytes(estimate.free_bytes);
  switch (estimate.message_code) {
    case 'ui.estimate.stop_no_space':
      return t.stopNoSpace(formatEstimateBytes(estimate.needed_bytes ?? 0), free);
    case 'ui.estimate.disk_unknown': return t.diskUnknown;
    case 'ui.estimate.disk_too_small': return t.diskTooSmall;
    case 'ui.estimate.builtin_guess': return t.builtinGuess(estimate.requested_days, add, free);
    case 'ui.estimate.measured': return t.measured(estimate.requested_days, add, free);
    case 'ui.estimate.partial': return t.partial(estimate.requested_days, add, free);
    default: return backendText(estimate.message, t.unknownEstimate);
  }
};

export const storageStopReasonText = (
  estimate: EstimateCopyInput | null | undefined,
): string => {
  if (!estimate?.stop_reason) return '';
  if (estimate.stop_reason_code === 'ui.estimate.stop_no_space') {
    return messagesOf(messages).stopNoSpace(
      formatEstimateBytes(estimate.needed_bytes ?? 0),
      formatEstimateBytes(estimate.free_bytes),
    );
  }
  return backendText(estimate.stop_reason, messagesOf(messages).unknownEstimate);
};
