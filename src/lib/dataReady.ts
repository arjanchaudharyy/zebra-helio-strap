import type { SyncOutcome, SyncReport } from '../types';

export type ReadyOutcome = Extract<SyncOutcome, 'updated' | 'no_new_data' | 'partial'>;

export type DataReady =
  | { phase: 'idle' }
  | { phase: 'waiting' }
  | {
    phase: 'ready';
    outcome: ReadyOutcome;
    records: number;
    finishedAt: string;
    failedStreams: string[];
    firstRun: boolean;
  };

export const IDLE: DataReady = { phase: 'idle' };

export const readyOnStart = (state: DataReady, waited: boolean): DataReady =>
  (waited ? { phase: 'waiting' } : state);

export const readyOnReport = (
  state: DataReady,
  report: SyncReport | null,
  failedStreams: string[] = [],
  firstRun = false,
): DataReady => {
  if (state.phase !== 'waiting') return state;
  if (!report) return IDLE;
  switch (report.outcome) {
    case 'updated':
    case 'no_new_data':
    case 'partial':
      return {
        phase: 'ready',
        outcome: report.outcome,
        records: Math.max(0, report.total_records),
        finishedAt: report.finished_at,
        failedStreams: report.outcome === 'partial' ? failedStreams : [],
        firstRun: firstRun && report.total_records > 0,
      };
    case 'deferred':
      return state;
    default:
      return IDLE;
  }
};

export const readyOnPickUp = (state: DataReady): DataReady => (state.phase === 'ready' ? IDLE : state);
