import type { Directive } from 'vue';

const MARGIN = 8;

interface Box { left: number; right: number; top: number; bottom: number }

const clips = (style: CSSStyleDeclaration) =>
  style.overflowX !== 'visible' || style.overflowY !== 'visible';

export function clipBox(el: HTMLElement): Box {
  const box: Box = { left: 0, top: 0, right: window.innerWidth, bottom: window.innerHeight };
  const fixed = getComputedStyle(el).position === 'fixed';
  if (fixed) return box;
  const absolute = getComputedStyle(el).position === 'absolute';
  let reachedContainer = !absolute;
  const container = el.offsetParent;
  for (let node = el.parentElement; node && node !== document.documentElement; node = node.parentElement) {
    if (!reachedContainer && node === container) reachedContainer = true;
    if (!reachedContainer) continue;
    const style = getComputedStyle(node);
    if (!clips(style)) continue;
    const rect = node.getBoundingClientRect();
    box.left = Math.max(box.left, rect.left);
    box.top = Math.max(box.top, rect.top);
    box.right = Math.min(box.right, rect.right);
    box.bottom = Math.min(box.bottom, rect.bottom);
  }
  return box;
}

export function shiftInto(left: number, right: number, box: Pick<Box, 'left' | 'right'>, margin = MARGIN): number {
  const lo = box.left + margin;
  const hi = box.right - margin;
  if (right - left > hi - lo) return lo - left;
  if (left < lo) return lo - left;
  if (right > hi) return hi - right;
  return 0;
}

export function nudgeInside(el: HTMLElement): void {
  el.style.marginLeft = '';
  el.style.marginRight = '';
  el.style.maxWidth = '';
  if (!el.offsetWidth) return;
  const box = clipBox(el);
  const room = box.right - box.left - MARGIN * 2;
  let rect = el.getBoundingClientRect();
  const scale = rect.width / el.offsetWidth || 1;
  if (room > 0 && rect.width > room) {
    el.style.maxWidth = `${Math.floor(room / scale)}px`;
    rect = el.getBoundingClientRect();
  }
  const dx = shiftInto(rect.left, rect.right, box);
  if (Math.abs(dx) < 0.5) return;
  el.style.marginLeft = `${(dx / scale).toFixed(1)}px`;
  if (Math.abs(el.getBoundingClientRect().left - rect.left - dx) < 1) return;
  el.style.marginLeft = '';
  el.style.marginRight = `${(-dx / scale).toFixed(1)}px`;
}

type Guarded = HTMLElement & { __edgeSafe?: { run: () => void; host: HTMLElement | null } };

export const vEdgeSafe: Directive<HTMLElement> = {
  mounted(el) {
    const node = el as Guarded;
    let frame = 0;
    const run = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => nudgeInside(el));
    };
    const host = el.parentElement;
    host?.addEventListener('pointerenter', run);
    host?.addEventListener('focusin', run);
    window.addEventListener('resize', run);
    node.__edgeSafe = { run, host };
    run();
  },
  updated(el) {
    (el as Guarded).__edgeSafe?.run();
  },
  unmounted(el) {
    const state = (el as Guarded).__edgeSafe;
    if (!state) return;
    state.host?.removeEventListener('pointerenter', state.run);
    state.host?.removeEventListener('focusin', state.run);
    window.removeEventListener('resize', state.run);
  },
};
