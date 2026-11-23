import { addDays } from '../aiTask/bridgeScale';

export interface DeckDay {
  date: string;
  value: number | null;
  has: boolean;
  id?: string;
  pickKey?: string;
  icon?: 'run' | 'bike' | 'activity';
  text?: string;
  value2?: number | null;
}

export const WORKOUT_PREFIX = 'workout:';

export const cardIdOf = (day: DeckDay): string => day.id ?? day.date;

export type GroupKind = 'week' | 'month' | 'quarter' | 'year';

export interface DeckGroup {
  id: string;
  kind: GroupKind;
  start: string;
  end: string;
  days: DeckDay[];
}

export type DeckLevel =
  | { kind: 'days'; days: DeckDay[] }
  | { kind: 'groups'; groups: DeckGroup[] };

export type DeckRange = 7 | 30 | 180 | 365 | 3650;

export const DECK_RANGES: readonly DeckRange[] = [7, 30, 180, 365, 3650];

export const ALL_YEARS = 10;

export const deckRangeOf = (days: number): DeckRange => (days <= 7 ? 7 : days <= 31 ? 30 : days <= 186 ? 180 : days <= 366 ? 365 : 3650);

const weekdayOf = (date: string): number => {
  const [y, m, d] = date.split('-').map(Number);
  return (new Date(y!, m! - 1, d!).getDay() + 6) % 7;
};

export const daysBetweenInclusive = (start: string, end: string, lookup: (date: string) => { value: number | null; has: boolean } | null): DeckDay[] => {
  const out: DeckDay[] = [];
  for (let day = start; day <= end; day = addDays(day, 1)) {
    const found = lookup(day);
    out.push({ date: day, value: found?.value ?? null, has: found?.has ?? false });
  }
  return out;
};

const firstOfMonth = (year: number, monthIndex: number): string => {
  const first = new Date(year, monthIndex, 1);
  return `${first.getFullYear()}-${String(first.getMonth() + 1).padStart(2, '0')}-01`;
};

export const rangeStart = (today: string, range: DeckRange): string => {
  const [y, m] = today.split('-').map(Number);
  if (range === 180) return firstOfMonth(y!, m! - 1 - 5);
  if (range === 365) return firstOfMonth(y!, Math.floor((m! - 1) / 3) * 3 - 9);
  if (range === 3650) return `${y! - (ALL_YEARS - 1)}-01-01`;
  return addDays(today, -(range - 1));
};

export const weeksOf = (days: DeckDay[], minDays = 3): DeckGroup[] => {
  const groups: DeckGroup[] = [];
  for (const day of days) {
    const monday = addDays(day.date, -weekdayOf(day.date));
    const last = groups[groups.length - 1];
    if (last && last.id === `w:${monday}`) last.days.push(day);
    else groups.push({ id: `w:${monday}`, kind: 'week', start: day.date, end: day.date, days: [day] });
  }
  if (groups.length > 1 && groups[0]!.days.length < minDays) {
    const [head, next] = groups.splice(0, 2) as [DeckGroup, DeckGroup];
    groups.unshift({ ...next, start: head.start, days: [...head.days, ...next.days] });
  }
  for (const group of groups) {
    group.start = group.days[0]!.date;
    group.end = group.days[group.days.length - 1]!.date;
  }
  return groups;
};

const groupBy = (days: DeckDay[], kind: GroupKind, keyOf: (date: string) => string): DeckGroup[] => {
  const groups: DeckGroup[] = [];
  for (const day of days) {
    const id = keyOf(day.date);
    const last = groups[groups.length - 1];
    if (last && last.id === id) { last.days.push(day); last.end = day.date; }
    else groups.push({ id, kind, start: day.date, end: day.date, days: [day] });
  }
  return groups;
};

export const quarterOf = (date: string): number => Math.floor((Number(date.slice(5, 7)) - 1) / 3) + 1;

export const monthsOf = (days: DeckDay[]): DeckGroup[] => groupBy(days, 'month', (date) => `m:${date.slice(0, 7)}`);

export const quartersOf = (days: DeckDay[]): DeckGroup[] => groupBy(days, 'quarter', (date) => `q:${date.slice(0, 4)}-${quarterOf(date)}`);

export const yearsOf = (days: DeckDay[]): DeckGroup[] => groupBy(days, 'year', (date) => `y:${date.slice(0, 4)}`);

export const rootLevel = (days: DeckDay[], range: DeckRange): DeckLevel => {
  if (range === 7) return { kind: 'days', days };
  const groups = range === 30 ? weeksOf(days) : range === 180 ? monthsOf(days) : range === 365 ? quartersOf(days) : yearsOf(days);
  return { kind: 'groups', groups };
};

export const expandGroup = (group: DeckGroup): DeckLevel => {
  if (group.kind === 'year') return { kind: 'groups', groups: quartersOf(group.days) };
  if (group.kind === 'quarter') return { kind: 'groups', groups: monthsOf(group.days) };
  if (group.kind === 'month') return { kind: 'groups', groups: weeksOf(group.days, 1) };
  return { kind: 'days', days: group.days };
};

export const groupDots = (group: DeckGroup, picked: (date: string) => boolean): Array<boolean | 'picked'> => {
  const dot = (days: DeckDay[]): boolean | 'picked' => (days.some((day) => day.has && picked(day.date)) ? 'picked' : days.some((day) => day.has));
  if (group.kind === 'week' || group.kind === 'month') return group.days.map((day) => dot([day]));
  return monthsOf(group.days).map((month) => dot(month.days));
};

export const spanBetween = <T>(items: readonly T[], idOf: (item: T) => string, from: string, to: string): T[] => {
  const a = items.findIndex((item) => idOf(item) === from);
  const b = items.findIndex((item) => idOf(item) === to);
  if (a < 0 || b < 0) return [];
  return items.slice(Math.min(a, b), Math.max(a, b) + 1);
};

export type GroupCoverage = 'not_backfilled' | 'cloud_empty' | null;

export const groupSummary = (group: DeckGroup): { recorded: number; total: number; average: number | null } => {
  const values = group.days.map((day) => day.value).filter((value): value is number => value !== null && Number.isFinite(value));
  return {
    recorded: group.days.filter((day) => day.has).length,
    total: group.days.length,
    average: values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : null,
  };
};

export const pickableDates = (group: DeckGroup): string[] => group.days.filter((day) => day.has).map((day) => day.date);

export const focusWindow = (focus: string, today: string, size = 7): [string, string] => {
  const half = Math.floor(size / 2);
  let end = addDays(focus, size - 1 - half);
  if (end > today) end = today;
  return [addDays(end, -(size - 1)), end];
};
