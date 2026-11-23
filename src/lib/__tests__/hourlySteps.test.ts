import { describe, expect, it } from 'vitest';
import { averageRow, hourRows, rangeBounds } from '../hourlySteps';

const rows = [
  { date: '2026-09-27', hour: 8, steps: 400 },
  { date: '2026-09-29', hour: 8, steps: 200 },
  { date: '2026-09-29', hour: 20, steps: 1000 },
];

describe('hourlySteps', () => {
  it('by day: a missing hour is empty, not 0, and a day with no data is an empty row', () => {
    const [today, yesterday] = hourRows(rows, '2026-09-27', '2026-09-29', false);
    expect(today!.start).toBe('2026-09-29');
    expect(today!.cells[8]).toBe(200);
    expect(today!.cells[9]).toBeNull();
    expect(yesterday!.covered).toBe(0);
    expect(yesterday!.cells.every((cell) => cell === null)).toBe(true);
  });

  it('the average counts only days with records, and a missing hour on such a day counts as 0', () => {
    const avg = averageRow(rows, '2026-09-27', '2026-09-29');
    expect(avg.covered).toBe(2);
    expect(avg.cells[8]).toBe(300);
    expect(avg.cells[20]).toBe(500);
    expect(avg.cells[3]).toBeNull();
  });

  it('by week: cuts back from the last day', () => {
    const weeks = hourRows(rows, '2026-09-20', '2026-09-29', true);
    expect(weeks.map((row) => [row.start, row.end])).toEqual([['2026-09-23', '2026-09-29'], ['2026-09-20', '2026-09-22']]);
  });

  it('the range includes today', () => {
    expect(rangeBounds(7, new Date(2026, 8, 29))).toEqual({ start: '2026-09-23', end: '2026-09-29' });
  });
});
