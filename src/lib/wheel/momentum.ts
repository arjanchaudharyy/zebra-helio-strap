
export interface WheelAccelState {
  lastTs: number;
  streak: number;
  pixels: number;
  sign: number;
}

export const newWheelAccel = (): WheelAccelState => ({ lastTs: 0, streak: 0, pixels: 0, sign: 0 });

export const GAP_MS = 90;
export const PIXELS_PER_STEP = 28;
const LADDER = [1, 1, 2, 2, 3, 3, 5, 5, 8];

const isNotch = (delta: number, mode: number) => mode !== 0 || Math.abs(delta) >= 50;

export const wheelSteps = (state: WheelAccelState, delta: number, mode: number, ts: number): number => {
  if (!delta) return 0;
  const sign = Math.sign(delta);
  const gap = state.lastTs ? ts - state.lastTs : Infinity;
  state.lastTs = ts;
  if (sign !== state.sign || gap > GAP_MS) {
    state.streak = 0;
    state.pixels = 0;
  }
  state.sign = sign;

  if (!isNotch(delta, mode)) {
    state.pixels += delta;
    const whole = Math.trunc(state.pixels / PIXELS_PER_STEP);
    state.pixels -= whole * PIXELS_PER_STEP;
    return whole;
  }
  const notches = mode === 0 ? Math.max(1, Math.round(Math.abs(delta) / 100)) : Math.max(1, Math.round(Math.abs(delta)));
  const multiplier = LADDER[Math.min(state.streak, LADDER.length - 1)]!;
  state.streak += 1;
  return sign * notches * multiplier;
};

export const daysInMonth = (year: number, month: number): number => new Date(year, month, 0).getDate();

const ISO_DAY = /^\d{4}-\d{2}-\d{2}$/;

export const wheelDateValue = (modelValue: string | null | undefined, min: string | undefined, today: string): string => {
  if (modelValue && ISO_DAY.test(modelValue)) return modelValue;
  if (min && ISO_DAY.test(min)) return min;
  return today;
};

export const clampDate = (year: number, month: number, day: number, min?: string | null, max?: string | null): string => {
  const d = Math.min(Math.max(1, day), daysInMonth(year, month));
  const pad = (n: number) => String(n).padStart(2, '0');
  let iso = `${year}-${pad(month)}-${pad(d)}`;
  if (min && iso < min) iso = min;
  if (max && iso > max) iso = max;
  return iso;
};
