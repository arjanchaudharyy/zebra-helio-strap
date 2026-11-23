import { displayDateTimeFormatter } from './dateTime';
import { isFiniteNumber } from './format';
import { downsample } from './workoutRoute';
import type { AxisRange } from './chartAxis';
import type { ChartPalette } from './echartsTheme';
import type { WorkoutSeriesSample } from '../types';


export type SeriesKey = keyof Pick<WorkoutSeriesSample, 'heart_rate' | 'pace' | 'altitude_m' | 'cadence'>;
export type ChartPoint = { t: number; v: number };

export const sampleSeries = (samples: WorkoutSeriesSample[] | undefined, key: SeriesKey): ChartPoint[] =>
  (samples ?? [])
    .map((sample) => ({ t: new Date(sample.timestamp).getTime(), v: sample[key] }))
    .filter((point): point is ChartPoint => {
      if (!Number.isFinite(point.t) || !isFiniteNumber(point.v)) return false;
      if (key === 'heart_rate') return point.v >= 20 && point.v <= 250;
      if (key === 'pace') return point.v >= 1 && point.v < 60;
      if (key === 'cadence') return point.v > 0 && point.v < 300;
      return point.v >= -500 && point.v <= 10_000;
    });

export const chartStats = (points: readonly ChartPoint[]) => {
  if (points.length < 2) return null;
  let low = points[0]!;
  let high = low;
  let sum = 0;
  for (const point of points) {
    if (point.v < low.v) low = point;
    if (point.v > high.v) high = point;
    sum += point.v;
  }
  return { low, high, average: sum / points.length };
};

export const paceSeconds = (seconds: number): string => {
  const total = Math.round(seconds);
  return `${Math.floor(total / 60)}'${String(total % 60).padStart(2, '0')}"`;
};

export interface LineOptionOpts {
  color: string;
  unit: string;
  palette: ChartPalette;
  axis?: AxisRange | null;
  inverse?: boolean;
  format?: (value: number) => string;
  average?: number | null;
  highlights?: readonly ChartPoint[];
}

export const lineOption = (points: ChartPoint[], opts: LineOptionOpts) => {
  const { color, unit, palette, axis, inverse = false, format } = opts;
  if (points.length < 2) return null;
  const stats = chartStats(points)!;
  const avg = isFiniteNumber(opts.average) ? opts.average : stats.average;
  let plotted = downsample(points);
  if (plotted !== points) {
    const keep = new Set([stats.low.t, stats.high.t, ...(opts.highlights ?? []).map((point) => point.t)]);
    const byTime = new Map(plotted.map((point) => [point.t, point]));
    for (const point of points) {
      if (keep.has(point.t)) byTime.set(point.t, point);
    }
    plotted = [...byTime.values()].sort((a, b) => a.t - b.t);
  }
  const span = points[points.length - 1]!.t - points[0]!.t;
  const tick = Math.max(60_000, Math.ceil(span / 4 / 60_000) * 60_000);
  return {
    animation: false,
    grid: { left: 8, right: 18, top: 12, bottom: 8, containLabel: true },
    xAxis: {
      type: 'time',
      splitNumber: 4,
      minInterval: tick,
      maxInterval: tick,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: palette.axis, fontSize: 14.5, hideOverlap: true, formatter: '{HH}:{mm}' },
      splitLine: { show: false },
    },
    yAxis: {
      type: 'value',
      scale: true,
      ...(axis ? { min: axis.min, max: axis.max, interval: axis.interval } : {}),
      inverse,
      axisLine: { show: false },
      axisTick: { show: false },
      axisLabel: { color: palette.axis, fontSize: 14.5, ...(format ? { formatter: format } : {}) },
      splitLine: { show: true, lineStyle: { color: palette.gridSoft, type: 'dashed' } },
    },
    tooltip: {
      trigger: 'axis',
      backgroundColor: palette.tooltipBg,
      borderColor: palette.tooltipBorder,
      borderWidth: 1,
      textStyle: { color: palette.tooltipText, fontSize: 15.5 },
      formatter: (params: Array<{ value: [number, number] }>) => {
        const point = Array.isArray(params) ? params[0] : params;
        if (!point) return '';
        const time = displayDateTimeFormatter({ hour: '2-digit', minute: '2-digit', hour12: false }).format(new Date(point.value[0]));
        const value = format ? format(point.value[1]) : String(Math.round(point.value[1] * 10) / 10);
        return `${time}&emsp;<b>${value}</b> ${unit}`;
      },
    },
    series: [{
      type: 'line',
      data: plotted.map((point) => [point.t, point.v]),
      smooth: 0.2,
      showSymbol: false,
      clip: true,
      lineStyle: { width: 2, color },
      areaStyle: {
        origin: inverse ? 'end' : 'start',
        color: {
          type: 'linear',
          x: 0,
          y: 0,
          x2: 0,
          y2: 1,
          colorStops: [
            { offset: 0, color: `${color}40` },
            { offset: 1, color: `${color}00` },
          ],
        },
      },
      markLine: {
        silent: true,
        symbol: 'none',
        lineStyle: { type: 'dashed', color: palette.mark, width: 1.2 },
        data: [{ yAxis: format ? avg : Math.round(avg) }],
        label: { show: false },
      },
    }],
  };
};

