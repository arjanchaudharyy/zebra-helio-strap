import { exemptFromSettle } from './interrupt';

const ATTR = 'data-source-hold';
const holds = new WeakMap<HTMLElement, number>();
const fades = new WeakMap<HTMLElement, Animation>();

const mark = (el: HTMLElement) => {
  if (fades.has(el)) { fades.get(el)?.cancel(); fades.delete(el); }
  el.setAttribute(ATTR, 'held');
};

export function holdSource(el: HTMLElement | null | undefined): () => void {
  if (!el?.isConnected) return () => undefined;
  holds.set(el, (holds.get(el) ?? 0) + 1);
  mark(el);
  let released = false;
  return () => {
    if (released) return;
    released = true;
    const left = (holds.get(el) ?? 1) - 1;
    if (left > 0) { holds.set(el, left); return; }
    holds.delete(el);
    fades.get(el)?.cancel();
    fades.delete(el);
    el.removeAttribute(ATTR);
  };
}

export function fadeSourceIn(el: HTMLElement | null | undefined, frames: Keyframe[], timing: KeyframeAnimationOptions): Animation | null {
  if (!el?.isConnected || !holds.has(el)) return null;
  fades.get(el)?.cancel();
  el.setAttribute(ATTR, 'fading');
  const animation = exemptFromSettle(el.animate(frames, { fill: 'both', ...timing }));
  fades.set(el, animation);
  return animation;
}

export const isSourceHeld = (el: HTMLElement): boolean => holds.has(el);

export const sourceInFrames = (from: number, to: number): Keyframe[] => [
  { opacity: 0 }, { opacity: 0, offset: from }, { opacity: 1, offset: to }, { opacity: 1 },
];
