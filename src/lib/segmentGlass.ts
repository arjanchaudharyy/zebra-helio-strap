
export interface ThumbBox {
  left: number;
  width: number;
  top: number;
  height: number;
}

export interface LensRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const LIFT_PAD_X = 7;
export const LIFT_Y = 1.2;

export function liftRect(thumb: ThumbBox, trackWidth: number, pad: number): LensRect {
  const row = thumb.height + pad * 2;
  const h = Math.round(row * LIFT_Y);
  const cx = thumb.left + thumb.width / 2;
  const room = 2 * Math.min(cx, trackWidth - cx);
  const want = Math.max(h, thumb.width + LIFT_PAD_X * 2);
  const w = Math.round(Math.max(thumb.width, Math.min(want, room)));
  return { x: cx - w / 2, y: thumb.top + thumb.height / 2 - h / 2, w, h };
}

export function rubberStretch(over: number, max: number): number {
  if (!over || max <= 0) return 0;
  const reach = Math.abs(over);
  return Math.sign(over) * max * (1 - 1 / ((reach / max) * 0.55 + 1));
}

export const stretchLimit = (trackWidth: number): number => Math.min(18, trackWidth * 0.05);

export function flickDeform(velocity: number): number {
  return Math.sign(velocity) * Math.min(0.035, Math.abs(velocity) * 0.012);
}

export const squash = (s: number): string => `${s.toFixed(4)} ${(1 - (s - 1) * 0.35).toFixed(4)}`;
