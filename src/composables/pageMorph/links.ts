import type { RouteLocationNormalized } from 'vue-router';
import { lessMotion } from '../../lib/motion/lessMotion';
import type { WindowRect } from '../../lib/motion/window';

export type Rect = WindowRect;

const MIN_WIDTH = 140;
const MIN_HEIGHT = 56;

export const rectOf = (el: Element): Rect => {
  const box = el.getBoundingClientRect();
  return { left: box.left, top: box.top, width: box.width, height: box.height };
};
export const main = () => document.getElementById('main-content');
export const bigEnough = (el: Element | null): boolean => {
  if (!(el instanceof HTMLElement)) return false;
  const box = el.getBoundingClientRect();
  return box.width >= MIN_WIDTH && box.height >= MIN_HEIGHT;
};
export const radiusOf = (el: Element) => Number.parseFloat(getComputedStyle(el).borderTopLeftRadius) || 22;
export const reducedMotion = () => lessMotion();
export const noop = () => undefined;

export const cardOfLink = (link: HTMLElement): HTMLElement | null => {
  if (link.closest('[data-no-morph]')) return null;
  const marked = link.closest<HTMLElement>('[data-morph-card]');
  if (marked) return marked;
  const box = bigEnough(link) ? link : link.closest<HTMLElement>('.metric-panel, .surface-card, .trend-card, li, section');
  return box && bigEnough(box) ? box : null;
};

export const linkOf = (target: EventTarget | null): HTMLElement | null => {
  const el = target instanceof Element ? target : null;
  const link = el?.closest<HTMLElement>('a[href]');
  if (!link || link.closest('.shell-head, .bottom-nav')) return null;
  if (main()?.contains(link)) return link;
  return link.closest('[data-morph-card]') ? link : null;
};

export const linksTo = (href: string, scope: string) => {
  const root = main();
  return root ? [...root.querySelectorAll<HTMLElement>(`${scope} a[href="${CSS.escape(href)}"]`)] : [];
};
export const STAYING = '.page-host > :not([class*="-leave"])';

export type Trail = { back: string; href: string; index: number; rect: Rect; radius: number; source?: string; small?: boolean };

let armed: { card: HTMLElement; href: string; id: string; at: number; anySize: boolean } | null = null;
let armSeq = 0;
export const armPageMorph = (card: HTMLElement | null, href: string, options: { anySize?: boolean } = {}): void => {
  if (!card) { armed = null; return; }
  const id = card.dataset.morphSource || `m${(armSeq += 1)}`;
  card.dataset.morphSource = id;
  armed = { card, href, id, at: performance.now(), anySize: !!options.anySize };
};
export const takeArmed = (to: RouteLocationNormalized) => {
  const hit = armed && performance.now() - armed.at < 1500 && (armed.href === to.fullPath || armed.href === to.path) && armed.card.isConnected ? armed : null;
  armed = null;
  return hit;
};
export type Origin = { card: HTMLElement; rect: Rect; radius: number; replica: { el: HTMLElement; rect: Rect } };

export const viewportOf = (): Rect | null => {
  const root = main();
  if (!root) return null;
  const box = rectOf(root);
  return { left: box.left, top: box.top, width: root.clientWidth, height: root.clientHeight };
};

export const currentCard = (trail: Trail): HTMLElement | null => {
  if (trail.source) {
    const sel = `[data-morph-source="${CSS.escape(trail.source)}"]`;
    return main()?.querySelector<HTMLElement>(`${STAYING} ${sel}`)
      ?? document.querySelector<HTMLElement>(`.app-topbar ${sel}`)
      ?? null;
  }
  const link = linksTo(trail.href, STAYING)[trail.index]
    ?? document.querySelector<HTMLElement>(`body > :not(#app) a[data-morph-card][href="${CSS.escape(trail.href)}"]`)
    ?? null;
  return link ? cardOfLink(link) : null;
};
