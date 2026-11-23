/* One snapshot of the person's recent health, computed in code.
   Body Age and the Coach both read it, so the AI explains numbers instead of inventing them. */
import { ref } from 'vue';
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
  hrv: number | null;
  daysWithHrv: number;
  readiness: number | null;
  daysWithReadiness: number;
}

export interface Snapshot {
  generatedAt: string;
  last30: Window;
  prev30: Window;
  last7: Window;
  nightsTotal: number;
  firstDay: string | null;
  strap: { date: string; hrvRmssd: number | null; skinTempC: number | null; spo2Avg: number | null; stressAvg: number | null; breathRate: number | null }[];
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

type StrapRow = Awaited<ReturnType<typeof backend.strapDaily>>[number];

const windowStats = (dayKeys: string[], allNights: SleepSession[], series: MetricSeries[], strapRows: StrapRow[] = []): Window => {
  const keySet = new Set(dayKeys);
  const ns = allNights.filter((n) => keySet.has(wakeDay(n)));
  const beds = ns.map(bedtimeMinutes);
  const steps = byDate(series.find((s) => s.metric === 'steps'));
  const worn = dayKeys.map((k) => steps.get(k)).filter((v): v is number => v != null && v >= 300);
  const rest = byDate(restingSeries(series));
  const rhr = dayKeys.map((k) => rest.get(k)).filter((v): v is number => v != null);
  const pai = byDate(series.find((s) => s.metric === 'pai_total'));
  const paiVals = dayKeys.map((k) => pai.get(k)).filter((v): v is number => v != null);
  const strapIn = strapRows.filter((r) => keySet.has(r.date));
  const hrvVals = strapIn.map((r) => r.hrvRmssd).filter((v): v is number => v != null);
  const readyVals = strapIn.map((r) => r.readiness).filter((v): v is number => v != null);
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
    hrv: round(median(hrvVals), 0),
    daysWithHrv: hrvVals.length,
    readiness: round(mean(readyVals), 0),
    daysWithReadiness: readyVals.length,
  };
};

export interface RawHealth { nights: SleepSession[]; series: MetricSeries[]; strap: StrapRow[] }

/** Everything the estimates need, loaded once. 400 days of history so trends keep updating. */
export const loadRaw = async (): Promise<RawHealth> => {
  const [strap, sleep, series] = await Promise.all([
    backend.strapDaily().catch(() => [] as StrapRow[]),
    backend.getRecentSleep(500).catch(() => [] as SleepSession[]),
    fetchSeries(['steps', 'resting_hr', 'device_resting_hr', 'pai_total', 'sleep_score'], 400).catch(() => [] as MetricSeries[]),
  ]);
  return { nights: nights(sleep), series, strap };
};

/** The 30 days ending on `endKey` (inclusive). */
const windowEnding = (raw: RawHealth, endKey: string, days = 30) => {
  const [y, m, d] = endKey.split('-').map(Number);
  const keys = Array.from({ length: days }, (_, i) => {
    const t = new Date(y, m - 1, d - (days - 1 - i));
    return `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, '0')}-${String(t.getDate()).padStart(2, '0')}`;
  });
  return windowStats(keys, raw.nights, raw.series, raw.strap);
};

export const snapshotFrom = (raw: RawHealth): Snapshot => {
  const allNights = raw.nights;
  const d60 = lastDays(60);
  const last = allNights[0];
  return {
    generatedAt: new Date().toISOString(),
    last30: windowStats(d60.slice(30), allNights, raw.series, raw.strap),
    prev30: windowStats(d60.slice(0, 30), allNights, raw.series, raw.strap),
    last7: windowStats(d60.slice(53), allNights, raw.series, raw.strap),
    strap: raw.strap.slice(-14).map((d) => ({ date: d.date, hrvRmssd: d.hrvRmssd, skinTempC: d.skinTempC, spo2Avg: d.spo2Avg, stressAvg: d.stressAvg, breathRate: d.breathRate })),
    nightsTotal: allNights.length,
    firstDay: allNights.length ? wakeDay(allNights[allNights.length - 1]) : null,
    lastNight: last ? {
      day: wakeDay(last), hours: round(last.duration_minutes / 60, 2)!, score: last.score ?? null,
      deep: last.deep_minutes ?? null, rem: last.rem_minutes ?? null, light: last.light_minutes ?? null, awake: last.awake_minutes ?? null,
    } : null,
  };
};

export const loadSnapshot = async (): Promise<Snapshot> => snapshotFrom(await loadRaw());

/* ---------- Body Age v2 ----------
   Start from the real age. Each measured habit moves it by a smooth, capped amount, scaled by how
   much data backs it (coverage). HRV and resting heart rate are compared with what is normal for
   the person's age. The same data always gives the same answer, and it is recomputed every day. */

