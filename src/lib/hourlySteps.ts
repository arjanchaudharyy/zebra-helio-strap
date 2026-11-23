import type { HourlySteps } from '../types';
import { localDateString } from './format';
import { today as currentToday } from './currentDay';

export interface HourRow {
  start: string;
  end: string;
  covered: number;
  cells: Array<number | null>;
}

const addDays = (date: string, days: number): string => {
  const [y, m, d] = date.split('-').map(Number);
  return localDateString(new Date(y!, m! - 1, d! + days));
};

export const dateSpan = (start: string, end: string): string[] => {
  const out: string[] = [];
  for (let day = start; day <= end && out.length < 1000; day = addDays(day, 1)) out.push(day);
  return out;
};

export const rangeBounds = (days: number, today = currentToday()): { start: string; end: string } => {
  const end = localDateString(today);
  return { start: addDays(end, -(Math.max(1, days) - 1)), end };
};

const indexByDate = (rows: HourlySteps[]) => {
  const byDate = new Map<string, Map<number, number>>();
  for (const row of rows) {
    const hours = byDate.get(row.date) ?? new Map<number, number>();
    hours.set(row.hour, row.steps);
    byDate.set(row.date, hours);
  }
  return byDate;
};

const rowOf = (dates: string[], byDate: Map<string, Map<number, number>>): HourRow => {
  const days = dates.map((date) => byDate.get(date)).filter((hours): hours is Map<number, number> => Boolean(hours?.size));
  const cells = Array.from({ length: 24 }, (_, hour) => {
    if (!days.some((hours) => hours.has(hour))) return null;
    return days.reduce((sum, hours) => sum + (hours.get(hour) ?? 0), 0) / days.length;
  });
  return { start: dates[0]!, end: dates[dates.length - 1]!, covered: days.length, cells };
};

export const hourRows = (rows: HourlySteps[], start: string, end: string, perWeek: boolean): HourRow[] => {
  const byDate = indexByDate(rows);
  const dates = dateSpan(start, end).reverse();
  const size = perWeek ? 7 : 1;
  const out: HourRow[] = [];
  for (let index = 0; index < dates.length; index += size) {
    out.push(rowOf(dates.slice(index, index + size).reverse(), byDate));
  }
  return out;
};

export const averageRow = (rows: HourlySteps[], start: string, end: string): HourRow =>
  rowOf(dateSpan(start, end), indexByDate(rows));
