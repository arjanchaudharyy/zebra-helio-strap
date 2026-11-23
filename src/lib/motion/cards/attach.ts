import type { Pt } from '../../aiTask/threads';
import { OPEN_EASE, paced } from '../timing';
import { settled } from './spring';

export interface Step { el: Element; frames: Keyframe[]; delay: number; duration: number; easing?: string }
export interface Plan { steps: Step[]; total: number }
export interface Played { animations: Animation[]; clock: Animation; plan: Plan }

export const ATTACH_EASE = 'cubic-bezier(.45, .2, .2, 1)';
export const FLY_MS = paced(760);
export const STAGGER = 70;
export const CHIP_MS = paced(600);
export const GATE_AT = 0.74;
const GATE_MS = paced(420);
export const RECALL_RATE = 1.8;

export const planOf = (steps: Step[]): Plan => ({ steps, total: Math.max(1, ...steps.map((s) => s.delay + s.duration)) });

export const play = (plan: Plan, reverse = false): Played => {
  const direction: PlaybackDirection = reverse ? 'reverse' : 'normal';
  const animations = plan.steps.map((step) => step.el.animate(step.frames, {
    duration: step.duration, delay: reverse ? plan.total - step.delay - step.duration : step.delay,
    easing: step.easing ?? 'linear', fill: 'both', direction,
  }));
  const clock = new Animation(new KeyframeEffect(null, [], { duration: plan.total, fill: 'both', direction }), document.timeline);
  clock.play();
  return { animations, clock, plan };
};

export const positionOf = (played: Played): number => Number(played.clock.effect?.getComputedTiming().progress ?? 0) * played.plan.total;

export const rewind = (played: Played, rate = RECALL_RATE): Promise<void> => {
  const all = [...played.animations, played.clock];
  for (const animation of all) {
    animation.updatePlaybackRate(-rate);
    if (animation.playState === 'finished') animation.play();
  }
  return settled(all);
};

export const cancelPlayed = (played: Played | null): void => {
  if (!played) return;
  for (const animation of played.animations) animation.cancel();
  played.clock.cancel();
};

const deg = (el: Element) => {
  const match = /(-?[\d.]+)deg/.exec(getComputedStyle(el).rotate);
  return match ? Number(match[1]) : 0;
};
const f1 = (n: number) => n.toFixed(1);

export const cardStep = (el: HTMLElement, turn: Element, side: 'left' | 'right', points: Pt[], delay: number): Step => {
  const theta = deg(turn);
  const rad = (theta * Math.PI) / 180;
  const cos = Math.cos(rad), sin = Math.sin(rad);
  const box = el.getBoundingClientRect();
  const half = (side === 'left' ? -1 : 1) * (el.offsetWidth / 2);
  const start = { x: box.left + box.width / 2 + cos * half, y: box.top + box.height / 2 + sin * half };
  const p0 = points[0] ?? start;
  const n = Math.max(1, points.length - 1);
  const origin = side === 'left' ? '0% 50%' : '100% 50%';
  const frames = points.map((p, k): Keyframe => {
    const u = k / n;
    const dx = p.x - p0.x + (p0.x - start.x) * u;
    const dy = p.y - p0.y + (p0.y - start.y) * u;
    const vx = cos * dx + sin * dy;
    const vy = -sin * dx + cos * dy;
    return {
      offset: u, transformOrigin: origin, opacity: u > 0.9 ? (1 - u) / 0.1 : 1,
      transform: `translate(${f1(vx)}px, ${f1(vy)}px) rotate(${(-theta * u).toFixed(2)}deg) scale(${(1 - 0.68 * u).toFixed(3)})`,
    };
  });
  return { el, frames, delay, duration: FLY_MS, easing: ATTACH_EASE };
};

export const reelStep = (el: Element, from: number, to: number, delay: number): Step => ({
  el, delay, duration: FLY_MS, easing: ATTACH_EASE,
  frames: [
    { offset: 0, strokeDashoffset: '0' },
    ...(from > 0 ? [{ offset: from, strokeDashoffset: '0' }] : []),
    { offset: Math.max(from + 0.001, Math.min(1, to)), strokeDashoffset: '1' },
    ...(to < 1 ? [{ offset: 1, strokeDashoffset: '1' }] : []),
  ],
});

