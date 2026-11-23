import type { RouteLocationNormalized } from 'vue-router';
import type { PageMotion } from '../lib/navigation';
import { holdMotion } from '../lib/motion/budget';
import { cardReplica } from '../lib/motion/window';
import { focusKeyOf, revealFocusLater } from '../lib/motion/focusTarget';
import { backdropFor } from '../lib/motion/pageFlight';
import { startCollapse } from './pageMorph/collapse';
import type { MorphCtx } from './pageMorph/context';
import { startExpand } from './pageMorph/expand';
import { createLedger } from './pageMorph/ledger';
import {
  bigEnough, cardOfLink, linkOf, linksTo, main, noop, radiusOf, rectOf, reducedMotion, takeArmed, viewportOf, type Origin, type Trail,
} from './pageMorph/links';
import { createState, type Collapse } from './pageMorph/types';

export { armPageMorph } from './pageMorph/links';

export const usePageMorph = (options: { back: () => void }) => {
  const state = createState();
  const ledger = createLedger(state);
  const ctx: MorphCtx = { state, ledger, back: options.back };
  const { takeHold, track } = ledger;
  let pressed: HTMLElement | null = null;
  let expandFrom: Origin | null = null;
  let collapseTo: Trail | null = null;
  let collapsePath = '';
  let collapseLift = 0;
  let focusKey: string | null = null;

  const remember = (event: Event) => { pressed = linkOf(event.target); };
  const rememberKey = (event: KeyboardEvent) => {
    if (event.key === 'Enter' || event.key === ' ') remember(event);
  };
  document.addEventListener('pointerdown', remember, true);
  document.addEventListener('keydown', rememberKey, true);
  const dispose = () => {
    document.removeEventListener('pointerdown', remember, true);
    document.removeEventListener('keydown', rememberKey, true);
  };


  const decide = (from: RouteLocationNormalized, to: RouteLocationNormalized, base: PageMotion): PageMotion => {
    const link = pressed;
    pressed = null;
    if (expandFrom && state.expandPath === to.fullPath) return 'expand';
    if (state.pendingCollapse) {
      const job = state.pendingCollapse;
      state.pendingCollapse = null;
      cancelAnimationFrame(job.frame);
      job.backdrop.remove();
      job.done();
      job.stayingDone();
      job.release();
    }
    const turning = Boolean(state.live && !state.live.landed && state.live.kind === 'expand' && !reducedMotion()
      && state.trails.get(state.live.path)?.back === to.fullPath);
    ledger.freeze(turning);
    expandFrom = null;
    state.expandPath = '';
    if (state.expandLeaving) { const { done } = state.expandLeaving; state.expandLeaving = null; done(); }
    collapseTo = null;
    collapseLift = 0;
    focusKey = focusKeyOf(to);
    state.hold?.();
    state.hold = null;
    ledger.sweepSoon();
    const direct = takeArmed(to);
    const motion = choose(from, to, base, link, direct);
    if (motion === 'expand' && !state.hold) state.hold = holdMotion();
    if (motion === 'expand' || motion === 'collapse') ledger.lockScroll();
    else ledger.unlockScroll(false);
    return motion;
  };

  const choose = (
    from: RouteLocationNormalized, to: RouteLocationNormalized, base: PageMotion, link: HTMLElement | null, direct: ReturnType<typeof takeArmed>,
  ): PageMotion => {
    if (reducedMotion()) return base;
    if (direct && (direct.anySize || bigEnough(direct.card))) {
      const rect = rectOf(direct.card);
      const radius = Math.min(radiusOf(direct.card), rect.width / 2, rect.height / 2);
      expandFrom = { card: direct.card, rect, radius, replica: cardReplica(direct.card) };
      state.expandPath = to.fullPath;
      state.trails.set(to.fullPath, { back: from.fullPath, href: direct.href, index: 0, rect: expandFrom.rect, radius: expandFrom.radius, source: direct.id, small: !bigEnough(direct.card) });
      return 'expand';
    }
    if (link && (base === 'forward' || link.closest('[data-morph-card]'))) {
      const href = link.getAttribute('href') ?? '';
      const card = href === to.fullPath || href === to.path ? cardOfLink(link) : null;
      if (card) {
        expandFrom = { card, rect: rectOf(card), radius: radiusOf(card), replica: cardReplica(card) };
        state.expandPath = to.fullPath;
        state.trails.set(to.fullPath, {
          back: from.fullPath,
          href,
          index: Math.max(0, linksTo(href, '.page-host > *').indexOf(link)),
          rect: expandFrom.rect,
          radius: expandFrom.radius,
        });
        return 'expand';
      }
    }
    const trail = state.trails.get(from.fullPath);
    if (trail && trail.back === to.fullPath) {
      collapseTo = trail;
      collapsePath = from.fullPath;
      collapseLift = main()?.scrollTop ?? 0;
      state.hold = holdMotion();
      return 'collapse';
    }
    if (trail && from.path.startsWith('/ai/plan/') && to.path.startsWith('/ai/plan/')) {
      state.trails.set(to.fullPath, { ...trail, href: to.fullPath, index: 0 });
    } else if (trail && (from.path === to.path || (from.path.startsWith('/settings/') && to.path.startsWith('/settings/')))) {
      state.trails.set(to.fullPath, trail);
    }
    return base;
  };


  const onEnter = (el: Element, done: () => void = noop) => {
    ledger.claim(el);
    const job = state.stale;
    if (job && el instanceof HTMLElement && !expandFrom && !state.pendingCollapse) {
      if (job.page === el) ledger.disposePage(job);
      if (job.under === el) ledger.disposeUnder(job);
    }
    ledger.clean(el);
    if (el instanceof HTMLElement) el.style.top = '';
    const focus = focusKey;
    focusKey = null;
    if (state.entered.has(el)) el.classList.add('page-revisit');
    state.entered.add(el);
    if (state.pendingCollapse && el instanceof HTMLElement) {
      state.pendingCollapse.staying = el;
      state.pendingCollapse.stayingDone = track(el, done);
      return;
    }
    const origin = expandFrom;
    expandFrom = null;
    const viewport = viewportOf();
    const host = main()?.parentElement;
    if (!origin || !viewport || !host || !(el instanceof HTMLElement)) {
      if (origin) takeHold()();
      ledger.unlockScroll(false);
      if (state.expandLeaving) { const leaving = state.expandLeaving; state.expandLeaving = null; leaving.done(); }
      if (focus && el instanceof HTMLElement) revealFocusLater(el, focus);
      done();
      return;
    }
    startExpand(ctx, el, track(el, done), origin, viewport, host, focus);
  };

  const onLeave = (el: Element, done: () => void = noop) => {
    ledger.claim(el);
    const trail = collapseTo;
    collapseTo = null;
    if (!(el instanceof HTMLElement)) { done(); return; }
    if (expandFrom) {
      state.expandLeaving = { el, done: track(el, done) };
      return;
    }
    if (!trail) { done(); return; }
    const viewport = viewportOf();
    const host = main()?.parentElement;
    if (!viewport || !host) {
      takeHold()();
      done();
      return;
    }
    const job: Collapse = {
      el, trail, path: collapsePath, backdrop: backdropFor(el, viewport, host, trail.radius), staying: null, release: takeHold(), lift: collapseLift,
      done: track(el, done), stayingDone: noop, frame: 0,
    };
    state.pendingCollapse = job;
    job.frame = requestAnimationFrame(() => startCollapse(ctx, job));
  };

  const onAfterLeave = (el: Element) => {
    const job = state.stale;
    if (job && job.page === el) ledger.disposePage(job);
    if (job && job.under === el) ledger.disposeUnder(job);
    ledger.dropIfDone();
    ledger.clean(el);
    if (el instanceof HTMLElement) el.style.top = '';
  };
  // Vue's CSS timeout uses wall time, while WAAPI can be paused or slowed in DevTools.
  // Let the actual flight own completion on both pages; ordinary navigation keeps
  // Vue's automatic CSS completion (one-arg hooks).
  const onMorphEnter = (el: Element, done: () => void) => onEnter(el, done);
  const onMorphLeave = (el: Element, done: () => void) => onLeave(el, done);
  return { decide, onEnter: (el: Element) => onEnter(el), onLeave: (el: Element) => onLeave(el), onMorphEnter, onMorphLeave, onAfterLeave, settle: ledger.settle, dispose };
};
