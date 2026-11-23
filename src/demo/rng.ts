export interface Rng {
  /** [0, 1) */
  next(): number;
  between(min: number, max: number): number;
  int(min: number, max: number): number;
  chance(probability: number): boolean;
}

export const makeRng = (seed: number): Rng => {
  let state = seed >>> 0;
  const next = (): number => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return {
    next,
    between: (min, max) => min + (max - min) * next(),
    int: (min, max) => Math.floor(min + (max - min + 1) * next()),
    chance: (probability) => next() < probability,
  };
};

const pad = (value: number) => String(value).padStart(2, '0');

export const dayKey = (date: Date): string => `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;

export const at = (base: Date, days: number, hour = 0, minute = 0): Date =>
  new Date(base.getFullYear(), base.getMonth(), base.getDate() + days, hour, minute, 0, 0);

export const iso = (date: Date): string => date.toISOString();

export const round = (value: number, digits = 0): number => {
  const scale = 10 ** digits;
  return Math.round(value * scale) / scale;
};

export const clamp = (value: number, min: number, max: number): number => Math.min(max, Math.max(min, value));
