import type { RouteLocationNormalized } from 'vue-router';
import { lessMotion } from './lessMotion';

export const focusKeyOf = (route: RouteLocationNormalized): string | null =>
  (typeof route.query.focus === 'string' && route.query.focus ? route.query.focus : null);

export const findFocusTarget = (page: Element, key: string): HTMLElement | null =>
  page.querySelector<HTMLElement>(`[data-focus-key="${CSS.escape(key)}"]`)
  ?? page.querySelector<HTMLElement>(`[data-focus-keys~="${CSS.escape(key)}"]`);

const TOP_GAP = 96;

export const focusScrollDelta = (target: HTMLElement, scroller: HTMLElement, page?: HTMLElement): number => {
  const box = target.getBoundingClientRect();
  const view = scroller.getBoundingClientRect();
  const wanted = box.top - (view.top + TOP_GAP);
  let bottom = scroller.scrollHeight;
  if (page) {
    const pad = Number.parseFloat(getComputedStyle(scroller).paddingBottom) || 0;
    bottom = Math.min(bottom, page.getBoundingClientRect().bottom - view.top - scroller.clientTop + scroller.scrollTop + pad);
  }
  const max = Math.max(0, bottom - scroller.clientHeight - scroller.scrollTop);
  return Math.max(-scroller.scrollTop, Math.min(wanted, max));
};

const reducedMotion = () => lessMotion();

export const ringFocus = (target: HTMLElement): void => {
  const ring = document.createElement('span');
  ring.setAttribute('aria-hidden', 'true');
  ring.className = 'focus-ring';
  Object.assign(ring.style, {
    position: 'absolute',
    inset: '0',
    zIndex: '5',
    borderRadius: 'inherit',
    pointerEvents: 'none',
    opacity: '0',
    boxShadow: 'inset 0 0 0 2px var(--accent), 0 0 0 4px color-mix(in srgb, var(--accent) 22%, transparent), '
      + '0 0 28px color-mix(in srgb, var(--accent) 28%, transparent)',
  });
  if (getComputedStyle(target).position === 'static') target.style.position = 'relative';
  target.appendChild(ring);
  const frames: Keyframe[] = reducedMotion()
    ? [{ opacity: 1 }, { opacity: 1, offset: 0.85 }, { opacity: 0 }]
    : [{ opacity: 0 }, { opacity: 1, offset: 0.14 }, { opacity: 0.45, offset: 0.4 }, { opacity: 1, offset: 0.56 }, { opacity: 1, offset: 0.72 }, { opacity: 0 }];
  const animation = ring.animate(frames, { duration: 1700, easing: 'ease-in-out', fill: 'forwards' });
  const drop = () => ring.remove();
  animation.finished.then(drop, drop);
};

const STEADY_FRAMES = 6;
const SCROLL_SETTLE_MS = 700;

export const revealFocusLater = (page: HTMLElement, key: string, waitMs = 2500): void => {
  const scroller = document.getElementById('main-content');
  if (!scroller) return;
  const started = performance.now();
  const startTop = scroller.scrollTop;
  let lastHeight = -1;
  let steady = 0;
  let opened = false;
  const look = () => {
    if (!page.isConnected) return;
    const late = performance.now() - started > waitMs;
    const target = findFocusTarget(page, key);
    if (!target) {
      if (!late) requestAnimationFrame(look);
      return;
    }
    if (!opened && target instanceof HTMLDetailsElement) target.open = true;
    opened = true;
    const height = page.getBoundingClientRect().height;
    steady = Math.abs(height - lastHeight) < 1 ? steady + 1 : 0;
    lastHeight = height;
    if (steady < STEADY_FRAMES && !late) {
      requestAnimationFrame(look);
      return;
    }
    const userScrolled = Math.abs(scroller.scrollTop - startTop) > 4;
    const delta = userScrolled ? 0 : focusScrollDelta(target, scroller, page);
    const box = target.getBoundingClientRect();
    const view = scroller.getBoundingClientRect();
    const alreadyInSafeZone = !userScrolled
      && box.top >= view.top + 64
      && box.bottom <= view.bottom - 24
      && Math.abs(delta) < 120;
    if (Math.abs(delta) <= 4 || alreadyInSafeZone) {
      ringFocus(target);
      return;
    }
    const smooth = !reducedMotion();
    let rung = false;
    const ring = () => {
      if (rung) return;
      rung = true;
      scroller.removeEventListener('scrollend', ring);
      if (target.isConnected) ringFocus(target);
    };
    if (smooth) {
      scroller.addEventListener('scrollend', ring, { once: true });
      window.setTimeout(ring, SCROLL_SETTLE_MS);
    }
    scroller.scrollBy({ top: delta, behavior: smooth ? 'smooth' : 'auto' });
    if (!smooth) ring();
  };
  requestAnimationFrame(look);
};
