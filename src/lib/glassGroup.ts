import type { LensRect } from './segmentGlass';
import { squash } from './segmentGlass';

export interface GlassPose {
  x: number;
  y: number;
  sx: number;
  anchor: 'start' | 'end' | 'center';
  active: boolean;
}

export const RESTING: GlassPose = Object.freeze({ x: 0, y: 0, sx: 1, anchor: 'center', active: false });

export function stretchPose(stretch: number, trackWidth: number, active = false): GlassPose {
  if (!stretch) return { ...RESTING, active };
  return { x: 0, y: 0, sx: 1 + Math.abs(stretch) / Math.max(1, trackWidth), anchor: stretch < 0 ? 'end' : 'start', active };
}

const ORIGIN = { start: 'left center', end: 'right center', center: 'center' } as const;

export function poseStyle(pose: GlassPose): Record<string, string> {
  if (pose.sx === 1) return {};
  return { scale: squash(pose.sx), transformOrigin: ORIGIN[pose.anchor] };
}

export function springFrames(from: number, flick: number, width: number): { frames: Keyframe[]; duration: number } | null {
  const dir = from ? Math.sign(from) : Math.sign(flick);
  if (!width || !dir) return null;
  const start = 1 + Math.abs(from) / width;
  const peak = Math.max(start, 1 + Math.abs(flick));
  const origin = dir < 0 ? ORIGIN.end : ORIGIN.start;
  const overshoots = peak > start + 0.001;
  const frames: Keyframe[] = [{ scale: squash(start), transformOrigin: origin, easing: 'cubic-bezier(.3, .7, .4, 1)' }];
  if (overshoots) frames.push({ scale: squash(peak), transformOrigin: origin, offset: 0.26, easing: 'cubic-bezier(.4, 0, .4, 1)' });
  frames.push(
    { scale: squash(1 - (peak - 1) * 0.3), transformOrigin: origin, offset: 0.62, easing: 'cubic-bezier(.4, 0, .4, 1)' },
    { scale: '1 1', transformOrigin: origin },
  );
  return { frames, duration: overshoots ? 560 : 460 };
}

export function lensSpan(sizes: readonly number[], pos: number, loop: boolean): number {
  const n = sizes.length;
  if (!n) return 0;
  const p = loop ? ((pos % n) + n) % n : Math.min(n - 1, Math.max(0, pos));
  const lo = Math.floor(p);
  const hi = loop ? (lo + 1) % n : Math.min(n - 1, lo + 1);
  const f = p - lo;
  return (sizes[lo] ?? 0) * (1 - f) + (sizes[hi] ?? 0) * f;
}


export function gapBetween(a: LensRect, b: LensRect): number {
  const dx = Math.max(b.x - (a.x + a.w), a.x - (b.x + b.w));
  const dy = Math.max(b.y - (a.y + a.h), a.y - (b.y + b.h));
  if (dx < 0 && dy < 0) return Math.max(dx, dy);
  return Math.hypot(Math.max(0, dx), Math.max(0, dy));
}

export function mergeAmount(gap: number, reach: number): number {
  if (reach <= 0) return gap <= 0 ? 1 : 0;
  const t = Math.min(1, Math.max(0, 1 - gap / reach));
  return t * t * (3 - 2 * t);
}

export function unionRect(rects: readonly LensRect[]): LensRect | null {
  if (!rects.length) return null;
  let x1 = Infinity, y1 = Infinity, x2 = -Infinity, y2 = -Infinity;
  for (const r of rects) {
    x1 = Math.min(x1, r.x);
    y1 = Math.min(y1, r.y);
    x2 = Math.max(x2, r.x + r.w);
    y2 = Math.max(y2, r.y + r.h);
  }
  return { x: x1, y: y1, w: x2 - x1, h: y2 - y1 };
}

export interface GroupMember {
  id: string;
  rect: LensRect | null;
  pose: GlassPose;
}

export interface GroupState {
  active: boolean;
  amount: number;
  merged: boolean;
  union: LensRect | null;
}

export function groupState(members: readonly GroupMember[], reach: number): GroupState {
  const rects = members.map((m) => m.rect).filter((r): r is LensRect => r !== null);
  let amount = 0;
  for (let i = 0; i < rects.length; i++) {
    for (let j = i + 1; j < rects.length; j++) amount = Math.max(amount, mergeAmount(gapBetween(rects[i]!, rects[j]!), reach));
  }
  return { active: members.some((m) => m.pose.active), amount, merged: amount >= 0.5, union: unionRect(rects) };
}

export function bridgeRect(
  from: LensRect,
  to: LensRect,
  axis: 'x' | 'y',
  reach: number,
  { tuck = 8, ratio = 0.5, max = 28 } = {},
): LensRect | null {
  const [lead, trail] = axis === 'y'
    ? (from.y <= to.y ? [from, to] : [to, from])
    : (from.x <= to.x ? [from, to] : [to, from]);
  const gap = axis === 'y' ? trail.y - (lead.y + lead.h) : trail.x - (lead.x + lead.w);
  if (gap < 0 || gap >= reach) return null;
  const lo = axis === 'y' ? Math.max(lead.x, trail.x) : Math.max(lead.y, trail.y);
  const hi = axis === 'y' ? Math.min(lead.x + lead.w, trail.x + trail.w) : Math.min(lead.y + lead.h, trail.y + trail.h);
  if (hi <= lo) return null;
  const smaller = axis === 'y' ? Math.min(lead.w, trail.w) : Math.min(lead.h, trail.h);
  const thick = Math.min(max, smaller * ratio, hi - lo);
  const centre = (lo + hi) / 2;
  if (axis === 'y') return { x: centre - thick / 2, y: lead.y + lead.h - tuck, w: thick, h: gap + tuck * 2 };
  return { x: lead.x + lead.w - tuck, y: centre - thick / 2, w: gap + tuck * 2, h: thick };
}
