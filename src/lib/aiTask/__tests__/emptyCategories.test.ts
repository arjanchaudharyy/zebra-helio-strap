import { describe, expect, it } from 'vitest';
import { emptyCategories } from '../emptyCategories';
import type { DayStripRow } from '../../../types/timeBridge';

const cell = (date: string, has: boolean, value: number | null = null) => ({ date, has, value, unit: null, workout_ids: [] });
const row = (category: DayStripRow['category'], cells: ReturnType<typeof cell>[]): DayStripRow => ({ category, metric: null, cells } as DayStripRow);

describe('categories with no records on any day', () => {
  it('a row is empty only with no records and no readings; one day with a record is not empty', () => {
    const rows = [
      row('body', [cell('2026-10-01', false), cell('2026-10-02', false)]),
      row('sleep', [cell('2026-10-01', false), cell('2026-10-02', true, 420)]),
      row('workout', [cell('2026-10-01', true)]),
    ];
    expect([...emptyCategories(rows)]).toEqual(['body']);
  });

  it('a strip not loaded yet (no cells) is not empty: do not turn a category off because it has not loaded', () => {
    expect(emptyCategories([row('body', [])]).size).toBe(0);
  });
});
