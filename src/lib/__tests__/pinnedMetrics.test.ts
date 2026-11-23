import { describe, expect, it } from 'vitest';
import { DEFAULT_PINS, MAX_PINS, normalizePins, pinDistanceUnit, pinnableMetric, pinsFromStored, pinSparkValues, pinValueText } from '../pinnedMetrics';
import { setDistanceUnit } from '../units';

describe('pinnedMetrics', () => {
  it('gives four default pins when none were picked; keeps an empty list the user cleared', () => {
    expect(pinsFromStored(null)).toEqual([...DEFAULT_PINS]);
    expect(DEFAULT_PINS).toHaveLength(MAX_PINS);
    expect(normalizePins(DEFAULT_PINS)).toEqual([...DEFAULT_PINS]);
    expect(pinsFromStored('[]')).toEqual([]);
    expect(pinsFromStored('["steps"]')).toEqual(['steps']);
  });

  it('keeps known metrics only, removes duplicates, keeps at most four, in the user order', () => {
    expect(normalizePins(['weight', 'nope', 'resting_hr', 'weight', 'steps', 'stress', 'spo2'])).toEqual(['weight', 'resting_hr', 'steps', 'stress']);
    expect(normalizePins('weight')).toEqual([]);
    expect(normalizePins(null)).toHaveLength(0);
    expect(MAX_PINS).toBe(4);
  });

  it('replaces the old "sleep HRV" with HRV (RMSSD) without a duplicate', () => {
    expect(normalizePins(['sleep_hrv', 'steps'])).toEqual(['hrv_rmssd', 'steps']);
    expect(normalizePins(['hrv_rmssd', 'sleep_hrv'])).toEqual(['hrv_rmssd']);
  });

  it('shows a dash when there is no record, not 0', () => {
    const metric = pinnableMetric('resting_hr')!;
    expect(pinValueText(metric, null)).toBe('\u2014');
    expect(pinValueText(metric, { latest: null } as never)).toBe('\u2014');
    expect(pinSparkValues(undefined)).toEqual([]);
  });

  it('shows weight with one decimal', () => {
    const metric = pinnableMetric('weight')!;
    expect(pinValueText(metric, { latest: { date: '2026-09-27', value: 68.25 } } as never)).toMatch(/^68[.,]3$/);
  });

  it('converts distance from meters to km or mi in the user unit (at most two decimals)', () => {
    const metric = pinnableMetric('distance')!;
    try {
      setDistanceUnit('metric');
      expect(pinValueText(metric, { latest: { date: '2026-09-27', value: 5326 } } as never)).toMatch(/^5[.,]33$/);
      expect(pinDistanceUnit()).toBeTruthy();
      setDistanceUnit('imperial');
      expect(pinValueText(metric, { latest: { date: '2026-09-27', value: 5326 } } as never)).toMatch(/^3[.,]31$/);
    } finally {
      setDistanceUnit('metric');
    }
  });
});
