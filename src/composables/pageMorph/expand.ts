import { holdMotion, stageMotion, whenSlotsDrained } from '../../lib/motion/budget';
import { findFocusTarget, focusScrollDelta, revealFocusLater } from '../../lib/motion/focusTarget';
import { hurryAnimation, onMotionEscape } from '../../lib/motion/interrupt';
import {
  backdropFor, contentIn, fly, hide, PAGE_WINDOW_EASE, PAGE_WINDOW_MS, pageVeil, plateIn, replicaOut, settleTransitions, veilIn, windowBack,
} from '../../lib/motion/pageFlight';
import { bleedRect } from '../../lib/motion/window';
import { main, noop, rectOf, type Origin, type Rect } from './links';
import type { MorphCtx } from './context';
import type { Live } from './types';

export const HURRY_MS = 180;
const CHARTS_DRAIN_MS = 260;
const MIN_REVERSE_MS = 180;

export const startExpand = (ctx: MorphCtx, el: HTMLElement, done: () => void, origin: Origin, viewport: Rect, host: HTMLElement, focus: string | null) => {
  const leaving = ctx.state.expandLeaving;
  ctx.state.expandLeaving = null;
  el.classList.add('page-revisit');
  const previous = ctx.ledger.adopt([el, leaving?.el ?? null]);
  const reversing = previous && previous.kind === 'collapse' && previous.path === ctx.state.expandPath ? previous : null;
  if (reversing && reversing.page !== el && !reversing.claimed.has(reversing.page)) {
    reversing.claimed.add(reversing.page);
    ctx.ledger.disposePage(reversing);
    reversing.pageDone();
  }
  const scroller = main();
  const target = focus ? findFocusTarget(el, focus) : null;
  if (target && scroller) {
    const before = scroller.scrollTop;
    scroller.scrollTop = before + focusScrollDelta(target, scroller, el);
    const moved = scroller.scrollTop - before;
    if (leaving && moved) leaving.el.style.top = `${(Number.parseFloat(leaving.el.style.top) || 0) + moved}px`;
  }
  const box = rectOf(el);
  const spot = target ? rectOf(target) : null;
  const focusAt = spot ? { x: spot.left - box.left, y: spot.top - box.top } : { x: 0, y: 0 };
  const focusPart = target ? [...el.children].find((child) => child.contains(target)) : undefined;
  const start = reversing?.snap.window ?? origin.rect;
  const duration = reversing ? Math.max(MIN_REVERSE_MS, reversing.elapsed) : PAGE_WINDOW_MS;
  const flight = fly({
    page: el,
    backdrop: backdropFor(el, viewport, host, origin.radius),
    radius: origin.radius,
    from: { rect: start, anchor: { x: start.left - focusAt.x, y: start.top - focusAt.y } },
    to: { rect: bleedRect(viewport, origin.radius), anchor: { x: box.left, y: box.top } },
    duration,
    easing: PAGE_WINDOW_EASE,
    replica: origin.replica,
    replicaAt: focusAt,
    replicaFade: replicaOut(reversing ? reversing.snap.replica : 1),
    head: contentIn(reversing ? reversing.snap.content : 0),
    body: contentIn(reversing ? reversing.snap.content : 0),
    plate: plateIn(reversing ? reversing.snap.plate : 0),
    fade: reversing && reversing.snap.page < 1 ? windowBack(reversing.snap.page) : undefined,
    early: focusPart ? (part) => part === focusPart : undefined,
  });
  const veil = leaving ? pageVeil(leaving.el, viewport) : null;
  settleTransitions(el, leaving?.el);
  const veilFrom = veilIn(previous ? previous.veilNow : 0, duration);
  const veilAnim = veil ? veil.animate(veilFrom.frames, veilFrom.options) : null;
  const hidden = origin.card.isConnected && main()?.contains(origin.card) ? hide(origin.card) : null;
  let release: () => void = noop;
  const job: Live = {
    kind: 'expand', page: el, path: ctx.state.expandPath, under: leaving?.el ?? null, flight, veil, veilAnim, hidden,
    release: () => release(), pageDone: done, underDone: leaving?.done ?? noop, stop: noop, hurry: noop,
    landed: false, pageGone: false, underGone: false,
  };
  ctx.state.live = job;
  ctx.ledger.dropIfDone();
  const all = () => [...flight.anims, ...(veilAnim ? [veilAnim] : [])];
  const staged = stageMotion();
  ctx.ledger.takeHold()();
  let drained = false;
  const lockAfterDrain = () => {
    if (drained) return;
    drained = true;
    if (ctx.state.live === job && !job.landed) {
      release = holdMotion();
    }
    staged();
  };
  void whenSlotsDrained(CHARTS_DRAIN_MS).then(lockAfterDrain);
  job.hurry = () => {
    if (ctx.state.live !== job || job.landed) return;
    lockAfterDrain();
    for (const animation of all()) hurryAnimation(animation, HURRY_MS);
  };
  const forgetEscape = onMotionEscape(() => {
    if (ctx.state.live !== job || job.landed) return false;
    ctx.back();
    return true;
  });
  job.stop = forgetEscape;
  const finish = () => {
    if (ctx.state.live !== job || job.landed) return;
    job.landed = true;
    ctx.state.live = null;
    forgetEscape();
    ctx.ledger.disposeUnder(job);
    job.underDone();
    job.pageDone();
    ctx.ledger.disposePage(job);
    release();
    ctx.ledger.sweepSoon();
    ctx.ledger.unlockScroll(true);
    if (focus) revealFocusLater(el, focus);
  };
  flight.main.finished.then(finish, noop);
};
