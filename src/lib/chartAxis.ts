
export interface AxisRange { min: number; max: number; interval: number }

export const niceStep = (raw: number): number => {
  if (!(raw > 0) || !Number.isFinite(raw)) return 1;
  const exp = Math.floor(Math.log10(raw));
  const base = 10 ** exp;
  const f = raw / base;
  const nice = f <= 1 ? 1 : f <= 2 ? 2 : f <= 2.5 ? 2.5 : f <= 5 ? 5 : 10;
  return nice * base;
};

const snap = (value: number, step: number, dir: 'down' | 'up'): number => {
  const n = value / step;
  const k = dir === 'down' ? Math.floor(n + 1e-9) : Math.ceil(n - 1e-9);
  return Math.round(k * step * 1e6) / 1e6;
};

const extent = (values: readonly number[]): [number, number] | null => {
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of values) {
    if (!Number.isFinite(v)) continue;
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  return lo <= hi ? [lo, hi] : null;
};

export const elevationAxis = (values: readonly number[], minSpan: number): AxisRange | null => {
  const range = extent(values);
  if (!range) return null;
  const [lo, hi] = range;
  const span = Math.max((hi - lo) * 1.3, minSpan, 1e-6);
  const step = niceStep(span / 4);
  let min = lo - (span - (hi - lo)) / 2;
  if (lo >= 0 && min < 0) min = 0;
  min = snap(min, step, 'down');
  const max = snap(Math.max(hi, min + span), step, 'up');
  return { min, max, interval: step };
};

const PACE_STEPS = [5, 10, 15, 20, 30, 60, 120, 180, 300, 600];

export const paceAxis = (lo: number, hi: number): AxisRange | null => {
  if (!Number.isFinite(lo) || !Number.isFinite(hi) || hi < lo) return null;
  const pad = Math.max((hi - lo) * 0.15, 10);
  const span = hi - lo + pad * 2;
  const step = PACE_STEPS.find((s) => span / s <= 5) ?? 600;
  const min = Math.max(0, snap(lo - pad, step, 'down'));
  const max = snap(hi + pad, step, 'up');
  return { min, max: max > min ? max : min + step, interval: step };
};
