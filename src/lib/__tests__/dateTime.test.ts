import { afterEach, describe, expect, it } from 'vitest';
import { displayDateTimeFormatter, parseDisplayDate, regionDateOrder, setDateOrder, setSystemRegion, setTimeFormat } from '../dateTime';
import { formatTime } from '../format';

afterEach(() => {
  setDateOrder('regional');
  setTimeFormat('regional');
});
describe('local display policy', () => {
  it('keeps a calendar date at local midnight and rejects impossible dates', () => {
    const date = parseDisplayDate('2026-09-12');
    expect([date.getFullYear(), date.getMonth(), date.getDate(), date.getHours()]).toEqual([2026, 8, 12, 0]);
    expect(parseDisplayDate('2026-02-30').getTime()).toBeNaN();
  });
  it('uses the same hour cycle for clocks and date/time displays', () => {
    setTimeFormat('24h');
    expect(formatTime(new Date(2026, 8, 12, 0, 15).toISOString())).toBe('00:15');
    setTimeFormat('12h');
    const parts = displayDateTimeFormatter({ hour: '2-digit', minute: '2-digit', hour12: false }).formatToParts(new Date(2026, 8, 12, 13, 15));
    expect(parts.find(p => p.type === 'hour')?.value).toBe('01');
    expect(parts.some(p => p.type === 'dayPeriod')).toBe(true);
  });
  it('respects explicit date order without moving the date', () => {
    setDateOrder('ymd');
    expect(displayDateTimeFormatter({ year: 'numeric', month: 'short', day: 'numeric' }).format(parseDisplayDate('2026-09-12'))).toBe('2026-09-12');
  });
  it('weekday names are in English', () => {
    setDateOrder('regional');
    const monday = new Date(2026, 0, 5);
    expect(displayDateTimeFormatter({ weekday: 'long' }).format(monday).toLowerCase()).toContain('monday');
  });
  it.each([
    ['Europe/London', '2026-03-29T00:30:00Z', '00:30'],
    ['Europe/London', '2026-03-29T01:30:00Z', '02:30'],
    ['Europe/London', '2026-10-25T01:30:00Z', '01:30'],
    ['America/New_York', '2026-03-08T06:30:00Z', '01:30'],
    ['America/New_York', '2026-03-08T07:30:00Z', '03:30'],
    ['Asia/Shanghai', '2026-03-29T01:30:00Z', '09:30'],
  ])('renders absolute timestamps in %s at %s', (timeZone, input, expected) => {
    setTimeFormat('24h');
    expect(displayDateTimeFormatter({ timeZone, hour: '2-digit', minute: '2-digit' }).format(parseDisplayDate(input))).toBe(expected);
  });
});

describe('"follow system region" follows the OS region format', () => {
  afterEach(() => {
    setSystemRegion(null);
    setDateOrder('regional');
    setTimeFormat('regional');
  });
  const day = new Date(2026, 9, 19, 16, 5);
  const numeric = { year: 'numeric', month: '2-digit', day: '2-digit' } as const;
  it('English UI with a Portugal region: day/month/year and 24-hour clock', () => {
    setSystemRegion('pt_PT.UTF-8');
    const date = displayDateTimeFormatter(numeric).format(day);
    expect(date.indexOf('19')).toBeLessThan(date.indexOf('10'));
    expect(displayDateTimeFormatter({ hour: 'numeric', minute: '2-digit' }).format(day)).toBe('16:05');
  });
  it('weekday names stay in English with a German region', () => {
    setSystemRegion('de-DE');
    expect(displayDateTimeFormatter({ weekday: 'long' }).format(day)).toBe('Monday');
  });
  it('keeps the order when region and UI agree, and falls back to the UI language with no region', () => {
    setSystemRegion('en-US');
    expect(displayDateTimeFormatter(numeric).format(day)).toBe('10/19/2026');
    setSystemRegion(null);
    expect(displayDateTimeFormatter(numeric).format(day)).toBe('10/19/2026');
  });
  it('a manual date order and clock override the system region', () => {
    setSystemRegion('pt-PT');
    setDateOrder('mdy');
    setTimeFormat('12h');
    expect(displayDateTimeFormatter(numeric).format(day)).toBe('10/19/2026');
    expect(displayDateTimeFormatter({ hour: 'numeric', minute: '2-digit' }).format(day)).toMatch(/4:05/);
  });
  it('detects the date order from a region format', () => {
    expect(regionDateOrder('en-US')).toBe('mdy');
    expect(regionDateOrder('pt-PT')).toBe('dmy');
    expect(regionDateOrder('zh-CN')).toBe('ymd');
  });
});
