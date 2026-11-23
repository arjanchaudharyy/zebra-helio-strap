import { describe, expect, it } from 'vitest';
import {
  formatBytes,
  formatDate,
  formatDistance,
  formatDuration,
  formatMetric,
  isFiniteNumber,
  localDateString,
} from '../format';


describe('missing values never show as 0', () => {
  it('treats undefined, NaN and Infinity as not displayable', () => {
    expect(isFiniteNumber(undefined)).toBe(false);
    expect(isFiniteNumber(Number.NaN)).toBe(false);
    expect(isFiniteNumber(Number.POSITIVE_INFINITY)).toBe(false);
    expect(isFiniteNumber(0)).toBe(true);
  });

  it('shows a placeholder, not 0, for a metric with no value', () => {
    expect(formatMetric(undefined)).toBe('\u2014');
    expect(formatMetric(Number.NaN)).toBe('\u2014');
  });

  it('says not recorded, not 0 m, when there is no distance', () => {
    expect(formatDistance(undefined)).toBe('Not recorded');
    expect(formatDistance(0)).toBe('Not recorded');
  });

  it('says unknown, not 0 min, when there is no duration', () => {
    expect(formatDuration(undefined)).toBe('Duration unknown');
    expect(formatDuration(null)).toBe('Duration unknown');
    expect(formatDuration(-3)).toBe('Duration unknown');
  });

  it('still shows 0 when the value is a real 0', () => {
    expect(formatMetric(0)).toBe('0');
    expect(formatDuration(0)).toBe('0 min');
  });
});

describe('unit conversion', () => {
  it('uses km from one kilometre up and m below', () => {
    expect(formatDistance(999)).toBe('999 m');
    expect(formatDistance(1000)).toBe('1.00 km');
    expect(formatDistance(5432)).toBe('5.43 km');
  });

  it('splits a duration into hours and minutes past one hour', () => {
    expect(formatDuration(45)).toBe('45 min');
    expect(formatDuration(60)).toBe('1 hr 0 min');
    expect(formatDuration(125)).toBe('2 hr 5 min');
  });

});

describe('local date string', () => {
  it('uses the local date, not the UTC date', () => {
    const localMidnight = new Date(2026, 0, 1, 0, 30);
    expect(localDateString(localMidnight)).toBe('2026-01-01');
  });

  it('pads month and day to two digits', () => {
    expect(localDateString(new Date(2026, 8, 5, 12))).toBe('2026-09-05');
  });
});

describe('formatBytes', () => {
  it('B / whole KB / 1-decimal MB / 2-decimal GB; empty for missing', () => {
    expect(formatBytes(null)).toBe('\u2014');
    expect(formatBytes(Number.NaN)).toBe('\u2014');
    expect(formatBytes(0)).toBe('\u2014');
    expect(formatBytes(0, '0 KB')).toBe('0 KB');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(1023)).toBe('1023 B');
    expect(formatBytes(1024)).toBe('1 KB');
    expect(formatBytes(2048)).toBe('2 KB');
    expect(formatBytes(3 * 1024 * 1024)).toBe('3.0 MB');
    expect(formatBytes(1024 * 1024 * 1024)).toBe('1.00 GB');
  });
});

describe('dates show the year only when needed', () => {
  const now = new Date(2026, 0, 1, 9, 0);

  it('leaves the year out of a date in this year in lists', () => {
    expect(formatDate('2026-01-01T08:00:00', 'auto', now)).not.toContain('2026');
  });

  it('adds the year to the last day of last year', () => {
    expect(formatDate('2025-12-31T20:00:00', 'auto', now)).toContain('2025');
  });

  it('always adds the year in the detail header; short still leaves it out', () => {
    expect(formatDate('2026-01-01T08:00:00', 'dated', now)).toContain('2026');
    expect(formatDate('2024-06-01T08:00:00', 'short', now)).not.toContain('2024');
  });
});