export const gateRing = (lock: Element): HTMLElement => {
  const ring = document.createElement('span');
  ring.setAttribute('aria-hidden', 'true');
  Object.assign(ring.style, {
    position: 'absolute', inset: '-9px', borderRadius: '50%', border: '2px solid var(--accent)', opacity: '0', pointerEvents: 'none',
    boxShadow: '0 0 12px -4px color-mix(in srgb, var(--accent) 60%, transparent)',
  });
  lock.appendChild(ring);
  return ring;
};

export const gateStep = (ring: Element, delay: number): Step => ({
  el: ring, delay, duration: GATE_MS, easing: 'ease-out',
  frames: [{ opacity: 0, transform: 'scale(.94)' }, { opacity: 0.55, transform: 'scale(1.02)', offset: 0.28 }, { opacity: 0, transform: 'scale(1.14)' }],
});

export const chipStep = (el: HTMLElement, points: Pt[], delay: number): Step => {
  const box = el.getBoundingClientRect();
  const c = { x: box.left + box.width / 2, y: box.top + box.height / 2 };
  const n = Math.max(1, points.length - 1);
  const frames = points.map((p, k): Keyframe => {
    const u = k / n;
    return { offset: u, transform: k === n ? 'none' : `translate(${f1(p.x - c.x)}px, ${f1(p.y - c.y)}px) scale(${(0.45 + 0.55 * u).toFixed(3)})` };
  });
  return { el, frames, delay, duration: CHIP_MS, easing: ATTACH_EASE };
};

export const popStep = (el: Element, delay: number): Step => ({
  el, delay, duration: 300, easing: 'ease-out',
  frames: [{ transform: 'none' }, { transform: 'scale(1.32)', offset: 0.35 }, { transform: 'none' }],
});

export const riseStep = (el: Element, delay: number, duration: number, lift = 6): Step => ({
  el, delay, duration, easing: OPEN_EASE,
  frames: [{ opacity: 0, transform: `translateY(${lift}px)` }, { opacity: 1, transform: 'none' }],
});

export const mergeSteps = (chips: HTMLElement[], panel: Element | null, pieces: HTMLElement[]): Step[] => {
  const file = pieces.find((el) => el.dataset.enter === 'file') ?? null;
  const target = (file ?? pieces[0])?.getBoundingClientRect();
  const steps: Step[] = [];
  chips.forEach((el, i) => {
    if (!target) return;
    const box = el.getBoundingClientRect();
    const dx = target.left + target.width / 2 - (box.left + box.width / 2);
    const dy = target.top + target.height / 2 - (box.top + box.height / 2);
    steps.push({
      el, delay: i * 28, duration: paced(460), easing: ATTACH_EASE,
      frames: [{ transform: 'none', opacity: 1 }, { transform: `translate(${f1(dx * 0.7)}px, ${f1(dy * 0.7)}px) scale(.62)`, opacity: 1, offset: 0.62 },
        { transform: `translate(${f1(dx)}px, ${f1(dy)}px) scale(.4)`, opacity: 0 }],
    });
  });
  if (panel) steps.push({ el: panel, delay: paced(80), duration: paced(320), easing: OPEN_EASE, frames: [{ opacity: 1 }, { opacity: 0 }] });
  const at: Record<string, [number, number]> = { head: [0, paced(380)], file: [paced(240), paced(480)], rest: [paced(400), paced(320)], act: [paced(480), paced(340)] };
  for (const el of pieces) {
    const [delay, duration] = at[el.dataset.enter ?? ''] ?? at.rest!;
    steps.push(el === file
      ? { el, delay, duration, easing: OPEN_EASE, frames: [{ opacity: 0, transform: 'scale(.72)' }, { opacity: 1, transform: 'none' }] }
      : riseStep(el, delay, duration));
  }
  return steps;
};
