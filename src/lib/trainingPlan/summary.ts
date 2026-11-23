import type { PlanIntensity } from '../../types/trainingPlan';
import type { DayRow } from './week';
import { workoutProfile, type ProfileSegment, type WorkoutProfile } from './profile';

export const INTENSITY_LEVEL: Record<PlanIntensity, number> = {
  rest: 0.14,
  cooldown: 0.3,
  recovery: 0.3,
  warmup: 0.38,
  active: 0.6,
  interval: 0.92,
};

export interface HrDomain { min: number; max: number }

export const sharedHrDomain = (profiles: WorkoutProfile[]): HrDomain | null => {
  const lows: number[] = [];
  const highs: number[] = [];
  for (const profile of profiles) {
    for (const segment of profile.segments) {
      if (segment.low !== null && segment.high !== null) { lows.push(segment.low); highs.push(segment.high); }
    }
  }
  if (!lows.length) return null;
  const min = Math.max(40, Math.floor((Math.min(...lows) - 25) / 10) * 10);
  const max = Math.ceil((Math.max(...highs) + 5) / 10) * 10;
  return { min, max: Math.max(min + 40, max) };
};

export const segmentLevel = (segment: ProfileSegment, domain: HrDomain | null): number => {
  if (domain && segment.low !== null && segment.high !== null) {
    const mid = (segment.low + segment.high) / 2;
    return Math.min(1, Math.max(0.12, (mid - domain.min) / (domain.max - domain.min)));
  }
  return INTENSITY_LEVEL[segment.intensity];
};

export interface WeekStats {
  sessions: number;
  seconds: number;
  approx: boolean;
  hard: number;
  restDays: number;
}

export const weekStats = (rows: DayRow[]): WeekStats => {
  const stats: WeekStats = { sessions: 0, seconds: 0, approx: false, hard: 0, restDays: 0 };
  for (const row of rows) {
    if (!row.inWindow) continue;
    if (!row.after.length) { stats.restDays += 1; continue; }
    for (const workout of row.after) {
      const profile = workoutProfile(workout);
      stats.sessions += 1;
      stats.seconds += profile.seconds;
      stats.approx = stats.approx || profile.approx;
      if (profile.segments.some((segment) => segment.intensity === 'interval')) stats.hard += 1;
    }
  }
  return stats;
};

export interface RepeatSpan {
  from: number;
  to: number;
  times: number;
}

export const repeatSpans = (segments: ProfileSegment[]): RepeatSpan[] => {
  const spans: RepeatSpan[] = [];
  let index = 0;
  while (index < segments.length) {
    if (segments[index].round === null) { index += 1; continue; }
    const from = index;
    let times = 0;
    while (index < segments.length && segments[index].round !== null && !(index > from && segments[index].round === 0 && segments[index - 1].round !== 0)) {
      times = Math.max(times, (segments[index].round as number) + 1);
      index += 1;
    }
    spans.push({ from, to: index, times });
  }
  return spans;
};
