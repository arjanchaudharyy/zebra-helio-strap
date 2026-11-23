import { cardFrame } from '../deck/morph';
import { NEARLY_HIDDEN } from './flight';
import { exemptFromSettle } from './interrupt';
import { BACK_EASE, paced } from './timing';
import { settleCssAnimations } from './settle';
import { pageBackdrop, visibleWindow, WINDOW_OFFSETS, windowPoses, type WindowPose, type WindowRect } from './window';

type Rect = WindowRect;

const r1 = (value: number) => Number(value.toFixed(1));
const r4 = (value: number) => Number(value.toFixed(4));

const rectOf = (el: Element): Rect => {
  const box = el.getBoundingClientRect();
  return { left: box.left, top: box.top, width: box.width, height: box.height };
};

export const PAGE_WINDOW_MS = paced(600);
export const PAGE_WINDOW_EASE = 'cubic-bezier(.3, .1, .15, 1)';
const OUT_PART = 0.55;
const IN_FROM = 0.45;
const OPEN_AT = 0.04;
const OPEN_SPAN = 0.2;
export const PAGE_BACK_EASE = BACK_EASE;
export const PAGE_BACK_MS = PAGE_WINDOW_MS;
export const PAGE_LAND_AT = 0.8;
const BACK_AT = 0.4;
const BACK_SPAN = 0.22;
const CARD_IN_AT = PAGE_LAND_AT;
const CARD_IN_END = PAGE_LAND_AT + (1 - PAGE_LAND_AT) / 2;
const WINDOW_OUT_AT = CARD_IN_END;
const onShape = (offset: number) => offset * PAGE_LAND_AT;
const keep = (from: number, at: number, to: number, end: number): Keyframe[] => [
  { opacity: from }, { opacity: from, offset: at }, { opacity: to, offset: Math.min(1, end) }, { opacity: to },
];
export const replicaOut = (from = 1): Keyframe[] => keep(from, OPEN_AT, 0, OPEN_AT + OPEN_SPAN * OUT_PART);
export const contentIn = (from = 0): Keyframe[] => keep(Math.max(NEARLY_HIDDEN, from), OPEN_AT + OPEN_SPAN * IN_FROM, 1, OPEN_AT + OPEN_SPAN);
export const contentOut = (from = 1): Keyframe[] => keep(from, BACK_AT, 0, BACK_AT + BACK_SPAN * OUT_PART);
export const replicaIn = (from = 0): Keyframe[] => keep(Math.max(NEARLY_HIDDEN, from), BACK_AT + BACK_SPAN * IN_FROM, 1, BACK_AT + BACK_SPAN);
const MORPH_REPLICA = [onShape(0.1), onShape(0.46)] as const;
const MORPH_CONTENT = [onShape(0.18), onShape(0.56)] as const;
export const contentMorphOut = (from = 1): Keyframe[] => keep(from, MORPH_CONTENT[0], 0, MORPH_CONTENT[1]);
export const replicaMorphIn = (from = 0): Keyframe[] => keep(Math.max(NEARLY_HIDDEN, from), MORPH_REPLICA[0], 1, MORPH_REPLICA[1]);
const SMALL_CONTENT = [onShape(0.46), onShape(0.74)] as const;
const SMALL_REPLICA = [onShape(0.6), onShape(0.8)] as const;
export const contentShrinkOut = (from = 1): Keyframe[] => keep(from, SMALL_CONTENT[0], 0, SMALL_CONTENT[1]);
export const replicaLateIn = (from = 0): Keyframe[] => keep(Math.max(NEARLY_HIDDEN, from), SMALL_REPLICA[0], 1, SMALL_REPLICA[1]);
export const replicaBack = (from = 0): Keyframe[] => [{ opacity: from }, { opacity: 1, offset: 0.25 }, { opacity: 1 }];
export const plateIn = (from = 0): Keyframe[] => [{ opacity: Math.max(NEARLY_HIDDEN, from) }, { opacity: Math.max(NEARLY_HIDDEN, from), offset: 0.2 }, { opacity: 1, offset: 0.8 }, { opacity: 1 }];
export const plateOut = (from = 1): Keyframe[] => [{ opacity: from }, { opacity: from, offset: 0.2 }, { opacity: NEARLY_HIDDEN, offset: BACK_AT + BACK_SPAN }, { opacity: NEARLY_HIDDEN }];
export const windowOut = (from = 1): Keyframe[] => keep(from, WINDOW_OUT_AT, 0, 1);
export const windowBack = (from = 1): Keyframe[] => [{ opacity: Math.max(NEARLY_HIDDEN, from) }, { opacity: 1, offset: 0.2 }, { opacity: 1 }];
export const cardIn = (): Keyframe[] => keep(0, CARD_IN_AT, 1, CARD_IN_END);
export const veilIn = (from = 0, duration = PAGE_WINDOW_MS): { frames: Keyframe[]; options: KeyframeAnimationOptions } => ({
  frames: [{ opacity: Math.max(NEARLY_HIDDEN, from) }, { opacity: 1 }],
  options: { duration: duration * 0.7, easing: 'ease-out', fill: 'both' },
});
export const veilOut = (from = 1, duration = PAGE_WINDOW_MS): { frames: Keyframe[]; options: KeyframeAnimationOptions } => ({
  frames: [{ opacity: from }, { opacity: from * 0.85, offset: 0.3 }, { opacity: 0, offset: CARD_IN_END }, { opacity: 0 }],
  options: { duration, easing: 'linear', fill: 'both' },
});

