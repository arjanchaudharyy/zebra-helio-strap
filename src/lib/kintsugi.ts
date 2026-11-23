import type { MonthSeam } from '../types';

export type SeamCell = MonthSeam['state'] | 'unasked' | 'out';

export interface KintsugiCell {
  month: string;
  state: SeamCell;
}

export interface SeamRun {
  kind: 'gold' | 'gap';
  from: number;
  to: number;
  points: Array<[number, number]>;
}

export interface KintsugiYear {
  year: number;
  cells: KintsugiCell[];
  runs: SeamRun[];
  firstGap: string | null;
}

export const isGap = (state: SeamCell): boolean => state === 'crack' || state === 'pending' || state === 'unasked';

const MIN_YEARS = 3;

const pad = (value: number) => String(value).padStart(2, '0');

export const seamHash = (text: string): number => {
  let hash = 2166136261;
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return ((hash >>> 0) % 10_000) / 10_000;
};

const runPoints = (cells: KintsugiCell[], from: number, to: number, kind: SeamRun['kind']): Array<[number, number]> => {
  const amplitude = kind === 'gold' ? 0.22 : 0.3;
  const y = (key: string) => 0.5 + (seamHash(`${kind}:${key}`) - 0.5) * 2 * amplitude;
  const points: Array<[number, number]> = [[from + 0.12, y(`${cells[from].month}<`)]];
  for (let index = from; index <= to; index += 1) {
    points.push([index + 0.5, y(`${cells[index].month}|`)]);
    if (index < to) points.push([index + 1, y(`${cells[index].month}>`)]);
  }
  points.push([to + 0.88, y(`${cells[to].month}>>`)]);
  return points;
};

const runsOf = (cells: KintsugiCell[]): SeamRun[] => {
  const runs: SeamRun[] = [];
  let index = 0;
  while (index < cells.length) {
    const state = cells[index].state;
    const kind: SeamRun['kind'] | null = state === 'gold' ? 'gold' : isGap(state) ? 'gap' : null;
    if (!kind) { index += 1; continue; }
    let end = index;
    while (end + 1 < cells.length) {
      const next = cells[end + 1].state;
      if (kind === 'gold' ? next !== 'gold' : !isGap(next)) break;
      end += 1;
    }
    runs.push({ kind, from: index, to: end, points: runPoints(cells, index, end, kind) });
    index = end + 1;
  }
  return runs;
};

export const kintsugiYears = (months: MonthSeam[], today: Date, earliest: string): KintsugiYear[] => {
  const byMonth = new Map(months.map((item) => [item.month, item.state]));
  const thisYear = today.getFullYear();
  const thisMonth = `${thisYear}-${pad(today.getMonth() + 1)}`;
  const floorMonth = earliest.slice(0, 7);
  const floorYear = Number(earliest.slice(0, 4)) || thisYear;
  const known = months.map((item) => Number(item.month.slice(0, 4))).filter(Number.isFinite);
  const firstYear = Math.max(floorYear, Math.min(thisYear - (MIN_YEARS - 1), ...known));
  const years: KintsugiYear[] = [];
  for (let year = thisYear; year >= firstYear; year -= 1) {
    const cells: KintsugiCell[] = [];
    for (let m = 1; m <= 12; m += 1) {
      const month = `${year}-${pad(m)}`;
      const state: SeamCell = month > thisMonth || month < floorMonth ? 'out' : byMonth.get(month) ?? 'unasked';
      cells.push({ month, state });
    }
    const gap = cells.find((cell) => isGap(cell.state));
    years.push({ year, cells, runs: runsOf(cells), firstGap: gap?.month ?? null });
  }
  return years;
};

export const seamPolyline = (points: Array<[number, number]>): string =>
  points.map(([x, y]) => `${x.toFixed(3)},${y.toFixed(3)}`).join(' ');

export const healingSpans = (before: ReadonlySet<number> | undefined, gold: readonly number[]): Array<[number, number]> => {
  if (!before) return [];
  const fresh = gold.filter((index) => !before.has(index)).sort((a, b) => a - b);
  const spans: Array<[number, number]> = [];
  for (const index of fresh) {
    const last = spans[spans.length - 1];
    if (last && index === last[1] + 1) last[1] = index;
    else spans.push([index, index]);
  }
  return spans;
};

export const healingMs = (span: readonly [number, number]): number => Math.max(420, Math.min(900, 140 * (span[1] - span[0] + 1)));
