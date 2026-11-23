import { animateFromNow } from './reversible';
import { reducedMotion, SPRINGS, springCurve } from './spring';

const lidOf = (box: Element | null | undefined) => box?.querySelector<HTMLElement>('.box-lid') ?? null;

const OPEN_POSE = { small: 'translate(-1px, -5px) rotate(-26deg)', wide: 'translate(-2px, -9px) rotate(-62deg)' } as const;

export const lidOpen = (box: Element | null | undefined, how: keyof typeof OPEN_POSE = 'small'): Animation | null => {
  const lid = lidOf(box);
  if (!lid || reducedMotion()) return null;
  const { easing, duration } = springCurve(SPRINGS.pop);
  return animateFromNow(lid, { transform: OPEN_POSE[how] }, { duration, easing, fill: 'forwards' }, ['transform']);
};

export const lidClose = (box: Element | null | undefined, delay = 0): Animation | null => {
  const lid = lidOf(box);
  if (!lid || reducedMotion()) return null;
  const { easing, duration } = springCurve(SPRINGS.settle);
  return animateFromNow(lid, { transform: 'none' }, { duration, delay, easing, fill: 'forwards' }, ['transform']);
};

export const catchCard = (box: Element | null | undefined, flightMs: number): ((landed: boolean) => void) => {
  if (!lidOf(box) || reducedMotion()) return () => undefined;
  const timer = window.setTimeout(() => lidOpen(box), Math.max(0, flightMs * 0.42));
  return () => {
    window.clearTimeout(timer);
    lidClose(box, 60);
  };
};
