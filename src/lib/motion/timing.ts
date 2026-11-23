export const PACE = 1.3;
export const paced = (ms: number) => Math.round(ms * PACE);

export const OPEN_MS = paced(500);
export const CLOSE_MS = paced(440);
export const OPEN_EASE = 'cubic-bezier(.4, .6, .2, 1)';
export const CLOSE_EASE = 'cubic-bezier(.4, .6, .2, 1)';
export const BACK_EASE = 'cubic-bezier(.32, .08, .24, 1)';

export const YIELD_MS = 160;
export const DEVELOP_MS = 680;
export const DEVELOP_EASE = 'cubic-bezier(.4, 0, .2, 1)';

export const REVEAL_MS = paced(640);

export const LOCALE_REVEAL_MS = 1050;

export const BODY_IN: Keyframe[] = [{ opacity: 0 }, { opacity: 0, offset: 0.12 }, { opacity: 1, offset: 0.5 }, { opacity: 1 }];
export const BODY_OUT: Keyframe[] = [{ opacity: 1 }, { opacity: 1, offset: 0.65 }, { opacity: 0, offset: 0.85 }, { opacity: 0 }];

export const HANDOFF_SPAN = 0.14;
export const HANDOFF_EARLIEST = 0.28;
export const HANDOFF_BACK = 0.6;
export const COMPRESS_MS = 90;

export const turnedEase = (ease: string): string => {
  const match = ease.match(/^cubic-bezier\(([^)]+)\)$/);
  const values = match?.[1].split(',').map((part) => Number.parseFloat(part));
  if (!values || values.length !== 4 || values.some((value) => !Number.isFinite(value))) return ease;
  const [x1, y1, x2, y2] = values as [number, number, number, number];
  const r = (value: number) => Number((1 - value).toFixed(3));
  return `cubic-bezier(${r(x2)}, ${r(y2)}, ${r(x1)}, ${r(y1)})`;
};

export const easeAt = (ease: string, x: number): number => {
  const values = ease.match(/^cubic-bezier\(([^)]+)\)$/)?.[1].split(',').map((part) => Number.parseFloat(part));
  const t0 = Math.min(1, Math.max(0, x));
  if (!values || values.length !== 4 || values.some((value) => !Number.isFinite(value))) return t0;
  const [x1, y1, x2, y2] = values as [number, number, number, number];
  const at = (t: number, a: number, b: number) => 3 * a * t * (1 - t) ** 2 + 3 * b * t * t * (1 - t) + t ** 3;
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 40; i += 1) {
    const mid = (lo + hi) / 2;
    if (at(mid, x1, x2) < t0) lo = mid; else hi = mid;
  }
  return at((lo + hi) / 2, y1, y2);
};

export const timeAt = (ease: string, y: number): number => {
  const target = Math.min(1, Math.max(0, y));
  let lo = 0;
  let hi = 1;
  for (let i = 0; i < 40; i += 1) {
    const mid = (lo + hi) / 2;
    if (easeAt(ease, mid) < target) lo = mid; else hi = mid;
  }
  return (lo + hi) / 2;
};

export const RECEDE_SCALE = 0.94;
export const RECEDE_OPACITY = 0.32;
