import { OPEN_EASE, paced } from './motion/timing';

export const ZOOM_MS = paced(320);
export const ZOOM_MIN_MS = paced(180);
export const ZOOM_EASE = OPEN_EASE;
export const ZOOM_SETTLE_MS = 120;
export const ZOOM_COMMIT_TIMEOUT_MS = 400;

const FULL_SPAN = Math.log(125 / 80);

export const scaleFromTransform = (transform: string | null | undefined): number => {
  if (!transform || transform === 'none') return 1;
  const matrix = transform.match(/^matrix\(([^)]+)\)$/);
  if (matrix) {
    const [a, b] = matrix[1].split(',').map((part) => Number.parseFloat(part));
    const value = Math.hypot(a, b);
    return Number.isFinite(value) && value > 0 ? value : 1;
  }
  const matrix3d = transform.match(/^matrix3d\(([^)]+)\)$/);
  if (matrix3d) {
    const [a, b] = matrix3d[1].split(',').map((part) => Number.parseFloat(part));
    const value = Math.hypot(a, b);
    return Number.isFinite(value) && value > 0 ? value : 1;
  }
  const scale = transform.match(/^scale\(([^,)]+)/);
  if (scale) {
    const value = Number.parseFloat(scale[1]);
    return Number.isFinite(value) && value > 0 ? value : 1;
  }
  return 1;
};

export interface ZoomLeg {
  from: number;
  to: number;
  durationMs: number;
}

export const zoomLeg = (applied: number, current: number, target: number): ZoomLeg => {
  const to = target / applied;
  const from = current > 0 && Number.isFinite(current) ? current : 1;
  const distance = Math.abs(Math.log(to / from));
  const share = Math.min(1, distance / FULL_SPAN);
  const durationMs = distance < 1e-4 ? 0 : Math.round(ZOOM_MIN_MS + (ZOOM_MS - ZOOM_MIN_MS) * Math.sqrt(share));
  return { from: round(from), to: round(to), durationMs };
};

export const ghostBox = (ratio: number): { width: string; height: string; marginLeft: string } => {
  const r = round(ratio);
  return {
    width: `calc(100% * ${r})`,
    height: `calc(100% * ${r})`,
    marginLeft: `calc(-50% * ${r})`,
  };
};

export const sameZoom = (a: number, b: number): boolean => Math.abs(a - b) < 1e-4;

const round = (value: number) => Number(value.toFixed(5));
