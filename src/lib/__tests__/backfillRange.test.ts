import { describe, expect, it } from 'vitest';
import {
  ALL_HISTORY_DAYS, HISTORY_LIMIT_DAYS, backfillStartDate, defaultCustomFrom, earliestBackfillDate,
  type BackfillChoice,
} from '../backfillRange';
import { daysInMonth, wheelDateValue } from '../wheel/momentum';

const backendAccepts = (from: string, utcToday: string): boolean => {
  const days = (Date.parse(`${utcToday}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / 86_400_000;
  return days >= 0 && days <= HISTORY_LIMIT_DAYS;
};

const iso = (date: Date) => date.toISOString().slice(0, 10);
const shift = (day: string, days: number) => iso(new Date(Date.parse(`${day}T00:00:00Z`) + days * 86_400_000));

describe('backfill start date (#136)', () => {
  const todays = [new Date(2026, 9, 19), new Date(2028, 1, 29), new Date(2026, 0, 1), new Date(2026, 11, 31)];

  it('all five choices give a date the backend accepts, even when local and UTC days differ by one', () => {
    for (const today of todays) {
      const local = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`;
      const choices: BackfillChoice[] = ['1y', '2y', '3y', 'all', 'custom'];
      for (const choice of choices) {
        const from = backfillStartDate(choice, today, defaultCustomFrom(today));
        expect(from, `${choice} @ ${local}`).toMatch(/^\d{4}-\d{2}-\d{2}$/);
        for (const utcToday of [shift(local, -1), local, shift(local, 1)]) {
          expect(backendAccepts(from, utcToday), `${choice} @ ${local} vs UTC ${utcToday}`).toBe(true);
        }
      }
    }
  });

  it('"all" aligns to the day limit, not ten calendar years back', () => {
    const today = new Date(2026, 9, 19);
    expect(backfillStartDate('all', today, '')).toBe(earliestBackfillDate(today));
    expect(ALL_HISTORY_DAYS).toBeLessThan(HISTORY_LIMIT_DAYS);
    expect(backfillStartDate('all', today, '')).not.toBe('2016-10-19');
  });

  it('custom start: empty input gives an empty string (UI asks for a date), too early clamps to the lower bound', () => {
    const today = new Date(2026, 9, 19);
    expect(backfillStartDate('custom', today, '')).toBe('');
    expect(backfillStartDate('custom', today, 'not-a-date')).toBe('');
    expect(backfillStartDate('custom', today, '2001-01-01')).toBe(earliestBackfillDate(today));
    expect(backfillStartDate('custom', today, '2024-05-06')).toBe('2024-05-06');
    expect(defaultCustomFrom(today)).toBe('2025-10-19');
  });
});

describe('wheel date initial value (#136: blank custom start)', () => {
  const columns = (value: string) => {
    const [y, m, d] = value.split('-').map(Number);
    return { years: y! - (y! - 10) + 1, months: 12, days: daysInMonth(y!, m!), d: d! };
  };

  it('empty and garbage values are not dates, and no column is empty', () => {
    for (const model of ['', null, undefined, 'garbage']) {
      const value = wheelDateValue(model, undefined, '2026-10-19');
      expect(value).toBe('2026-10-19');
      const col = columns(value);
      expect(col.years).toBeGreaterThan(0);
      expect(col.days).toBeGreaterThan(0);
      expect(Number.isFinite(col.d)).toBe(true);
    }
  });

  it('falls back to min when there is no value, and uses the value when there is one', () => {
    expect(wheelDateValue('', '2020-01-01', '2026-10-19')).toBe('2020-01-01');
    expect(wheelDateValue('2024-02-29', '2020-01-01', '2026-10-19')).toBe('2024-02-29');
  });
});
