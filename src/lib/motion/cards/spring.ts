import { lessMotion } from '../lessMotion';
export interface Spring {
  stiffness: number;
  damping: number;
  mass?: number;
}

export const SPRINGS = {
  deal: { stiffness: 210, damping: 22 },
  flip: { stiffness: 420, damping: 32 },
  settle: { stiffness: 170, damping: 24 },
  pop: { stiffness: 520, damping: 26 },
  stack: { stiffness: 340, damping: 34 },
} as const satisfies Record<string, Spring>;

export interface SpringCurve {
  easing: string;
  duration: number;
}

const cache = new Map<string, SpringCurve>();

let linearOk: boolean | null = null;
export const supportsLinearEasing = (): boolean => {
  if (linearOk !== null) return linearOk;
  const css = typeof CSS !== 'undefined' && typeof CSS.supports === 'function' ? CSS : null;
  linearOk = css ? css.supports('transition-timing-function', 'linear(0, 1)') : true;
  return linearOk;
};
export const resetLinearProbe = (): void => {
  linearOk = null;
};

const fallbacks = new WeakMap<SpringCurve, SpringCurve>();
export const withLinearFallback = (curve: SpringCurve, spring: Spring): SpringCurve => {
  if (supportsLinearEasing()) return curve;
  const hit = fallbacks.get(curve);
  if (hit) return hit;
  const zeta = spring.damping / (2 * Math.sqrt(spring.stiffness * (spring.mass ?? 1)));
  const easing = zeta < 0.7 ? 'cubic-bezier(.2, 1.15, .32, 1)' : 'cubic-bezier(.4, .6, .2, 1)';
  const fallback = { easing, duration: curve.duration };
  fallbacks.set(curve, fallback);
  return fallback;
};

export const springCurve = (spring: Spring, samples = 40): SpringCurve => {
  const key = `${spring.stiffness}/${spring.damping}/${spring.mass ?? 1}/${samples}`;
  const hit = cache.get(key);
  if (hit) return withLinearFallback(hit, spring);
  const mass = spring.mass ?? 1;
  const dt = 1 / 1000;
  const trace: number[] = [];
  let x = 0;
  let v = 0;
  let t = 0;
  while (t < 3) {
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
  cache.set(key, curve);
  return withLinearFallback(curve, spring);
};

export const reducedMotion = (): boolean => lessMotion();

export const settled = (animations: Animation[]): Promise<void> =>
  Promise.all(animations.map((animation) => animation.finished.catch(() => undefined))).then(() => undefined);

export const centerOf = (rect: DOMRect): { x: number; y: number } => ({ x: rect.left + rect.width / 2, y: rect.top + rect.height / 2 });
