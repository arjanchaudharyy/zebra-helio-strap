import { computed, nextTick, onBeforeUnmount, ref, shallowRef, watch, type Ref } from 'vue';
import type { IconName } from '../../components/Icon.vue';
import { slotRoute, strandRoute, traceEven, type Pt, type ThreadTree } from '../../lib/aiTask/threads';
import {
  cancelPlayed, cardStep, chipStep, gateRing, gateStep, GATE_AT, FLY_MS, mergeSteps, planOf, play, popStep, positionOf, reelStep, rewind, riseStep,
  STAGGER, CHIP_MS, type Played, type Step,
} from '../../lib/motion/cards/attach';
import { animateFromNow, reducedMotion, settled } from '../../lib/motion/cards';
import { paced } from '../../lib/motion/timing';
import type { StageCardModel } from './useStageCards';
import type { FutureFace } from './useStageSend';

export interface AttachItem { id: string; icon: IconName; tint: string; title: string; meta: string }

interface Options {
  stage: Ref<HTMLElement | null>;
  face: Ref<FutureFace>;
  future: () => { front: HTMLElement | null; body: HTMLElement | null } | null;
  tree: () => ThreadTree | null;
  lock: () => { x: number; y: number; r: number };
  cards: () => Array<{ card: StageCardModel; side: 'left' | 'right' }>;
  slotOf: (id: string) => HTMLElement | null;
}

const FADE_MS = paced(260);

