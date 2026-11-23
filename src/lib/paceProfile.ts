import type { ChartPoint } from './workoutCharts';

export interface TimeSpan { start: number; end: number }

export const SLOW_FACTOR = 3;
export const FAST_FACTOR = 0.4;
const SMOOTH = 5;

export interface PaceProfile {
  fastest: ChartPoint;
  slowest: ChartPoint;
  lo: number;
  hi: number;
  dropped: number;
  mean: number;
}

export const consistentAverage = (profile: PaceProfile, summary: number | null | undefined): number =>
  summary != null && Number.isFinite(summary) && summary >= profile.fastest.v && summary <= profile.slowest.v ? summary : profile.mean;

export const inSpans = (t: number, spans: readonly TimeSpan[]): boolean =>
  spans.some((span) => t >= span.start && t <= span.end);

export const quantile = (sorted: readonly number[], q: number): number => {
  if (!sorted.length) return NaN;
  const at = (sorted.length - 1) * q;
  const i = Math.floor(at);
  const frac = at - i;
  return i + 1 < sorted.length ? sorted[i] + (sorted[i + 1] - sorted[i]) * frac : sorted[i];
};

const rollingMedian = (points: readonly ChartPoint[], size: number): ChartPoint[] => {
  const half = Math.floor(size / 2);
  return points.map((point, i) => {
    const window = points.slice(Math.max(0, i - half), i + half + 1).map((p) => p.v).sort((a, b) => a - b);
    return { t: point.t, v: quantile(window, 0.5) };
  });
};

export const paceProfile = (points: readonly ChartPoint[], pauses: readonly TimeSpan[]): PaceProfile | null => {
  const moving = points.filter((p) => !inSpans(p.t, pauses));
  if (moving.length < 2) return null;
  const median = quantile(moving.map((p) => p.v).sort((a, b) => a - b), 0.5);
  const kept = moving.filter((p) => p.v <= median * SLOW_FACTOR && p.v >= median * FAST_FACTOR);
  if (kept.length < 2) return null;
  const smooth = kept.length < SMOOTH ? kept : rollingMedian(kept, SMOOTH);
  let fastest = smooth[0];
  let slowest = smooth[0];
  for (const p of smooth) {
    if (p.v < fastest.v) fastest = p;
    if (p.v > slowest.v) slowest = p;
  }
  const sorted = smooth.map((p) => p.v).sort((a, b) => a - b);
  let elapsed = 0;
  let distance = 0;
  for (let i = 1; i < smooth.length; i++) {
    const previous = smooth[i - 1];
    const point = smooth[i];
    const duration = point.t - previous.t;
    if (duration <= 0 || pauses.some(({ start, end }) => start < point.t && end > previous.t)) continue;
    elapsed += duration;
    distance += duration * (1 / previous.v + 1 / point.v) / 2;
  }
  const mean = elapsed > 0 ? elapsed / distance : smooth.length / smooth.reduce((sum, p) => sum + 1 / p.v, 0);
  return { fastest, slowest, lo: quantile(sorted, 0.02), hi: quantile(sorted, 0.98), dropped: moving.length - kept.length, mean };
};