export interface Factor {
  key: string;
  label: string;
  value: string;
  years: number | null; // null = not enough data
  advice: string;
  coverage: number;     // 0..1, how much data backs this factor
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
export const MIN_DAYS = 14;
/** Sleep need by age (hours): teenagers need more. */
const sleepNeed = (age: number) => (age < 18 ? 8.5 : age < 65 ? 7.5 : 7.25);
/** Typical night RMSSD for an age (ms); falls about 0.75 ms a year. */
const hrvNorm = (age: number) => Math.max(20, 70 - 0.75 * age);

export const factors = (w: Window, age: number): Factor[] => {
  const cov = (n: number, need: number) => clamp(n / need, 0, 1);
  const f = (key: string, label: string, value: string, raw: number | null, coverage: number, advice: string): Factor => ({
    key, label, value, advice, coverage,
    years: raw == null || coverage < 0.25 ? null : Math.round(raw * coverage * 10) / 10,
  });
  const need = sleepNeed(age);
  return [
    f('sleep', 'Hours of sleep', w.sleepHours == null ? 'No data' : `${w.sleepHours.toFixed(1)} h a night (you need about ${need} h)`,
      w.sleepHours == null ? null : clamp((need - w.sleepHours) * 1.2, -1, 3), cov(w.nightsWithData, 14), `Sleep ${need} hours or more.`),
    f('consistency', 'Sleep consistency', w.bedtimeSpreadHours == null ? 'No data' : `Bedtime moves ${w.bedtimeSpreadHours.toFixed(1)} h`,
      w.bedtimeSpreadHours == null ? null : clamp((w.bedtimeSpreadHours - 1.5) * 0.6, -1, 2.5), cov(w.nightsWithData, 14), 'Go to bed within the same hour each night.'),
    f('hrv', 'HRV (from strap)', w.hrv == null ? 'No data' : `${w.hrv} ms (typical for ${age}: ${Math.round(hrvNorm(age))} ms)`,
      w.hrv == null ? null : clamp(((70 - w.hrv) / 0.75 - age) * 0.15, -2, 2.5), cov(w.daysWithHrv, 7), 'Steady sleep and less late caffeine raise HRV.'),
    f('rhr', 'Resting heart rate', w.restingHr == null ? 'No data' : `${w.restingHr} bpm`,
      w.restingHr == null ? null : clamp(((w.restingHr - 62) / 4) * 0.6, -2, 2.5), cov(w.daysWithRestingHr, 14), 'Regular cardio and good sleep bring it down.'),
    f('steps', 'Daily steps', w.steps == null ? 'No data' : `${w.steps.toLocaleString('en-GB')} a day`,
      w.steps == null ? null : clamp(((7500 - w.steps) / 2500) * 1, -1.5, 2), cov(w.daysWithSteps, 14), 'Walk 7,500 steps or more a day.'),
    f('pai', 'Heart workouts (PAI)', w.pai == null ? 'No data' : `PAI ${w.pai}`,
      w.pai == null ? null : clamp(((75 - w.pai) / 25) * 0.5, -1.5, 1.5), w.pai == null ? 0 : 1, 'Keep PAI at 100 or more with brisk exercise.'),
    f('readiness', 'Recovery (readiness)', w.readiness == null ? 'No data' : `${w.readiness} average`,
      w.readiness == null ? null : clamp(((70 - w.readiness) / 10) * 0.4, -1, 1), cov(w.daysWithReadiness, 7), 'Recover well between hard days.'),
  ];
};

export const bodyAge = (realAge: number, w: Window) => {
  const fs = factors(w, realAge);
  const counted = fs.filter((x) => x.years != null);
  const delta = Math.round(counted.reduce((sum, x) => sum + (x.years ?? 0), 0) * 10) / 10;
  return { factors: fs, counted: counted.length, delta, age: Math.max(10, realAge + delta) };
};

/** Body age for each of the last `days` days (each from the 30 days ending that day). */
export const bodyAgeHistory = (raw: RawHealth, realAge: number, days = 60) =>
  lastDays(days).map((key) => {
    const ba = bodyAge(realAge, windowEnding(raw, key));
    return { date: key, age: ba.counted >= 3 ? Math.round(ba.age * 10) / 10 : null };
  });

export const BIRTH_YEAR_KEY = 'zebra.birthYear';
const storedYear = (): number | null => {
  try {
    const v = Number(window.localStorage.getItem(BIRTH_YEAR_KEY));
    return Number.isInteger(v) && v > 1900 && v <= new Date().getFullYear() ? v : null;
  } catch { return null; }
};
/** Shared across pages, so setting it on Body age updates Today at once. */
export const birthYearRef = ref<number | null>(storedYear());
export const readBirthYear = (): number | null => birthYearRef.value;
export const saveBirthYear = (year: number | null) => {
  birthYearRef.value = year;
  try {
    if (year == null) window.localStorage.removeItem(BIRTH_YEAR_KEY);
    else window.localStorage.setItem(BIRTH_YEAR_KEY, String(year));
  } catch { /* kept for this session only */ }
};