export const pageVeil = (under: HTMLElement, viewport: Rect): HTMLElement => {
  const veil = document.createElement('div');
  veil.className = 'page-veil';
  veil.setAttribute('aria-hidden', 'true');
  veil.dataset.morphLayer = '';
  if (getComputedStyle(under).position === 'static') under.style.position = 'relative';
  under.style.isolation = 'isolate';
  under.appendChild(veil);
  const box = under.getBoundingClientRect();
  Object.assign(veil.style, {
    left: `${r1(viewport.left - box.left - under.clientLeft)}px`,
    top: `${r1(viewport.top - box.top - under.clientTop)}px`,
    width: `${r1(viewport.width)}px`,
    height: `${r1(viewport.height)}px`,
  });
  return veil;
};
export const removeVeil = (under: HTMLElement | null, veil: HTMLElement | null) => {
  veil?.getAnimations().forEach((animation) => animation.cancel());
  veil?.remove();
  if (!under) return;
  under.style.isolation = '';
  under.style.position = '';
};

const opacityOf = (el: Element | null | undefined) => (el ? Number.parseFloat(getComputedStyle(el).opacity) : Number.NaN);

export const partsOf = (page: HTMLElement) => {
  const children = [...page.children]
    .filter((child): child is HTMLElement => child instanceof HTMLElement && !child.hasAttribute('data-morph-layer'));
  const head = children.find((child) => child.matches('.page-header-wrap, .page-header, .page-heading')) ?? children[0] ?? null;
  return { head, body: children.filter((child) => child !== head) };
};

export const settleTransitions = (...roots: Array<Element | null | undefined>) => {
  const transitions = roots.flatMap((root) =>
    root?.isConnected ? root.getAnimations({ subtree: true }).filter((a) => a instanceof CSSTransition) : [],
  );
  for (const transition of transitions) transition.finish();
};

export const hide = (el: HTMLElement) =>
  exemptFromSettle(el.animate([{ opacity: 0 }, { opacity: 0 }], { duration: 1, fill: 'forwards' }));

export interface FlightSnapshot {
  window: Rect;
  anchor: { x: number; y: number };
  content: number;
  replica: number;
  plate: number;
  page: number;
}

export interface Flight {
  main: Animation;
  anims: Animation[];
  snapshot: () => FlightSnapshot;
  pose: () => WindowPose;
  turn: () => void;
  retarget: (to: WindowPose) => void;
  dispose: () => void;
}

export const backdropFor = (page: HTMLElement, viewport: Rect, host: HTMLElement, radius: number) =>
  pageBackdrop(page, rectOf(page), viewport, rectOf(host), radius);

