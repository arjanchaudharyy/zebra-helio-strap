import { afterEach, describe, expect, it } from 'vitest';
import type { MetricSeries } from '../../types';
import {
  buildSeriesOption,
  coverageLabel,
  formatPaceSeconds,
  indexSeries,
  latestValue,
  sliceSeries,
} from '../metricSeries';
import { setDateOrder } from '../dateTime';

const series = (overrides: Partial<MetricSeries> = {}): MetricSeries =>
  ({
    metric: 'resting_hr',
    unit: 'bpm',
    window_days: 180,
    days_with_data: 12,
    points: [
      { date: '2026-01-01', value: 52 },
      { date: '2026-06-01', value: 55 },
    ],
    latest: { date: '2026-06-01', value: 55 },
    ...overrides,
  }) as MetricSeries;

describe('coverage is stated, not smoothed away by the curve', () => {
  it('keeps measured blood pressure min and max after a range change, not daily average extremes', () => {
    const bp = series({ metric: 'blood_pressure_systolic', points: [
      { date: '2026-06-01', value: 111, min: 106, max: 126 },
      { date: '2026-05-31', value: 110, min: 105, max: 130 },
    ] });
    const cut = sliceSeries(bp, 1, new Date(2026, 5, 1, 12));
    expect(cut.minimum).toBe(106);
    expect(cut.maximum).toBe(126);
    expect(cut.average).toBe(111);
  });
  it('states how many days in the window have records', () => {
    expect(coverageLabel(series())).toBe('12 of 180 days have records');
  });

  it('says no records when no day has a record', () => {
    expect(coverageLabel(series({ days_with_data: 0 }))).toBe('No records in the last 180 days');
  });

  it('does not claim a sync state when there is no series', () => {
    expect(coverageLabel(null)).toBe('No records to show');
    expect(coverageLabel(undefined)).toBe('No records to show');
  });
});

describe('latest value', () => {
  it('returns null, not 0, when there is no latest value', () => {
    expect(latestValue(series({ latest: undefined }))).toBeNull();
    expect(latestValue(null)).toBeNull();
  });

  it('keeps a real 0', () => {
    expect(latestValue(series({ latest: { date: '2026-06-01', value: 0 } }))).toBe(0);
  });

  it('does not count a non-finite number as a value', () => {
    expect(
      latestValue(series({ latest: { date: '2026-06-01', value: Number.NaN } })),
    ).toBeNull();
  });
});

describe('index by metric name', () => {
  it('turns the array into a lookup table', () => {
    const map = indexSeries([series(), series({ metric: 'vo2max' })]);
    expect(Object.keys(map).sort()).toEqual(['resting_hr', 'vo2max']);
    expect(map.vo2max.metric).toBe('vo2max');
  });
});

describe('pace format', () => {
  it('pads seconds to two digits', () => {
    expect(formatPaceSeconds(305)).toBe('5:05');
    expect(formatPaceSeconds(300)).toBe('5:00');
  });

  it('gives a placeholder, not 0:00, when there is no pace', () => {
    expect(formatPaceSeconds(null)).toBe('\u2014');
    expect(formatPaceSeconds(undefined)).toBe('\u2014');
    expect(formatPaceSeconds(0)).toBe('\u2014');
  });
});

describe('chart options', () => {
  it('does not join a line across gaps in the data', () => {
    const option = buildSeriesOption(series(), { color: '#66d9a0' }) as {
      series: Array<Record<string, unknown>>;
    };
    const line = option.series.find((item) => item.type === 'line');
    expect(line?.connectNulls).toBeFalsy();
  });

  it('puts every data point in the chart without resampling', () => {
    const option = buildSeriesOption(series(), { color: '#66d9a0' }) as {
      series: Array<{ data?: unknown[] }>;
    };
    const line = option.series.find((item) => Array.isArray(item.data));
    expect(line?.data).toHaveLength(2);
  });
});

describe('manually logged days must not be drawn as continuous', () => {
  const logged = series({
    metric: 'intake_calories',
    unit: 'kcal',
    window_days: 30,
    days_with_data: 3,
    points: [
      { date: '2026-03-01', value: 2100 },
      { date: '2026-03-02', value: 1980 },
      { date: '2026-03-05', value: 2260 },
    ],
    latest: { date: '2026-03-05', value: 2260 },
  } as Partial<MetricSeries>);

  it('has no missing days on the default axis, so the line joins straight across', () => {
    const option = buildSeriesOption(logged, { color: '#fff' });
    expect(option.xAxis).toMatchObject({ data: ['2026-03-01', '2026-03-02', '2026-03-05'] });
  });

  it('leaves unlogged days empty on the axis with calendarAxis on', () => {
    const option = buildSeriesOption(logged, { color: '#fff', chart: 'bar', calendarAxis: true });
    expect(option.xAxis).toMatchObject({
      data: ['2026-03-01', '2026-03-02', '2026-03-03', '2026-03-04', '2026-03-05'],
    });
    const [bars] = option.series as Array<{ type: string; data: Array<number | null> }>;
    expect(bars.type).toBe('bar');
    expect(bars.data).toEqual([2100, 1980, null, null, 2260]);
  });
});

describe('date labels are local calendar dates, not instants', () => {
  afterEach(() => setDateOrder('regional'));

  type DateChart = {
    xAxis: { axisLabel: { formatter: (value: string) => string } };
    tooltip: { formatter: (params: Array<{ axisValue: string }>) => string };
  };

  const chart = (date: string) =>
    buildSeriesOption(series({ points: [{ date, value: 52 }] }), { color: '#fff' }) as unknown as DateChart;

  it('does not roll an impossible YYYY-MM-DD over to another day', () => {
    const option = chart('2026-02-30');
    expect(option.xAxis.axisLabel.formatter('2026-02-30')).toBe('2026-02-30');
    expect(option.tooltip.formatter([{ axisValue: '2026-02-30' }]))
      .toBe('2026-02-30<br><b>52</b>');
  });

  it('follows the date order setting on the axis', () => {
    const formatter = chart('2026-11-25').xAxis.axisLabel.formatter;
    const day = new Date(2026, 10, 25);
    setDateOrder('mdy');
    const mdy = formatter('2026-11-25');
    setDateOrder('dmy');
    const dmy = formatter('2026-11-25');
    expect(mdy).toBe(new Intl.DateTimeFormat('en-US', { month: '2-digit', day: '2-digit' }).format(day));
    expect(dmy).toBe(new Intl.DateTimeFormat('en-GB', { month: '2-digit', day: '2-digit' }).format(day));
    expect(mdy).not.toBe(dmy);
  });

  it('uses the same date format in the tooltip, not the raw ISO string', () => {
    setDateOrder('ymd');
    const option = chart('2026-11-25');
    const label = option.xAxis.axisLabel.formatter('2026-11-25');
    expect(label).not.toBe('2026-11-25');
    expect(option.tooltip.formatter([{ axisValue: '2026-11-25' }])).toBe(`${label}<br><b>52</b>`);
  });
});
