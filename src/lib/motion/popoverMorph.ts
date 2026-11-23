import { springCurve, withLinearFallback, type Spring, type SpringCurve } from './cards/spring';
import { windowInset, windowPoses, WINDOW_OFFSETS, type WindowRect } from './window';

export const POPOVER_RADIUS = 22;
export const SHADOW_BLEED = 64;

export function popoverFrames(from: WindowRect, to: WindowRect, panel: WindowRect, radius = POPOVER_RADIUS, bleed = 0): Keyframe[] {
  const poses = windowPoses({ rect: from, anchor: { x: from.left, y: from.top } }, { rect: to, anchor: { x: to.left, y: to.top } });
  const last = poses.length - 1;
  return poses.map((pose, i) => {
    const dx = pose.anchor.x - panel.left;
    const dy = pose.anchor.y - panel.top;
    const rect = i === last && bleed > 0
      ? { left: pose.rect.left - bleed, top: pose.rect.top - bleed, width: pose.rect.width + bleed * 2, height: pose.rect.height + bleed * 2 }
      : pose.rect;
    return {
      offset: WINDOW_OFFSETS[i],
      transform: `translate(${r2(dx)}px, ${r2(dy)}px)`,
      clipPath: windowInset(rect, { left: panel.left + dx, top: panel.top + dy, width: panel.width, height: panel.height }, radius, true),
    };
  });
}

export const bleedInset = (bleed = SHADOW_BLEED, radius = POPOVER_RADIUS): string =>
  `inset(${-bleed}px ${-bleed}px ${-bleed}px ${-bleed}px round ${radius}px)`;

export function reachable(rect: WindowRect | null, view: { width: number; height: number }): rect is WindowRect {
  if (!rect || rect.width <= 0 || rect.height <= 0) return false;
  return rect.left + rect.width > 0 && rect.top + rect.height > 0 && rect.left < view.width && rect.top < view.height;
}

const r2 = (value: number) => Number(value.toFixed(2));


export const rubberBand = (over: number, c = 120): number => {
  const a = Math.abs(over);
  return Math.sign(over) * (a / (1 + a / c));
};

export const elastic = (raw: number, min: number, max: number, c = 120): number => {
  if (raw < min) return min + rubberBand(raw - min, c);
  if (raw > max) return max + rubberBand(raw - max, c);
  return raw;
};

export interface DragSample { t: number; x: number; y: number }

export function releaseVelocity(samples: readonly DragSample[], releasedAt = Number.NaN, window = 80, stale = 60): { x: number; y: number } {
  const end = samples[samples.length - 1];
  if (!end || releasedAt - end.t > stale) return { x: 0, y: 0 };
  let first = end;
  for (let i = samples.length - 2; i >= 0; i -= 1) {
    if (end.t - samples[i]!.t > window) break;
    first = samples[i]!;
  }
  const dt = end.t - first.t;
  if (dt < 8) return { x: 0, y: 0 };
  return { x: (end.x - first.x) / dt, y: (end.y - first.y) / dt };
}

export const DISMISS_DISTANCE = 140;
export const DISMISS_VELOCITY = 0.9;

export const shouldDismiss = (offsetY: number, velocityY: number): boolean =>
  offsetY > DISMISS_DISTANCE || velocityY > DISMISS_VELOCITY;

const springCache = new Map<string, SpringCurve>();

export function springBack(spring: Spring, offset: { x: number; y: number }, velocity: { x: number; y: number }, samples = 40): SpringCurve {
  const d2 = offset.x * offset.x + offset.y * offset.y;
  const v0 = d2 < 1 ? 0 : Math.max(-24, Math.min(24, (-(velocity.x * offset.x + velocity.y * offset.y) / d2) * 1000));
  const rounded = Math.round(v0 * 4) / 4;
  if (rounded === 0) return springCurve(spring, samples);
  const key = `${spring.stiffness}/${spring.damping}/${spring.mass ?? 1}/${rounded}/${samples}`;
  const hit = springCache.get(key);
  if (hit) return withLinearFallback(hit, spring);
  const mass = spring.mass ?? 1;
  const dt = 1 / 1000;
  const trace: number[] = [];
  let x = 0;
  let v = rounded;
  let t = 0;
  while (t < 0.6) {
    const a = (-spring.stiffness * (x - 1) - spring.damping * v) / mass;
    v += a * dt;
    x += v * dt;
    t += dt;
    trace.push(x);
    if (Math.abs(1 - x) < 0.001 && Math.abs(v) < 0.01) break;
  }
  const points: string[] = [];
  for (let i = 0; i <= samples; i += 1) {
    const index = Math.min(trace.length - 1, Math.round((i / samples) * (trace.length - 1)));
    const value = i === 0 ? 0 : i === samples ? 1 : trace[index]!;
    points.push(String(Math.round(value * 1000) / 1000));
  }
  const curve = { easing: `linear(${points.join(', ')})`, duration: Math.round(trace.length * dt * 1000) };
  springCache.set(key, curve);
  return withLinearFallback(curve, spring);
}
