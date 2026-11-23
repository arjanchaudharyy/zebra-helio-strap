import type { MetricSeries } from '../types';
import { formatMetric } from './format';
import { bodyMassUnitLabel, distanceUnitLabel, toBigDistance, toBodyMass } from './units';

export type PinGroup = 'recovery' | 'activity' | 'body';

export type PinUnit = 'bpm' | 'ms' | 'score' | 'percent' | 'steps' | 'distance' | 'kcal' | 'min' | 'mass' | 'vo2' | 'pai' | '';

export interface PinnableMetric {
  id: string;
  group: PinGroup;
  route: string;
  focus?: string | null;
  digits: number;
  unit: PinUnit;
}

export const PINNABLE_METRICS: readonly PinnableMetric[] = [
  { id: 'resting_hr', group: 'recovery', route: '/heart', digits: 0, unit: 'bpm' },
  { id: 'hrv_rmssd', group: 'recovery', route: '/heart', digits: 0, unit: 'ms' },
  { id: 'hrv', group: 'recovery', route: '/heart', digits: 0, unit: 'ms' },
  { id: 'sleep_score', group: 'recovery', route: '/sleep', focus: null, digits: 0, unit: 'score' },
  { id: 'readiness', group: 'recovery', route: '/body', digits: 0, unit: 'score' },
  { id: 'stress', group: 'recovery', route: '/body', digits: 0, unit: 'score' },
  { id: 'spo2', group: 'recovery', route: '/body', digits: 0, unit: 'percent' },
  { id: 'steps', group: 'activity', route: '/activity', digits: 0, unit: 'steps' },
  { id: 'distance', group: 'activity', route: '/activity', digits: 2, unit: 'distance' },
  { id: 'active_calories', group: 'activity', route: '/activity', digits: 0, unit: 'kcal' },
  { id: 'active_minutes', group: 'activity', route: '/activity', digits: 0, unit: 'min' },
  { id: 'training_load', group: 'activity', route: '/training', digits: 0, unit: '' },
  { id: 'vo2max', group: 'activity', route: '/training', digits: 1, unit: 'vo2' },
  { id: 'pai_total', group: 'activity', route: '/training', digits: 0, unit: 'pai' },
  { id: 'weight', group: 'body', route: '/body', digits: 1, unit: 'mass' },
  { id: 'body_fat_rate', group: 'body', route: '/body', digits: 1, unit: 'percent' },
  { id: 'bmi', group: 'body', route: '/body', digits: 1, unit: '' },
];

const RENAMED: Record<string, string> = { sleep_hrv: 'hrv_rmssd' };

export const MAX_PINS = 4;

const BY_ID = new Map(PINNABLE_METRICS.map((metric) => [metric.id, metric]));

export const pinHref = (metric: PinnableMetric): string => {
  const focus = metric.focus === undefined ? metric.id : metric.focus;
  return focus ? `${metric.route}?focus=${encodeURIComponent(focus)}` : metric.route;
};
export const pinnableMetric = (id: string): PinnableMetric | undefined => BY_ID.get(id);

export const normalizePins = (value: unknown): string[] => {
  if (!Array.isArray(value)) return [];
  const out: string[] = [];
  for (const item of value) {
    const id = typeof item === 'string' ? RENAMED[item] ?? item : null;
    if (!id || !BY_ID.has(id) || out.includes(id)) continue;
    out.push(id);
    if (out.length === MAX_PINS) break;
  }
  return out;
};

const STORAGE_KEY = 'zeppbridge-overview-pins';

export const DEFAULT_PINS: readonly string[] = ['resting_hr', 'hrv_rmssd', 'sleep_score', 'steps'];

export const pinsFromStored = (stored: string | null): string[] => {
  if (stored === null) return [...DEFAULT_PINS];
  try {
    return normalizePins(JSON.parse(stored));
  } catch {
    return [...DEFAULT_PINS];
  }
};

export const readPins = (): string[] => {
  try {
    return pinsFromStored(window.localStorage.getItem(STORAGE_KEY));
  } catch {
    return [...DEFAULT_PINS];
  }
};

export const writePins = (pins: string[]): void => {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(normalizePins(pins)));
  } catch {
  }
};

const shown = (metric: PinnableMetric, value: number) => {
  if (metric.unit === 'mass') return toBodyMass(value);
  if (metric.unit === 'distance') return toBigDistance(value);
  return value;
};

export const pinValueText = (metric: PinnableMetric, series?: MetricSeries | null): string => {
  const value = series?.latest?.value;
  return typeof value === 'number' && Number.isFinite(value) ? formatMetric(shown(metric, value), metric.digits) : '—';
};

export const pinNumberText = (metric: PinnableMetric, value: number): string => formatMetric(shown(metric, value), metric.digits);

export const pinMassUnit = (): string => bodyMassUnitLabel();

export const pinDistanceUnit = (): string => distanceUnitLabel();

export const pinLatestDate = (series?: MetricSeries | null): string | null => series?.latest?.date ?? null;

export const pinSparkValues = (series?: MetricSeries | null): number[] =>
  (series?.points ?? []).map((point) => point.value).filter((value) => Number.isFinite(value));
