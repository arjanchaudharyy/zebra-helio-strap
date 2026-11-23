import { isFiniteNumber } from './format';

export const finiteOrNull = (value: unknown): number | null =>
  isFiniteNumber(value) ? value : null;

export const stageMinutesForBar = (minutes: unknown): number | null =>
  finiteOrNull(minutes);

export const minutesToHours = (minutes?: number | null): number | null => {
  const value = finiteOrNull(minutes);
  return value === null ? null : Math.round((value / 60) * 10) / 10;
};

export const coveredWindowValue = (
  value: number,
  daysWithData: number,
): number | null => (daysWithData > 0 ? finiteOrNull(value) : null);
