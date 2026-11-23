import { describe, expect, it } from 'vitest';
import { healingMs, healingSpans, kintsugiYears } from '../kintsugi';

const today = new Date(2026, 9, 21);

describe('kintsugiYears', () => {
  it('shows three full years with only two local months, unasked months are gaps, and "fill it" starts at the earliest cell', () => {
    const years = kintsugiYears([
      { month: '2026-09', state: 'local' },
      { month: '2026-10', state: 'local' },
    ], today, '2016-10-23');
    expect(years.map((year) => year.year)).toEqual([2026, 2025, 2024]);
    expect(years[0].cells[10].state).toBe('out');
    expect(years[0].cells[7].state).toBe('unasked');
    expect(years[2].firstGap).toBe('2024-01');
  });

  it('a month the cloud says is empty is not a gap and gets no "fill it"', () => {
    const years = kintsugiYears(
      Array.from({ length: 12 }, (_, index) => ({ month: `2024-${String(index + 1).padStart(2, '0')}`, state: 'empty' as const })),
      today,
      '2016-10-23',
    );
    const y2024 = years.find((year) => year.year === 2024)!;
    expect(y2024.firstGap).toBeNull();
    expect(y2024.runs).toEqual([]);
  });

  it('adjacent gold joins into one run, gaps and gold never share a run, and the same gap draws the same each time', () => {
    const months = [
      { month: '2025-03', state: 'gold' as const },
      { month: '2025-04', state: 'gold' as const },
      { month: '2025-05', state: 'crack' as const },
      { month: '2025-06', state: 'local' as const },
    ];
    const a = kintsugiYears(months, today, '2016-10-23').find((year) => year.year === 2025)!;
    const b = kintsugiYears(months, today, '2016-10-23').find((year) => year.year === 2025)!;
    const gold = a.runs.find((run) => run.kind === 'gold')!;
    expect([gold.from, gold.to]).toEqual([2, 3]);
    expect(a.runs.find((run) => run.from === 4)?.kind).toBe('gap');
    expect(a.runs).toEqual(b.runs);
  });

  it('months before the backfill limit are not drawn (they cannot be filled, so they are not gaps)', () => {
    const years = kintsugiYears([{ month: '2016-12', state: 'gold' }], today, '2016-10-23');
    const first = years[years.length - 1];
    expect(first.year).toBe(2016);
    expect(first.cells[8].state).toBe('out');
    expect(first.cells[9].state).toBe('unasked');
  });
});

describe('healingSpans', () => {
  it('does not animate the first time a year shows', () => {
    expect(healingSpans(undefined, [0, 1, 2])).toEqual([]);
  });

  it('newly filled adjacent months join into one span, non-adjacent ones split', () => {
    expect(healingSpans(new Set([0]), [0, 3, 4, 5, 8])).toEqual([[3, 5], [8, 8]]);
  });

  it('does not animate when gold shrinks or stays the same', () => {
    expect(healingSpans(new Set([2, 3]), [2])).toEqual([]);
    expect(healingSpans(new Set([2, 3]), [3, 2])).toEqual([]);
  });

  it('duration scales with months, clamped to 420-900ms', () => {
    expect(healingMs([4, 4])).toBe(420);
    expect(healingMs([0, 4])).toBe(700);
    expect(healingMs([0, 11])).toBe(900);
  });
});