export type ChartTier = 'sm' | 'md' | 'lg';
export const chartTier = (width: number, height = Infinity): ChartTier => {
  if (width >= 760 && height >= 360) return 'lg';
  if (width >= 440 && height >= 250) return 'md';
  return 'sm';
};
export const CHART_TYPE: Record<ChartTier, { num: number; numLong: number; label: number; axis: number }> = {
  sm: { num: 18.5, numLong: 17.5, label: 13, axis: 12 },
  md: { num: 22, numLong: 20, label: 13, axis: 13 },
  lg: { num: 28, numLong: 24, label: 14.5, axis: 14.5 },
};

export const withFontSize = <T extends LineOption>(option: T, tier: ChartTier): T => {
  const px = CHART_TYPE[tier].axis;
  const label = (axis: { axisLabel: Record<string, unknown> }) => ({ ...axis, axisLabel: { ...axis.axisLabel, fontSize: px } });
  const series = option.series.map((line) => {
    const rich = line as typeof line & { markPoint?: { label?: Record<string, unknown> }; markArea?: { label?: Record<string, unknown> } };
    return {
      ...line,
      markLine: { ...line.markLine, label: { ...line.markLine.label, fontSize: px } },
      ...(rich.markPoint ? { markPoint: { ...rich.markPoint, label: { ...rich.markPoint.label, fontSize: px } } } : {}),
      ...(rich.markArea ? { markArea: { ...rich.markArea, label: { ...rich.markArea.label, fontSize: px - 1 } } } : {}),
    };
  });
  return { ...option, xAxis: label(option.xAxis), yAxis: label(option.yAxis), series } as T;
};

export interface ChartBand { index: number; low: number | null; high: number }
export interface ChartDetail {
  color: string;
  palette: ChartPalette;
  format?: (value: number) => string;
  bands?: ChartBand[];
  zoneColor?: (index: number) => string;
  pauses?: Array<{ start: number; end: number }>;
  extremes?: ChartPoint[];
}
type LineOption = NonNullable<ReturnType<typeof lineOption>>;
type AreaEdge = { xAxis?: number; yAxis?: number; name?: string; itemStyle?: Record<string, unknown>; label?: Record<string, unknown> };

const visibleBands = (bands: ChartBand[], values: number[]) => {
  const lo = Math.min(...values);
  const hi = Math.max(...values);
  return bands
    .map((band) => ({ ...band, low: band.low ?? -Infinity }))
    .filter((band) => band.high > lo && band.low < hi)
    .map((band) => ({ index: band.index, from: Math.max(band.low, lo), to: Math.min(band.high, hi), low: band.low, high: band.high }));
};