const focusFramesFor = (
  o: { from: WindowPose; to: WindowPose; focus?: { el: Element; to: Rect }; replica: { rect: Rect } | null; replicaAt?: { x: number; y: number }; scaleReplica?: boolean },
  start: Rect,
  current: () => WindowPose,
) => {
  if (!o.focus) return null;
  const a = rectOf(o.focus.el);
  if (a.width < 1 || a.height < 1) return null;
  const card = o.replica?.rect ?? o.to.rect;
  const rel = { x: o.focus.to.left + o.focus.to.width / 2 - card.left, y: o.focus.to.top + o.focus.to.height / 2 - card.top, w: o.focus.to.width };
  const at = o.replicaAt ?? { x: 0, y: 0 };
  const scales = o.replica && o.scaleReplica !== false;
  const ca = { x: a.left + a.width / 2, y: a.top + a.height / 2 };
  return (part: Rect) => () => {
    const poses = windowPoses(o.from, current());
    const w0 = poses[0].rect.width, w1 = poses[poses.length - 1].rect.width;
    const origin = `${r1(ca.x - part.left)}px ${r1(ca.y - part.top)}px`;
    return poses.map((pose, index) => {
      const f = Math.abs(w1 - w0) > 0.5 ? (pose.rect.width - w0) / (w1 - w0) : index / Math.max(1, poses.length - 1);
      const k = scales ? pose.rect.width / Math.max(1, card.width) : 1;
      const g = 1 - (1 - f) ** 3;
      const goal = { x: pose.anchor.x + at.x + rel.x * k, y: pose.anchor.y + at.y + rel.y * k };
      const dx = pose.anchor.x - start.left, dy = pose.anchor.y - start.top;
      const tx = (goal.x - ca.x) * g - dx;
      const ty = (goal.y - ca.y) * g - dy;
      const s = Math.exp(Math.log(Math.max(0.01, (rel.w * k) / a.width)) * g);
      return { transform: `translate(${r1(tx)}px, ${r1(ty)}px) scale(${r4(s)})`, transformOrigin: origin, offset: WINDOW_OFFSETS[index] };
    });
  };
};

