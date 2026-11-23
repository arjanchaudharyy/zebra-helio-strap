import { lessMotion } from '../../../lib/motion/lessMotion';
import { OPEN_EASE, paced } from '../../../lib/motion/timing';

export const MENU_MS = paced(340);
const RADIUS = 22;
const BLEED = 56;

interface From { l: number; t: number; w: number; h: number }
const fromOf = (el: HTMLElement): From | null => {
  const parts = (el.dataset.from ?? '').split(',').map(Number);
  return parts.length === 4 && parts.every(Number.isFinite) ? { l: parts[0], t: parts[1], w: parts[2], h: parts[3] } : null;
};
const px = (n: number) => `${Math.round(n * 10) / 10}px`;
const pill = (f: From, w: number, h: number) => `inset(${px(f.t)} ${px(w - f.l - f.w)} ${px(h - f.t - f.h)} ${px(f.l)} round ${RADIUS}px)`;
const whole = `inset(0px 0px 0px 0px round ${RADIUS}px)`;
const bled = `inset(${-BLEED}px ${-BLEED}px ${-BLEED}px ${-BLEED}px round ${RADIUS}px)`;
const innerOf = (el: HTMLElement) => el.querySelector<HTMLElement>(':scope > .menu-inner');

export const menuEnter = (node: Element, done: () => void): void => {
  const el = node as HTMLElement;
  const from = fromOf(el);
  const inner = innerOf(el);
  if (lessMotion() || !from || !inner) {
    inner?.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 140 });
    done();
    return;
  }
  const [w, h] = [el.offsetWidth, el.offsetHeight];
  const timing: KeyframeAnimationOptions = { duration: MENU_MS, easing: OPEN_EASE };
  const glass = el.animate([
    { offset: 0, clipPath: pill(from, w, h) },
    { offset: 0.92, clipPath: whole },
    { offset: 1, clipPath: bled },
  ], { ...timing, pseudoElement: '::before' });
  inner.animate([{ clipPath: pill(from, w, h), opacity: 0 }, { opacity: 1, offset: 0.45 }, { clipPath: whole, opacity: 1 }], timing);
  glass.finished.then(done, done);
};

export const menuLeave = (node: Element, done: () => void): void => {
  const el = node as HTMLElement;
  const from = fromOf(el);
  const inner = innerOf(el);
  if (lessMotion() || !from || !inner) {
    (inner ?? el).animate([{ opacity: 1 }, { opacity: 0 }], { duration: 120 }).finished.then(done, done);
    return;
  }
  const [w, h] = [el.offsetWidth, el.offsetHeight];
  const timing: KeyframeAnimationOptions = { duration: Math.round(MENU_MS * 0.85), easing: OPEN_EASE, fill: 'forwards' };
  const glass = el.animate([
    { offset: 0, clipPath: bled },
    { offset: 0.08, clipPath: whole },
    { offset: 1, clipPath: pill(from, w, h) },
  ], { ...timing, pseudoElement: '::before' });
  inner.animate([{ clipPath: whole, opacity: 1 }, { opacity: 0, offset: 0.5 }, { clipPath: pill(from, w, h), opacity: 0 }], timing);
  glass.finished.then(done, done);
};
