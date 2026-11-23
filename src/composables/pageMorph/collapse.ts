import { onMotionEscape, onMotionSkip, hurryAnimation } from '../../lib/motion/interrupt';
import {
  cardIn, contentMorphOut, PAGE_LAND_AT, contentShrinkOut, fly, hide, PAGE_BACK_EASE, PAGE_BACK_MS, PAGE_WINDOW_MS, pageVeil, plateOut, replicaBack, replicaLateIn, replicaMorphIn, settleTransitions, veilOut, windowOut,
} from '../../lib/motion/pageFlight';
import { exemptFromSettle } from '../../lib/motion/interrupt';
import { settleCssAnimations } from '../../lib/motion/settle';
import { whenFramesSteady } from '../../lib/motion/steady';
import { bleedRect, cardReplica } from '../../lib/motion/window';
import { whenScrollRestored } from '../../lib/returnScroll';
import { currentCard, main, noop, rectOf, STAYING, viewportOf, type Trail } from './links';
import { MIN_TURN_MS } from './ledger';
import type { MorphCtx } from './context';
import type { Collapse, Live } from './types';
import { HURRY_MS } from './expand';

const MIN_REVERSE_MS = 180;

const CARD_WAIT_MS = 300;
const ESC_FINISH_MS = 200;
const LAND_FADE_MS = 140;
const PAINT_WAIT_MS = 120;

const findCard = (trail: Trail): Promise<HTMLElement | null> =>
  new Promise((resolve) => {
    const started = performance.now();
    let done = false;
    const forget = onMotionSkip(() => finish());
    const finish = () => {
      if (done) return;
      done = true;
      forget();
      resolve(currentCard(trail));
    };
    const look = () => {
      if (done) return;
      if (currentCard(trail) || performance.now() - started > CARD_WAIT_MS) finish();
      else requestAnimationFrame(look);
    };
    requestAnimationFrame(look);
  });

const settleStaying = () => settleCssAnimations(main()?.querySelector<HTMLElement>(STAYING));