export const withDetail = (option: LineOption, tier: ChartTier, detail: ChartDetail): LineOption => {
  if (tier === 'sm') return option;
  const [series] = option.series;
  const data = series.data;
  const span = data[data.length - 1]![0] - data[0]![0];
  const parts = tier === 'lg' ? 8 : 6;
  const tick = Math.max(60_000, Math.ceil(span / parts / 60_000) * 60_000);
  const fmt = (value: number) => (detail.format ? detail.format(value) : String(Math.round(value)));
  const { palette, color } = detail;
  const label = { color: palette.axis, fontSize: 12.5, fontWeight: 600 };
  const areas: Array<[AreaEdge, AreaEdge]> = [];
  for (const pause of detail.pauses ?? []) {
    areas.push([{ xAxis: pause.start, itemStyle: { color: `${palette.axis}1f` } }, { xAxis: pause.end }]);
  }
  const zoneColor = detail.zoneColor ?? (() => color);
  const crossed = tier === 'lg' && detail.bands?.length ? visibleBands(detail.bands, data.map((d) => d[1])) : [];
  const bands = crossed.length >= 2 ? crossed : [];
  for (const band of bands) {
    const tone = zoneColor(band.index);
    areas.push([
      { yAxis: band.from, name: `Z${band.index}`, itemStyle: { color: `${tone}1c` }, label: { color: tone } },
      { yAxis: band.to },
    ]);
  }
  const line: Record<string, unknown> = {
    ...series,
    lineStyle: bands.length ? { width: series.lineStyle.width } : series.lineStyle,
    markLine: {
      ...series.markLine,
      label: { show: true, position: bands.length ? 'insideStartTop' : 'insideEndTop', ...label, formatter: (p: { value: number }) => fmt(p.value) },
    },
    ...(areas.length ? {
      markArea: {
        silent: true,
        label: { show: true, position: 'right', distance: 6, fontSize: 11, fontWeight: 600 },
        data: areas,
      },
    } : {}),
  };
  const next = {
    ...option,
    grid: { ...option.grid, top: tier === 'lg' ? 28 : option.grid.top, right: bands.length ? 44 : option.grid.right },
    xAxis: { ...option.xAxis, splitNumber: parts, minInterval: tick, maxInterval: tick },
    series: [line],
  } as unknown as LineOption & Record<string, unknown>;
  if (bands.length) {
    next.visualMap = {
      show: false,
      type: 'piecewise',
      seriesIndex: 0,
      dimension: 1,
      pieces: bands.map((band) => ({ ...(Number.isFinite(band.low) ? { gt: band.low } : {}), lte: band.high, color: zoneColor(band.index) })),
      outOfRange: { color },
    };
  }
  if (tier === 'lg') {
    const points = detail.extremes?.length ? detail.extremes.map((p) => ({ coord: [p.t, p.v], value: p.v })) : [{ type: 'max' }, { type: 'min' }];
    line.markPoint = {
      symbol: 'circle',
      symbolSize: 8,
      itemStyle: { color, borderColor: palette.surface, borderWidth: 2 },
      label: { show: true, position: 'top', distance: 6, ...label, color: palette.tooltipText, formatter: (p: { value: number }) => fmt(p.value) },
      data: points,
    };
  }
  return next;
};

const PX_PER_TICK = 34;
export const fitTicks = <T extends LineOption>(option: T, plotHeight: number): T => {
  const most = Math.max(2, Math.floor(plotHeight / PX_PER_TICK));
  const y = option.yAxis as LineOption['yAxis'] & { min?: number; max?: number; interval?: number; splitNumber?: number };
  if (y.min == null || y.max == null || !y.interval) return { ...option, yAxis: { ...y, splitNumber: Math.min(5, most) } };
  const n = Math.round((y.max - y.min) / y.interval);
  if (n <= most) return option;
  let d = 2;
  while (d < n && (n % d !== 0 || n / d > most)) d += 1;
  return { ...option, yAxis: { ...y, interval: y.interval * d } };
};
