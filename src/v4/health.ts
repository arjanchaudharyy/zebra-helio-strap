/* One snapshot of the person's recent health, computed in code.
   Body Age and the Coach both read it, so the AI explains numbers instead of inventing them. */
import { backend, bedtimeMinutes, byDate, fetchSeries, lastDays, mean, median, nights, quantile, restingSeries, wakeDay } from './data';
import type { MetricSeries, SleepSession } from '../types';

export interface Window {
  days: number;
  nightsWithData: number;
  sleepHours: number | null;
  sleepScore: number | null;
  bedtimeSpreadHours: number | null;
  bedtimeMedian: string | null;
  steps: number | null;
  daysWithSteps: number;
  restingHr: number | null;
  daysWithRestingHr: number;
  pai: number | null;
}

export interface Snapshot {
  generatedAt: string;
  last30: Window;
  prev30: Window;
  last7: Window;
  nightsTotal: number;
  firstDay: string | null;
  lastNight: { day: string; hours: number; score: number | null; deep: number | null; rem: number | null; light: number | null; awake: number | null } | null;
}

const fmtClock = (minutesAfter6pm: number): string => {
  const total = Math.round(minutesAfter6pm + 18 * 60) % (24 * 60);
  const h = Math.floor(total / 60);
  const m = total % 60;
  const suffix = h >= 12 ? 'PM' : 'AM';
  return `${((h + 11) % 12) + 1}:${String(m).padStart(2, '0')} ${suffix}`;
};

const round = (v: number | null, d = 1) => (v == null ? null : Math.round(v * 10 ** d) / 10 ** d);

const windowStats = (dayKeys: string[], allNights: SleepSession[], series: MetricSeries[]): Window => {
  const keySet = new Set(dayKeys);
  const ns = allNights.filter((n) => keySet.has(wakeDay(n)));
  const beds = ns.map(bedtimeMinutes);
  const steps = byDate(series.find((s) => s.metric === 'steps'));
  const worn = dayKeys.map((k) => steps.get(k)).filter((v): v is number => v != null && v >= 300);
  const rest = byDate(restingSeries(series));
  const rhr = dayKeys.map((k) => rest.get(k)).filter((v): v is number => v != null);
  const pai = byDate(series.find((s) => s.metric === 'pai_total'));
  const paiVals = dayKeys.map((k) => pai.get(k)).filter((v): v is number => v != null);
  return {
    days: dayKeys.length,
    nightsWithData: ns.length,
    sleepHours: round(median(ns.map((n) => n.duration_minutes / 60)), 2),
    sleepScore: round(mean(ns.map((n) => n.score).filter((s): s is number => s != null)), 0),
    bedtimeSpreadHours: beds.length >= 5 ? round((quantile(beds, 0.9)! - quantile(beds, 0.1)!) / 60) : null,
    bedtimeMedian: beds.length ? fmtClock(median(beds)!) : null,
    steps: round(mean(worn), 0),
    daysWithSteps: worn.length,
    restingHr: round(median(rhr), 0),
    daysWithRestingHr: rhr.length,
    pai: round(median(paiVals), 0),
  };
};

export const loadSnapshot = async (): Promise<Snapshot> => {
  const [sleep, series] = await Promise.all([
    backend.getRecentSleep(200).catch(() => [] as SleepSession[]),
    fetchSeries(['steps', 'resting_hr', 'device_resting_hr', 'pai_total'], 90).catch(() => [] as MetricSeries[]),
  ]);
  const allNights = nights(sleep);
  const d60 = lastDays(60);
  const last = allNights[0];
  return {
    generatedAt: new Date().toISOString(),
    last30: windowStats(d60.slice(30), allNights, series),
    prev30: windowStats(d60.slice(0, 30), allNights, series),
    last7: windowStats(d60.slice(53), allNights, series),
    nightsTotal: allNights.length,
    firstDay: allNights.length ? wakeDay(allNights[allNights.length - 1]) : null,
    lastNight: last ? {
      day: wakeDay(last), hours: round(last.duration_minutes / 60, 2)!, score: last.score ?? null,
      deep: last.deep_minutes ?? null, rem: last.rem_minutes ?? null, light: last.light_minutes ?? null, awake: last.awake_minutes ?? null,
    } : null,
  };
};