export const startCollapse = (ctx: MorphCtx, job: Collapse) => {
  if (ctx.state.pendingCollapse === job) ctx.state.pendingCollapse = null;
  const { el, trail, backdrop } = job;
  const viewport = viewportOf();
  const host = main()?.parentElement;
  if (!el.isConnected || !viewport || !host) {
    backdrop.remove();
    job.done();
    job.stayingDone();
    job.release();
    ctx.ledger.unlockScroll(false);
    return;
  }
  settleStaying();
  const staying = job.staying ?? main()?.querySelector<HTMLElement>(STAYING) ?? null;
  const turned = ctx.state.stale && ctx.state.stale.turned && ctx.state.stale.kind === 'expand' && ctx.state.stale.page === el ? ctx.state.stale : null;
  if (turned) {
    const pose = turned.flight.pose();
    const veilNow = turned.veil ? Number.parseFloat(getComputedStyle(turned.veil).opacity) : 0;
    const left = Number(turned.flight.main.currentTime ?? 0);
    turned.snap = { ...turned.flight.snapshot(), window: pose.rect, anchor: pose.anchor };
    turned.veilNow = Number.isFinite(veilNow) ? veilNow : 0;
    turned.elapsed = Number.isFinite(left) ? Math.min(PAGE_WINDOW_MS, Math.max(MIN_TURN_MS, left)) : MIN_TURN_MS;
  }
  const previous = ctx.ledger.adopt([el, staying]);
  const reversing = previous && previous.kind === 'expand' && previous.page === el ? previous : null;
  const present = currentCard(trail);
  const target = present ? rectOf(present) : trail.rect;
  const box = rectOf(el);
  const duration = reversing ? Math.max(MIN_REVERSE_MS, reversing.elapsed) : PAGE_BACK_MS;
  const small = !!trail.small;
  const focusFrom = present ? el.querySelector('[data-morph-focus]') : null;
  const focusTo = focusFrom ? present?.querySelector<HTMLElement>('[data-morph-focus]') : null;
  const flight = fly({
    page: el,
    backdrop,
    radius: trail.radius,
    from: reversing ? { rect: reversing.snap.window, anchor: reversing.snap.anchor } : { rect: bleedRect(viewport, trail.radius), anchor: { x: box.left, y: box.top } },
    to: { rect: target, anchor: { x: target.left, y: target.top - job.lift } },
    duration,
    easing: PAGE_BACK_EASE,
    replica: present ? cardReplica(present) : null,
    replicaAt: { x: 0, y: job.lift },
    replicaFade: reversing && reversing.snap.content < 0.5
      ? replicaBack(reversing.snap.replica)
      : (small ? replicaLateIn : replicaMorphIn)(reversing ? reversing.snap.replica : 0),
    head: (small ? contentShrinkOut : contentMorphOut)(reversing ? reversing.snap.content : 1),
    body: (small ? contentShrinkOut : contentMorphOut)(reversing ? reversing.snap.content : 1),
    scale: true,
    scaleReplica: !small,
    focus: focusFrom && focusTo ? { el: focusFrom, to: rectOf(focusTo) } : undefined,
    plate: plateOut(reversing ? reversing.snap.plate : 1),
    fade: present ? windowOut(reversing ? reversing.snap.page : 1) : undefined,
    landAt: present ? PAGE_LAND_AT : undefined,
    early: job.lift > 40 ? (part) => {
      const shown = part.getBoundingClientRect();
      return shown.bottom > viewport.top && shown.top < viewport.top + viewport.height;
    } : undefined,
  });
  const veil = staying ? pageVeil(staying, viewport) : null;
  settleTransitions(el, staying);
  const veilFrom = veilOut(previous ? previous.veilNow : 1, duration);
  const veilAnim = veil ? veil.animate(veilFrom.frames, veilFrom.options) : null;
  const hidden = present && (main()?.contains(present) || present.closest('.app-topbar'))
    ? exemptFromSettle(present.animate(cardIn(), { duration, easing: 'linear', fill: 'both' }))
    : null;
  const me: Live = {
    kind: 'collapse', page: el, path: job.path, under: staying, flight, veil, veilAnim, hidden,
    release: job.release, pageDone: job.done, underDone: job.stayingDone, stop: noop, hurry: noop,
    landed: false, pageGone: false, underGone: false,
  };
  ctx.state.live = me;
  ctx.ledger.dropIfDone();
  if (!present) {
    void findCard(trail).then((found) => {
      if (!found || me.landed || ctx.state.live !== me || !el.isConnected) return;
      settleStaying();
      const settled = rectOf(found);
      me.hidden = hide(found);
      flight.retarget({ rect: settled, anchor: { x: settled.left, y: settled.top - job.lift } });
    });
  }
  if (present) {
    let aim = target;
    const follow = () => {
      if (me.landed || ctx.state.live !== me || !el.isConnected || !present.isConnected) return;
      const now = rectOf(present);
      if (Math.abs(now.left - aim.left) > 1.5 || Math.abs(now.top - aim.top) > 1.5) {
        aim = now;
        flight.retarget({ rect: now, anchor: { x: now.left, y: now.top - job.lift } });
      }
      requestAnimationFrame(follow);
    };
    requestAnimationFrame(follow);
  }
  const all = () => [...flight.anims, ...(veilAnim ? [veilAnim] : []), ...(hidden ? [hidden] : [])];
  if (!reversing) {
    for (const animation of all()) animation.pause();
    let started = false;
    const go = () => {
      if (started) return;
      started = true;
      if (me.landed || ctx.state.live !== me) return;
      settleTransitions(el, staying);
      for (const animation of all()) if (animation.playState === 'paused') animation.play();
    };
    me.go = go;
    void whenFramesSteady(PAINT_WAIT_MS, 3).then(go);
  }
  const hurry = (ms: number) => {
    me.go?.();
    for (const animation of all()) hurryAnimation(animation, ms);
  };
  me.hurry = () => { if (!me.landed && ctx.state.live === me) hurry(HURRY_MS); };
  const forgetEscape = onMotionEscape(() => {
    if (me.landed || ctx.state.live !== me) return false;
    hurry(ESC_FINISH_MS);
    return true;
  });
  me.stop = forgetEscape;
  const land = () => {
    if (me.landed || ctx.state.live !== me) return;
    me.landed = true;
    ctx.state.live = null;
    forgetEscape();
    ctx.ledger.disposeUnder(me);
    me.underDone();
    if (ctx.state.guard) { const held = ctx.state.guard; void whenScrollRestored().then(() => { if (ctx.state.guard === held) ctx.ledger.unlockScroll(true); }); }
    const leave = () => {
      me.pageDone();
      ctx.ledger.disposePage(me);
      ctx.ledger.sweepSoon();
    };
    if (present) {
      leave();
      return;
    }
    el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: LAND_FADE_MS, easing: 'ease-out', fill: 'forwards' })
      .finished.then(leave, leave);
  };
  flight.main.finished.then(land, noop);
};
