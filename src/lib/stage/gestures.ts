import { GAP, MIN_H, ratioPresets, ratioRange, SPAN_MIN, SPLIT_MIN, SPLIT_PRESETS, STRIP_W, type Rect, type SideRole, type Sides } from './layout';
import { COLUMN_SNAP, FULL_SNAP, GRID_SNAP, ROW_SNAP, TOP_SNAP, type SizePatch } from './state';

export const DRAG_SLOP = 6;
export const passedSlop = (dx: number, dy: number): boolean => Math.hypot(dx, dy) >= DRAG_SLOP;

export type Drop = { kind: 'strip' } | { kind: 'stage'; index: number; onto: string | null } | { kind: 'none' };

export const dropAt = (
  point: { x: number; y: number },
  canvas: { width: number; height: number; stageX: number; narrow: boolean },
  stage: ReadonlyArray<{ id: string } & Rect>,
  dragging: string,
): Drop => {
  if (point.x < 0 || point.y < -40 || point.x > canvas.width || point.y > canvas.height + 40) return { kind: 'none' };
  if (!canvas.narrow && point.x < Math.max(canvas.stageX, STRIP_W) - 8) return { kind: 'strip' };
  if (canvas.narrow && point.x < 40) return { kind: 'strip' };
  const others = stage.filter((r) => r.id !== dragging);
  const hit = others.findIndex((r) => point.x >= r.x && point.x <= r.x + r.w && point.y >= r.y && point.y <= r.y + r.h);
  if (hit >= 0) return { kind: 'stage', index: stage.findIndex((r) => r.id === others[hit].id), onto: others[hit].id };
  const mid = canvas.stageX + (canvas.width - canvas.stageX) / 2;
  return { kind: 'stage', index: point.x < mid ? 0 : stage.length, onto: null };
};

export const TUCK_RATIO = 0.55;
export const MAGNET = 0.015;

export type Handle = 'left' | 'right' | 'bottom' | 'bl' | 'br';
export const HANDLES: readonly Handle[] = ['left', 'right', 'bottom', 'bl', 'br'];
export const handleSide = (handle: Handle): 'left' | 'right' | null =>
  handle === 'left' || handle === 'bl' ? 'left' : handle === 'right' || handle === 'br' ? 'right' : null;
export const handleVertical = (handle: Handle): boolean => handle === 'bottom' || handle === 'bl' || handle === 'br';

export interface ResizeContext {
  count: number; index: number; snap: number;
  id: string;
  sides: Sides;
  avail: number;
  width: number;
  groupW: number;
  startW: number; startH: number;
  maxH: number;
  splits?: readonly [number, number];
}

export const ratioFor = (c: Pick<ResizeContext, 'count' | 'index' | 'snap' | 'avail'> & { groupW?: number }, w: number): number | null => {
  if (c.count === 1) return w / Math.max(1, c.avail);
  if (c.count === 3 && c.snap === TOP_SNAP && c.index === 0) return null;
  if (c.count === 4 && c.snap === ROW_SNAP) return null;
  const share = w / Math.max(1, (c.groupW ?? c.avail) - GAP);
  if (c.count === 4 && c.snap === GRID_SNAP) return c.index % 2 === 0 ? share : 1 - share;
  if (c.count === 4 && c.snap === COLUMN_SNAP) return c.index === 0 ? share : 1 - share;
  const lead = c.count === 3 && c.snap === TOP_SNAP ? 1 : 0;
  return c.index === lead ? share : 1 - share;
};

const splitOutcome = (c: ResizeContext, side: 'left' | 'right', grow: number): Resize => {
  const cuts: [number, number] = [...(c.splits ?? [1 / 3, 2 / 3])];
  const k = side === 'right' ? c.index - 1 : c.index - 2;
  if (k < 0 || k > 1) return { kind: 'none' };
  const row = Math.max(1, c.groupW - 2 * GAP);
  const raw = cuts[k] + (side === 'right' ? grow : -grow) / row;
  const [lo, hi] = k === 0 ? [SPLIT_MIN, cuts[1] - SPLIT_MIN] : [cuts[0] + SPLIT_MIN, 1 - SPLIT_MIN];
  cuts[k] = magnet(Math.min(hi, Math.max(lo, raw)), SPLIT_PRESETS);
  return { kind: 'size', patch: { splits: cuts } };
};

const magnet = (value: number, presets: readonly number[]): number =>
  presets.find((preset) => Math.abs(preset - value) <= MAGNET) ?? value;

export type Resize = { kind: 'size'; patch: SizePatch } | { kind: 'tuck' } | { kind: 'full' } | { kind: 'none' };

const horizontal = (c: ResizeContext, role: SideRole, grow: number, side: 'left' | 'right'): Resize => {
  const full = c.count === 1 && c.snap === FULL_SNAP;
  const pointerW = role === 'sym' ? c.startW + 2 * grow : c.startW + grow;
  if (pointerW < c.startW * TUCK_RATIO) return { kind: 'tuck' };
  if (role === 'sym') {
    if (!full && pointerW > c.avail + 48) return { kind: 'full' };
    if (full && pointerW >= c.width - 24) return { kind: 'none' };
    const [lo, hi] = ratioRange(1, 0);
    return { kind: 'size', patch: { ratio: magnet(Math.min(hi, Math.max(lo, pointerW / Math.max(1, c.avail))), ratioPresets(1, 0)) } };
  }
  if (role === 'seam') {
    if (c.count === 4 && c.snap === ROW_SNAP) return splitOutcome(c, side, grow);
    const raw = ratioFor(c, pointerW);
    if (raw == null) return { kind: 'none' };
    const [lo, hi] = ratioRange(c.count, c.snap);
    return { kind: 'size', patch: { ratio: magnet(Math.min(hi, Math.max(lo, raw)), ratioPresets(c.count, c.snap)) } };
  }
  const span = (c.groupW + 2 * grow) / Math.max(1, c.avail);
  const clamped = Math.min(1, Math.max(SPAN_MIN, span));
  return { kind: 'size', patch: { span: clamped > 1 - MAGNET * 2 ? 1 : Math.round(clamped * 1000) / 1000 } };
};

export const resizeOutcome = (c: ResizeContext, handle: Handle, dx: number, dy: number): Resize => {
  const side = handleSide(handle);
  let patch: SizePatch = {};
  if (side) {
    const role = c.sides[side];
    const grow = side === 'right' ? dx : -dx;
    const out = horizontal(c, role, grow, side);
    if (out.kind === 'tuck' || out.kind === 'full') return out;
    if (out.kind === 'size') patch = { ...out.patch };
  }
  if (handleVertical(handle)) {
    const h = Math.round(Math.min(c.maxH, Math.max(MIN_H, c.startH + dy)));
    patch.heights = { [c.id]: h };
  }
  return Object.keys(patch).length ? { kind: 'size', patch } : { kind: 'none' };
};