export const useAttachFlight = (o: Options) => {
  const away = computed(() => o.face.value === 'receipt');
  const tray = ref(false);
  const items = shallowRef<AttachItem[]>([]);
  const landed = ref(0);
  const lit = ref(false);
  const busy = ref(false);

  let flight: Played | null = null;
  let merged: Played | null = null;
  let rings: HTMLElement[] = [];
  let lands: number[] = [];
  let fades: Animation[] = [];
  let raf = 0;

  const host = () => o.stage.value;
  const q = <T extends Element>(sel: string) => host()?.querySelector<T>(sel) ?? null;
  const qa = <T extends Element>(sel: string, root: ParentNode | null | undefined = host()) => [...(root?.querySelectorAll<T>(sel) ?? [])];
  const lockEl = () => q<HTMLElement>('.bridge-lock .lock');
  const chipOf = (id: string) => q<HTMLElement>(`.attach-tray .chip[data-id="${CSS.escape(id)}"]`);
  const chips = () => qa<HTMLElement>('.attach-tray .chip');
  const pieces = () => qa<HTMLElement>('[data-enter]', o.future()?.front);
  const paths = () => qa<SVGPathElement>('.threads .tree path');
  const buttons = () => o.cards().map((c) => o.slotOf(c.card.id)?.querySelector<HTMLElement>('.slot-button')).filter((el): el is HTMLElement => !!el);
  const canFly = () => !!o.tree() && !!host() && !reducedMotion();

  const count = () => {
    cancelAnimationFrame(raf);
    const tick = () => {
      if (!flight) return;
      const at = positionOf(flight);
      const next = lands.filter((t) => t <= at + 1).length;
      if (next !== landed.value) landed.value = next;
      raf = requestAnimationFrame(tick);
    };
    tick();
  };

  const flightPlan = (): Step[] | null => {
    const tree = o.tree();
    const stage = host();
    const lock = lockEl();
    if (!tree || !stage || !lock) return null;
    const box = stage.getBoundingClientRect();
    const view = (p: Pt) => ({ x: p.x + box.left, y: p.y + box.top });
    const l = o.lock();
    const center = { x: l.x, y: l.y };
    const list = o.cards();
    const trunkOf = new Map(tree.strands.map((s) => [s.id, s.trunk]));
    const lastOn = new Map<number, string>();
    for (const c of list) lastOn.set(trunkOf.get(c.card.id) ?? -1, c.card.id);
    const steps: Step[] = [];
    lands = [];
    rings = [];
    let firstChip = Infinity;
    list.forEach((c, i) => {
      const slot = o.slotOf(c.card.id);
      const el = slot?.querySelector<HTMLElement>('.scard');
      const turn = slot?.querySelector('.slot-button');
      const trace = traceEven(strandRoute(tree, c.card.id, center), 28);
      if (!el || !turn || !trace.points.length) return;
      const delay = i * STAGGER;
      const [fork = 0.5, origin = 0.9] = trace.marks.slice(-3);
      steps.push(cardStep(el, turn, c.side, trace.points.map(view), delay));
      const branch = q(`.threads .branch[data-id="${CSS.escape(c.card.id)}"]`);
      if (branch) steps.push(reelStep(branch, 0, fork, delay));
      const trunk = trunkOf.get(c.card.id);
      const trunkEl = trunk === undefined ? null : q(`.threads .trunk[data-i="${trunk}"]`);
      if (trunkEl && lastOn.get(trunk!) === c.card.id) steps.push(reelStep(trunkEl, fork, origin, delay));
      const gate = delay + FLY_MS * GATE_AT;
      const ring = gateRing(lock);
      rings.push(ring);
      steps.push(gateStep(ring, gate));
      const chip = chipOf(c.card.id);
      if (!chip) return;
      const r = chip.getBoundingClientRect();
      const goal = { x: r.left + r.width / 2 - box.left, y: r.top + r.height / 2 - box.top };
      steps.push(chipStep(chip, traceEven(slotRoute(tree, center, l.r, goal), 24).points.map(view), gate));
      firstChip = Math.min(firstChip, gate);
      lands.push(gate + CHIP_MS * 0.62);
      const counter = q('.attach-tray .count');
      if (counter) steps.push(popStep(counter, gate + CHIP_MS * 0.62 - 40));
    });
    const panel = q('.attach-tray .tray-panel');
    if (panel && Number.isFinite(firstChip)) steps.push(riseStep(panel, Math.max(0, firstChip - paced(160)), paced(320), 8));
    return steps;
  };

  const fade = (out: boolean): Promise<void> => {
    for (const a of fades) a.cancel();
    const line = (el: Element) => el.animate(out ? [{ opacity: 1, strokeDashoffset: '0' }, { opacity: 0, strokeDashoffset: '0' }] : [{ opacity: 0, strokeDashoffset: '0' }, { strokeDashoffset: '0' }], { duration: FADE_MS, easing: 'ease-out', fill: out ? 'forwards' : 'none' });
    const card = (el: Element) => el.animate(out ? [{ opacity: 1 }, { opacity: 0 }] : [{ opacity: 0 }, { opacity: 1 }], { duration: FADE_MS, easing: 'ease-out', fill: out ? 'forwards' : 'none' });
    fades = [...buttons().map(card), ...paths().map(line)];
    return settled(fades);
  };
  const dropFades = () => { for (const a of fades) a.cancel(); fades = []; };

  const cleanup = () => {
    cancelAnimationFrame(raf);
    cancelPlayed(flight);
    cancelPlayed(merged);
    flight = null;
    merged = null;
    for (const ring of rings) ring.remove();
    rings = [];
    dropFades();
    tray.value = false;
    landed.value = 0;
    lit.value = false;
    busy.value = false;
  };

  const snapshot = () => { items.value = o.cards().map(({ card }) => ({ id: card.id, icon: card.icon, tint: card.tint, title: card.title, meta: card.line })); };

  const attach = async () => {
    busy.value = true;
    lit.value = true;
    snapshot();
    landed.value = 0;
    if (!canFly()) { await fade(true); return; }
    tray.value = true;
    await nextTick();
    const steps = flightPlan();
    if (!steps) { tray.value = false; await fade(true); return; }
    flight = play(planOf(steps));
    count();
    await settled([...flight.animations, flight.clock]);
  };

  const retract = async () => {
    if (flight) {
      await rewind(flight);
    } else if (fades.length) {
      const back = [...buttons(), ...paths()].map((el) => animateFromNow(el, { opacity: 1 }, { duration: paced(220), easing: 'ease-out', fill: 'forwards' }));
      dropFades();
      await settled(back);
      for (const a of back) a.cancel();
    }
    cleanup();
  };

  const merge = async (swap: () => void) => {
    const body = o.future()?.body ?? null;
    const out = body?.animate([{ opacity: 1 }, { opacity: 0 }], { duration: paced(180), easing: 'cubic-bezier(.4, 0, 1, 1)', fill: 'forwards' });
    await out?.finished.catch(() => undefined);
    swap();
    await nextTick();
    cancelPlayed(flight);
    flight = null;
    cancelAnimationFrame(raf);
    for (const ring of rings) ring.remove();
    rings = [];
    dropFades();
    const tiles = chips();
    if (tiles.length && !reducedMotion()) {
      merged = play(planOf(mergeSteps(tiles, q('.attach-tray .tray-panel'), pieces())));
      out?.cancel();
      await settled(merged.animations);
    } else {
      const fadeIn = body?.animate([{ opacity: 0 }, { opacity: 1 }], { duration: FADE_MS, easing: 'ease-out' });
      out?.cancel();
      await fadeIn?.finished.catch(() => undefined);
    }
    cleanup();
  };

  const detach = async () => {
    if (busy.value) return;
    const body = o.future()?.body ?? null;
    busy.value = true;
    if (!canFly() || !body) {
      await body?.animate([{ opacity: 1 }, { opacity: 0 }], { duration: paced(180), fill: 'forwards' }).finished.catch(() => undefined);
      o.face.value = 'question';
      await nextTick();
      body?.getAnimations().forEach((a) => a.cancel());
      body?.animate([{ opacity: 0 }, { opacity: 1 }], { duration: FADE_MS, easing: 'ease-out' });
      await fade(false);
      cleanup();
      return;
    }
    snapshot();
    lit.value = true;
    tray.value = true;
    landed.value = items.value.length;
    await nextTick();
    merged = play(planOf(mergeSteps(chips(), q('.attach-tray .tray-panel'), pieces())), true);
    await settled(merged.animations);
    o.face.value = 'question';
    await nextTick();
    cancelPlayed(merged);
    merged = null;
    body.animate([{ opacity: 0 }, { opacity: 1 }], { duration: paced(260), easing: 'ease-out' });
    const steps = flightPlan();
    if (steps) {
      flight = play(planOf(steps), true);
      count();
      await settled([...flight.animations, flight.clock]);
    }
    cleanup();
  };

  watch(away, (on) => { if (!busy.value) void fade(on).then(() => { if (!busy.value) dropFades(); }); });
  onBeforeUnmount(cleanup);

  return { away, tray, items, landed, lit, busy, attach, merge, retract, detach };
};
