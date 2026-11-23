import { describe, expect, it } from 'vitest';
import { metricTone } from '../metricTone';
import { PAIRED_METRICS } from '../cards/sources';

describe('metric colors', () => {
  it('matches the colors the user checked', () => {
    expect(metricTone('stress')).toBe('calories');
    expect(metricTone('spo2_odi')).toBe('altitude');
    expect(metricTone('training_load')).toBe('training');
    expect(metricTone('resting_hr')).toBe('heart');
    expect(metricTone('hrv')).toBe('stride');
    expect(metricTone('hrv_rmssd')).toBe('sleepLight');
  });

  it('lactate threshold is a pair: heart rate red, pace blue; all-day heart rate is red', () => {
    expect(PAIRED_METRICS.lactate_threshold_hr).toBe('lactate_threshold_pace');
    expect(metricTone('lactate_threshold_hr')).toBe('heart');
    expect(metricTone('lactate_threshold_pace')).toBe('pace');
    expect(metricTone('heart_rate')).toBe('heart');
  });
});
