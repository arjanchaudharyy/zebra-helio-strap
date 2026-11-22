/* Data helpers for the v4 screens. Everything reads through `backend`; nothing here talks to Zepp.
   Missing data stays missing: helpers return null instead of 0. */
import { onMounted, ref, watch } from 'vue';
import { backend } from '../lib/bridge';
import { useSyncController } from '../composables/useSyncController';
import type { MetricSeries, MetricSeriesPoint, SleepSession } from '../types';

export const median = (values: number[]): number | null => {
  const v = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (!v.length) return null;
  const mid = Math.floor(v.length / 2);
  return v.length % 2 ? v[mid] : (v[mid - 1] + v[mid]) / 2;
};

export const quantile = (values: number[], q: number): number | null => {
  const v = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (!v.length) return null;
  const pos = (v.length - 1) * q;
  const lo = Math.floor(pos);
  const hi = Math.ceil(pos);
  return v[lo] + (v[hi] - v[lo]) * (pos - lo);
};

export const mean = (values: number[]): number | null => {
  const v = values.filter(Number.isFinite);
  return v.length ? v.reduce((a, b) => a + b, 0) / v.length : null;
};

/** "6 h 44 min", "45 min". */
export const duration = (minutes: number | null | undefined): string => {
  if (minutes == null || !Number.isFinite(minutes)) return '';
  const m = Math.round(minutes);
  const h = Math.floor(m / 60);
  const rest = m % 60;
  if (!h) return `${rest} min`;
  return rest ? `${h} h ${rest} min` : `${h} h`;
};

export const hoursMinutes = (minutes: number | null | undefined): { h: number; m: number } | null => {
  if (minutes == null || !Number.isFinite(minutes)) return null;
  const total = Math.round(minutes);
  return { h: Math.floor(total / 60), m: total % 60 };
};

const timeFormat = new Intl.DateTimeFormat('en-GB', { hour: 'numeric', minute: '2-digit', hour12: true });
export const clock = (iso: string | null | undefined): string => {
  if (!iso) return '';
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? '' : timeFormat.format(date).replace(' ', ' ').toUpperCase();
};

const dayFormat = new Intl.DateTimeFormat('en-GB', { weekday: 'long', day: 'numeric', month: 'long' });
export const longDay = (date: Date): string => dayFormat.format(date);
const shortDayFormat = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short' });
/** `2026-11-22` → `22 Nov`. */
export const shortDay = (key: string): string => {
  const [y, m, d] = key.split('-').map(Number);
  return shortDayFormat.format(new Date(y, m - 1, d));
};
const weekdayFormat = new Intl.DateTimeFormat('en-GB', { weekday: 'short' });
export const weekday = (key: string): string => {
  const [y, m, d] = key.split('-').map(Number);
  return weekdayFormat.format(new Date(y, m - 1, d));
};

export const dayKey = (date: Date): string =>
  `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;

export const todayKey = (): string => dayKey(new Date());

export const addDaysKey = (key: string, days: number): string => {
  const [y, m, d] = key.split('-').map(Number);
  return dayKey(new Date(y, m - 1, d + days));
};

/** The last `days` local dates, oldest first, ending today. */
export const lastDays = (days: number): string[] => {
  const end = todayKey();
  return Array.from({ length: days }, (_, i) => addDaysKey(end, i - days + 1));
};

/** A series as a date → value map. */
export const byDate = (series: MetricSeries | undefined): Map<string, number> =>
  new Map((series?.points ?? []).filter((p) => Number.isFinite(p.value)).map((p) => [p.date, p.value]));

/** The night belongs to the local day you woke up. */
export const wakeDay = (session: SleepSession): string => dayKey(new Date(session.end_time));

/** One night per wake day: the longest session (naps are left out). */
export const nights = (sessions: SleepSession[]): SleepSession[] => {
  const best = new Map<string, SleepSession>();
  for (const s of sessions) {
    const key = wakeDay(s);
    const current = best.get(key);
    if (!current || s.duration_minutes > current.duration_minutes) best.set(key, s);
  }
  return [...best.values()].sort((a, b) => b.end_time.localeCompare(a.end_time));
};

/** Minutes after 6 PM that sleep started; lets bedtimes around midnight be compared. */
export const bedtimeMinutes = (session: SleepSession): number => {
  const d = new Date(session.start_time);
  const m = d.getHours() * 60 + d.getMinutes();
  return m >= 18 * 60 ? m - 18 * 60 : m + 6 * 60;
};

/** Resting heart rate: Zepp's sleep-summary value when it exists, otherwise the watch's own daily value. */
export const restingSeries = (all: MetricSeries[]): MetricSeries | undefined => {
  const primary = all.find((s) => s.metric === 'resting_hr');
  if (primary && primary.days_with_data > 0) return primary;
  const device = all.find((s) => s.metric === 'device_resting_hr');
  return device && device.days_with_data > 0 ? device : primary;
};

export const latestPoint = (series: MetricSeries | undefined): MetricSeriesPoint | null => {
  const points = series?.points ?? [];
  return points.length ? points[points.length - 1] : null;
};

/** Re-run a loader on mount and after every sync that brought new data. */
export const useReload = (load: () => Promise<void>) => {
  const { dataRevision } = useSyncController();
  const loading = ref(true);
  const error = ref<string | null>(null);
  const run = async () => {
    try {
      error.value = null;
      await load();
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      loading.value = false;
    }
  };
  onMounted(run);
  watch(dataRevision, () => { void run(); });
  return { loading, error, reload: run };
};

export const METRICS_ALL = ['steps', 'resting_hr', 'device_resting_hr', 'sleep_score', 'pai_total', 'pai_daily', 'distance', 'active_calories'];

export const fetchSeries = (metrics: string[], days: number) => backend.getMetricSeries(metrics, days);
export { backend };
