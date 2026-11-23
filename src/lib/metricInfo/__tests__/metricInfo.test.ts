import { describe, expect, it } from 'vitest';
import { METRIC_INFO_IDS, metricInfo } from '../index';
import { metricTrust } from '../trust';
import { METRICS as BODY_METRICS } from '../../../views/body/bodyCards';


const HEART_PAGE_METRICS = ['resting_hr', 'hrv', 'hrv_rmssd', 'heart_rate_24h', 'daily_max_hr'];
const TRAINING_PAGE_METRICS = ['vo2max', 'training_load', 'pai_total', 'lactate_threshold', 'training_balance'];
const ACTIVITY_PAGE_METRICS = ['steps', 'distance', 'active_calories', 'active_minutes'];
const SLEEP_PAGE_METRICS = ['sleep_duration', 'sleep_score', 'sleep_weekly', 'sleep_stages'];

const PAGE_METRICS = [
  ...BODY_METRICS,
  ...HEART_PAGE_METRICS,
  ...TRAINING_PAGE_METRICS,
  ...ACTIVITY_PAGE_METRICS,
  ...SLEEP_PAGE_METRICS,
];

const CHINESE = /[\u4e00-\u9fff]/;

describe('metric info registry', () => {
  it('every id used by a card has all three layers in English', () => {
    for (const id of PAGE_METRICS) {
      const info = metricInfo(id);
      expect(info, `${id} not in the registry`).not.toBeNull();
      for (const layer of ['what', 'chart', 'how'] as const) {
        const text = info?.[layer];
        expect(typeof text, `${id}.${layer}`).toBe('string');
        expect((text ?? '').length, `${id}.${layer}`).toBeGreaterThan(0);
        expect(text, `${id}.${layer}`).not.toMatch(CHINESE);
      }
    }
  });

  it('every registered id itself has all three layers (no empty entries)', () => {
    for (const id of METRIC_INFO_IDS) {
      const info = metricInfo(id);
      expect(info, id).not.toBeNull();
      expect(info?.what.length, `${id}.what`).toBeGreaterThan(0);
      expect(info?.chart.length, `${id}.chart`).toBeGreaterThan(0);
      expect(info?.how.length, `${id}.how`).toBeGreaterThan(0);
    }
  });

  it('every registered id says how far it can be trusted; only black-box models like VO₂max are flagged', () => {
    for (const id of METRIC_INFO_IDS) expect(metricTrust(id), id).not.toBeNull();
    expect(metricTrust('vo2max')).toBe('modeled');
    expect(metricTrust('steps')).toBe('measured');
    for (const id of ['sleep_duration', 'sleep_stages', 'sleep_score', 'distance']) expect(metricTrust(id), id).toBe('inferred');
  });

  it('an unknown id renders no button at all', () => {
    expect(metricInfo('not_a_metric')).toBeNull();
    expect(metricInfo('')).toBeNull();
  });
});