/* ---------- Body Age ----------
   A transparent estimate: start from the real age, then each habit adds or removes years.
   Bands are fixed in code, so the same data always gives the same answer. */

export interface Factor {
  key: string;
  label: string;
  value: string;
  years: number | null; // null = not enough data
  advice: string;
}

const band = (v: number, rules: [number, number][], fallback: number) => {
  for (const [limit, years] of rules) if (v < limit) return years;
  return fallback;
};

export const MIN_DAYS = 14;

export const factors = (w: Window): Factor[] => {
  const out: Factor[] = [];
  out.push({
    key: 'sleep', label: 'Hours of sleep',
    value: w.sleepHours == null ? 'No data' : `${w.sleepHours.toFixed(1)} h a night`,
    years: w.nightsWithData < MIN_DAYS / 2 || w.sleepHours == null ? null : band(w.sleepHours, [[6, 2], [6.5, 1], [7, 0], [7.5, -0.5]], -1),
    advice: 'Aim for 7.5 hours or more.',
  });
  out.push({
    key: 'consistency', label: 'Sleep consistency',
    value: w.bedtimeSpreadHours == null ? 'No data' : `Bedtime moves ${w.bedtimeSpreadHours.toFixed(1)} h`,
    years: w.bedtimeSpreadHours == null ? null : band(w.bedtimeSpreadHours, [[1, -1], [2, 0], [3, 1]], 2),
    advice: 'Go to bed within the same hour each night.',
  });
  out.push({
    key: 'steps', label: 'Daily steps',
    value: w.steps == null ? 'No data' : `${w.steps.toLocaleString('en-GB')} a day`,
    years: w.daysWithSteps < MIN_DAYS || w.steps == null ? null : band(w.steps, [[3000, 2], [5000, 1], [7500, 0], [10000, -0.75]], -1.5),
    advice: 'Walk 7,500 steps or more a day.',
  });
  out.push({
    key: 'rhr', label: 'Resting heart rate',
    value: w.restingHr == null ? 'No data' : `${w.restingHr} bpm`,
    years: w.daysWithRestingHr < MIN_DAYS || w.restingHr == null ? null : band(w.restingHr, [[55, -1.5], [60, -0.75], [65, 0], [70, 0.75], [75, 1.5]], 2.5),
    advice: 'Regular cardio and good sleep bring it down.',
  });
  out.push({
    key: 'pai', label: 'Heart workouts (PAI)',
    value: w.pai == null ? 'No data' : `PAI ${w.pai}`,
    years: w.pai == null ? null : band(w.pai, [[25, 1.5], [50, 0.75], [75, 0], [100, -0.75]], -1.5),
    advice: 'Keep PAI at 100 or more with brisk exercise.',
  });
  return out;
};

export const bodyAge = (realAge: number, w: Window) => {
  const fs = factors(w);
  const counted = fs.filter((f) => f.years != null);
  const delta = counted.reduce((sum, f) => sum + (f.years ?? 0), 0);
  return { factors: fs, counted: counted.length, delta, age: Math.max(10, realAge + delta) };
};

export const BIRTH_YEAR_KEY = 'zebra.birthYear';
export const readBirthYear = (): number | null => {
  try {
    const v = Number(window.localStorage.getItem(BIRTH_YEAR_KEY));
    return Number.isInteger(v) && v > 1900 && v <= new Date().getFullYear() ? v : null;
  } catch { return null; }
};
export const saveBirthYear = (year: number) => {
  try { window.localStorage.setItem(BIRTH_YEAR_KEY, String(year)); } catch { /* kept for this session only */ }
};