export const fly = (o: {
  page: HTMLElement;
  backdrop: HTMLElement;
  radius: number;
  from: WindowPose;
  to: WindowPose;
  duration: number;
  easing: string;
  replica: { el: HTMLElement; rect: Rect } | null;
  replicaAt?: { x: number; y: number };
  early?: (part: HTMLElement) => boolean;
  replicaFade: Keyframe[];
  head: Keyframe[];
  body: Keyframe[];
  plate: Keyframe[];
  fade?: Keyframe[];
  scale?: boolean;
  scaleReplica?: boolean;
  landAt?: number;
  focus?: { el: Element; to: Rect };
}): Flight => {
  const { page } = o;
  const start = rectOf(page);
  let box: Rect = { left: start.left, top: start.top, width: page.offsetWidth, height: page.offsetHeight };
  let to = o.to;
  const land = Math.min(1, Math.max(0.5, o.landAt ?? 1));
  const timing: KeyframeAnimationOptions = { duration: o.duration * land, endDelay: o.duration * (1 - land), easing: o.easing, fill: 'both' };
  const linear: KeyframeAnimationOptions = { duration: o.duration, easing: 'linear', fill: 'both' };
  const frames = () => windowPoses(o.from, to).map((pose, index) => ({ ...cardFrame(pose, box, o.radius), offset: WINDOW_OFFSETS[index] }));
  const overflowX = page.style.overflowX;
  page.style.overflowX = 'visible';
  const { head, body } = partsOf(page);
  const anims: Animation[] = [];
  const track = (animation: Animation) => { anims.push(exemptFromSettle(animation)); return animation; };
  const main = track(page.animate(frames(), timing));
  if (o.fade) track(page.animate(o.fade, linear));
  if (o.replica) {
    const { el } = o.replica;
    el.dataset.morphLayer = '';
    Object.assign(el.style, { left: `${r1(o.replicaAt?.x ?? 0)}px`, top: `${r1(o.replicaAt?.y ?? 0)}px`, zIndex: '50' });
    page.appendChild(el);
    settleCssAnimations(el);
    track(el.animate(o.replicaFade, linear));
  }
  const plate = o.backdrop.firstElementChild;
  if (plate instanceof HTMLElement) track(plate.animate(o.plate, linear));
  if (head) track(head.animate(o.head, linear));
  const parts = new Map<HTMLElement, Animation>();
  const leavingParts = new WeakSet<HTMLElement>();
  const fadePart = (part: HTMLElement) => {
    const animation = track(part.animate(o.early?.(part) ? o.head : o.body, linear));
    animation.currentTime = main.currentTime;
    if (main.playState === 'paused') animation.pause();
    parts.set(part, animation);
  };
  for (const part of body) fadePart(part);

  const scaled: Array<{ animation: Animation; frames: () => Keyframe[] }> = [];
  if (o.scale) {
    const widths = () => windowPoses(o.from, to).map((pose) => pose.rect.width);
    const pin = { x: o.to.rect.left - o.to.anchor.x, y: o.to.rect.top - o.to.anchor.y };
    const base = Math.max(1, o.from.rect.width);
    const scaleFrames = (of: number, origin: string) => () =>
      widths().map((width, index) => ({ transform: `scale(${r4(width / of)})`, transformOrigin: origin, offset: WINDOW_OFFSETS[index] }));
    const focusFrames = focusFramesFor(o, start, () => to);
    for (const part of [head, ...body]) {
      if (!part) continue;
      if (focusFrames && part.contains(o.focus!.el)) {
        const frames = focusFrames(rectOf(part));
        scaled.push({ animation: track(part.animate(frames(), timing)), frames });
        continue;
      }
      const box = rectOf(part);
      const origin = `${r1(start.left + pin.x - box.left)}px ${r1(start.top + pin.y - box.top)}px`;
      const frames = scaleFrames(base, origin);
      scaled.push({ animation: track(part.animate(frames(), timing)), frames });
    }
    if (o.replica && o.scaleReplica !== false) {
      const frames = scaleFrames(Math.max(1, o.replica.rect.width), '0 0');
      scaled.push({ animation: track(o.replica.el.animate(frames(), timing)), frames });
    }
  }

  const resized = typeof ResizeObserver === 'function'
    ? new ResizeObserver(() => {
      if (disposed || main.playState === 'finished') return;
      box = { ...box, width: page.offsetWidth, height: page.offsetHeight };
      (main.effect as KeyframeEffect | null)?.setKeyframes(frames());
    })
    : null;
  resized?.observe(page);
  const added = typeof MutationObserver === 'function'
    ? new MutationObserver((records) => {
      if (disposed || main.playState === 'finished') return;
      for (const record of records) {
        for (const node of record.addedNodes) {
          if (node instanceof HTMLElement && node !== head && !parts.has(node) && !node.hasAttribute('data-morph-layer')) fadePart(node);
        }
      }
      for (const [part, animation] of parts) {
        if (leavingParts.has(part) || !/-leave-active(\s|$)/.test(part.className)) continue;
        leavingParts.add(part);
        const now = Number.parseFloat(getComputedStyle(part).opacity) || 0;
        animation.cancel();
        track(part.animate([{ opacity: now }, { opacity: 0 }], { duration: 320, easing: 'ease', fill: 'forwards' }));
      }
    })
    : null;
  added?.observe(page, { childList: true });

  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    resized?.disconnect();
    added?.disconnect();
    for (const animation of anims) animation.cancel();
    o.backdrop.remove();
    o.replica?.el.remove();
    page.style.overflowX = overflowX;
  };
  return {
    main,
    anims,
    snapshot: () => {
      const box = page.getBoundingClientRect();
      const content = opacityOf(head);
      const replica = opacityOf(o.replica?.el);
      const plateNow = opacityOf(plate);
      const pageNow = opacityOf(page);
      return {
        window: visibleWindow(page),
        anchor: { x: box.left, y: box.top },
        content: Number.isFinite(content) ? content : 1,
        replica: Number.isFinite(replica) ? replica : 0,
        plate: Number.isFinite(plateNow) ? plateNow : 1,
        page: Number.isFinite(pageNow) ? pageNow : 1,
      };
    },
    pose: () => {
      const style = getComputedStyle(page);
      const matrix = style.transform && style.transform !== 'none' ? new DOMMatrixReadOnly(style.transform) : null;
      const anchor = { x: box.left + (matrix?.e ?? 0), y: box.top + (matrix?.f ?? 0) };
      const clip = style.clipPath.match(/^inset\((.*?)(?: round|\))/);
      const edges = clip?.[1].trim().split(/\s+/).map(Number.parseFloat) ?? [];
      const [top = 0, right = top, bottom = top, left = right] = edges.every(Number.isFinite) ? edges : [];
      return {
        anchor,
        rect: { left: anchor.x + left, top: anchor.y + top, width: Math.max(1, box.width - left - right), height: Math.max(1, box.height - top - bottom) },
      };
    },
    turn: () => {
      for (const animation of anims) {
        const rate = -(animation.playbackRate || 1);
        if (typeof animation.updatePlaybackRate === 'function') animation.updatePlaybackRate(rate);
        else animation.playbackRate = rate;
      }
    },
    retarget: (next) => {
      to = next;
      (main.effect as KeyframeEffect | null)?.setKeyframes(frames());
      for (const { animation, frames: of } of scaled) (animation.effect as KeyframeEffect | null)?.setKeyframes(of());
    },
    dispose,
  };
};
