import { PAGE_WINDOW_MS, removeVeil } from '../../lib/motion/pageFlight';
import { guardScroll } from '../../lib/motion/scrollGuard';
import { main, noop } from './links';
import type { Live, MorphState, Stale } from './types';

const RETIRE_MS = 160;
export const MIN_TURN_MS = 16;

export const createLedger = (state: MorphState) => {
  const takeHold = () => {
    const release = state.hold ?? noop;
    state.hold = null;
    return release;
  };
  const lockScroll = () => {
    const root = main();
    if (!state.guard && root) state.guard = guardScroll(root, () => state.live?.hurry());
  };
  const unlockScroll = (apply: boolean) => {
    const held = state.guard;
    state.guard = null;
    held?.release(apply);
  };

  const track = (el: Element, done: () => void) => {
    let called = false;
    const once = () => {
      if (called) return;
      called = true;
      if (state.pending.get(el) === once) state.pending.delete(el);
      done();
    };
    state.pending.set(el, once);
    return once;
  };


  const disposePage = (job: Live) => {
    if (job.pageGone) return;
    job.pageGone = true;
    job.flight.dispose();
    if (job.underGone) job.release();
  };
  const disposeUnder = (job: Live) => {
    if (job.underGone) return;
    job.underGone = true;
    removeVeil(job.under, job.veil);
    job.hidden?.cancel();
    if (job.pageGone) job.release();
  };
  const owns = (job: Live | null, el: Element) => !!job && (job.page === el || job.under === el);

  const freeze = (turn = false) => {
    if (state.stale) finishStale(state.stale);
    const job = state.live;
    state.live = null;
    if (!job || job.landed) return;
    job.stop();
    job.go?.();
    const anims = [...job.flight.anims, ...(job.veilAnim ? [job.veilAnim] : []), ...(job.kind === 'collapse' && job.hidden ? [job.hidden] : [])];
    const elapsed = Number(job.flight.main.currentTime ?? 0);
    if (turn) {
      job.flight.turn();
      for (const extra of anims.slice(job.flight.anims.length)) extra.updatePlaybackRate(-(extra.playbackRate || 1));
    } else {
      for (const animation of anims) animation.pause();
    }
    const veilNow = job.veil ? Number.parseFloat(getComputedStyle(job.veil).opacity) : 0;
    state.stale = {
      ...job, snap: job.flight.snapshot(), veilNow: Number.isFinite(veilNow) ? veilNow : 0, claimed: new Set(),
      elapsed: Number.isFinite(elapsed) ? Math.min(PAGE_WINDOW_MS, Math.max(MIN_TURN_MS, elapsed)) : PAGE_WINDOW_MS,
      turned: turn,
    };
  };
  const finishStale = (job: Stale) => {
    if (state.stale === job) state.stale = null;
    disposePage(job);
    disposeUnder(job);
    job.pageDone();
    job.underDone();
  };
  const dropIfDone = () => { if (state.stale && state.stale.pageGone && state.stale.underGone) state.stale = null; };
  const retire = (el: HTMLElement, done: () => void, after: () => void) => {
    const now = Number.parseFloat(getComputedStyle(el).opacity);
    const fade = el.animate([{ opacity: Number.isFinite(now) ? now : 1 }, { opacity: 0 }], { duration: RETIRE_MS, easing: 'ease-out', fill: 'forwards' });
    const end = () => { after(); done(); fade.cancel(); dropIfDone(); };
    fade.finished.then(end, end);
  };
  const adopt = (mine: Array<Element | null>) => {
    const job = state.stale;
    if (!job) return null;
    if (mine.includes(job.page)) { job.claimed.add(job.page); disposePage(job); }
    if (job.under && mine.includes(job.under)) { job.claimed.add(job.under); disposeUnder(job); }
    return job;
  };

  const settle = () => {
    const job = state.stale;
    if (!job) return;
    const leavingPage = job.kind === 'collapse';
    if (!job.claimed.has(job.page)) {
      if (leavingPage) retire(job.page, job.pageDone, () => disposePage(job));
      else { disposePage(job); job.pageDone(); }
    }
    if (job.under && !job.claimed.has(job.under)) {
      if (!leavingPage) retire(job.under, job.underDone, () => disposeUnder(job));
      else { disposeUnder(job); job.underDone(); }
    }
    dropIfDone();
    sweepSoon();
  };

  let sweepTimer = 0;
  const busy = (el: Element) =>
    owns(state.live, el) || owns(state.stale, el) || state.pendingCollapse?.el === el || state.pendingCollapse?.staying === el || state.expandLeaving?.el === el;
  const sweep = () => {
    for (const [el, done] of [...state.pending]) {
      if (!busy(el)) done();
    }
    for (const layer of main()?.querySelectorAll<HTMLElement>('.page-host > * > [data-morph-layer]') ?? []) {
      const page = layer.parentElement;
      if (!page || busy(page)) continue;
      if (import.meta.env.DEV) console.warn('[page-morph] stray morph layer swept', layer.className || layer.tagName);
      if (layer.classList.contains('page-veil')) removeVeil(page, layer);
      else layer.remove();
    }
  };
  const sweepSoon = () => {
    window.clearTimeout(sweepTimer);
    sweepTimer = window.setTimeout(sweep, PAGE_WINDOW_MS * 2 + 400);
  };

  const clean = (el: Element) => {
    if (!(el instanceof HTMLElement)) return;
    const busy = owns(state.live, el) || owns(state.stale, el);
    if (!busy && state.entered.has(el)) {
      for (const animation of el.getAnimations()) {
        if (animation instanceof CSSAnimation || animation instanceof CSSTransition) continue;
        animation.cancel();
      }
      el.querySelectorAll(':scope > [data-morph-layer]').forEach((layer) => layer.remove());
      el.style.isolation = '';
      el.style.position = '';
      el.style.clipPath = '';
    }
    el.style.transformOrigin = '';
    el.style.opacity = '';
    el.style.transition = '';
    el.style.zIndex = '';
  };

  const claim = (el: Element) => { if (state.stale && owns(state.stale, el)) state.stale.claimed.add(el); };

  return {
    takeHold, lockScroll, unlockScroll, track, disposePage, disposeUnder, owns, freeze, finishStale, dropIfDone, retire, adopt,
    settle, sweepSoon, clean, claim,
  };
};

export type Ledger = ReturnType<typeof createLedger>;
