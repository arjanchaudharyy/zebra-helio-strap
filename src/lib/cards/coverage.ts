import type { CoverageLedger } from '../../types';
import type { AiTaskCategory } from '../bridge/types';
import type { DeckGroup, GroupCoverage } from './deck';

const STREAMS: Partial<Record<AiTaskCategory, readonly string[]>> = {
  sleep: ['sleep'],
  heart_rate: ['heart_rate'],
  resting_hr: ['heart_rate', 'daily_summary'],
  recovery: ['daily_summary', 'hrv', 'wellness'],
  workout: ['workouts'],
  training: ['workouts'],
};

export interface CoverageContext {
  category: AiTaskCategory;
  earliestLocal: string | null;
  ledger: CoverageLedger | null;
}

export const monthsIn = (start: string, end: string): string[] => {
  const out: string[] = [];
  let [y, m] = start.split('-').map(Number) as [number, number];
  const last = end.slice(0, 7);
  for (;;) {
    const key = `${y}-${String(m).padStart(2, '0')}`;
    if (key > last) break;
    out.push(`${key}-01`);
    m += 1;
    if (m > 12) { m = 1; y += 1; }
  }
  return out;
};

export const groupCoverage = (group: DeckGroup, ctx: CoverageContext | null): GroupCoverage => {
  if (!ctx || group.days.some((day) => day.has)) return null;
  const streams = STREAMS[ctx.category] ?? [];
  if (ctx.ledger && streams.length) {
    const empty = streams.map((name) => new Set(ctx.ledger!.streams.find((item) => item.stream === name)?.empty_months ?? []));
    const months = monthsIn(group.start, group.end);
    if (months.length && months.every((month) => empty.every((set) => set.has(month)))) return 'cloud_empty';
  }
  if (!ctx.earliestLocal || group.end >= ctx.earliestLocal) return null;
  return 'not_backfilled';
};
