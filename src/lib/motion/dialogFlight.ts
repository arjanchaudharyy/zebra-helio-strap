import { createFlight, NEARLY_HIDDEN, whileShown, type Flight } from './flight';
import { onMotionEscape } from './interrupt';
import { easeAt, OPEN_EASE as WINDOW_EASE, OPEN_MS, timeAt, turnedEase } from './timing';
import { lessMotion } from './lessMotion';
import { settleCssAnimations } from './settle';
import { belowLine, cardReplica, visibleWindow, windowInset, windowPoses, WINDOW_OFFSETS, type WindowRect } from './window';

export { visibleWindow };

export interface FlightOrigin {
  el: HTMLElement | null;
  rect: DOMRect | null;
}

const OBJECT_MS = OPEN_MS;
const WINDOW_MS = OPEN_MS;
const CLOSE_CONTENT_OUT: readonly [number, number] = [0.64, 0.9];
const CLOSE_REPLICA_IN: readonly [number, number] = [0.46, 0.68];
const LAND_HOLD = 0.14;
const CLOSE_FADE = 0.12;
export const SOURCE_IN: readonly [number, number] = [CLOSE_FADE + 0.06, 0.42];
const onShape = (offset: number) => LAND_HOLD + (1 - LAND_HOLD) * offset;
export const CLOSE_VEIL: Keyframe[] = [{ opacity: NEARLY_HIDDEN }, { opacity: NEARLY_HIDDEN, offset: 0.42 }, { opacity: 1, offset: 0.97 }, { opacity: 1 }];
const CLOSE_EASE = turnedEase(WINDOW_EASE);
export const closeTimeFor = (openProgress: number) => 1 - timeAt(WINDOW_EASE, 1 - easeAt(WINDOW_EASE, openProgress));
export const openTimeFor = (closeBuildProgress: number) => timeAt(WINDOW_EASE, easeAt(CLOSE_EASE, closeBuildProgress));
const shapeOfClose = (progress: number) => Math.max(0, (progress - LAND_HOLD) / (1 - LAND_HOLD));
const RELAY_CLOSE_MS = 260;

const reducedMotion = () => lessMotion();

let lastPress: { x: number; y: number } | null = null;
if (typeof document !== 'undefined') {
  document.addEventListener('pointerdown', (event) => { lastPress = { x: event.clientX, y: event.clientY }; }, true);
}

const usable = (el: HTMLElement | null): el is HTMLElement =>
  !!el && el.isConnected && el !== document.body && el.getClientRects().length > 0;

export const originOf = (el: HTMLElement | null): FlightOrigin => {
  if (usable(el)) return { el, rect: el.getBoundingClientRect() };
  if (lastPress) return { el: null, rect: new DOMRect(lastPress.x - 20, lastPress.y - 20, 40, 40) };
  return { el: null, rect: null };
};

const barLine = (el: HTMLElement | null | undefined): number => {
  if (!el || el.closest('.shell-head') || !el.closest('#main-content')) return -Infinity;
  return document.querySelector('.shell-head')?.getBoundingClientRect().bottom ?? -Infinity;
};
const currentRect = (origin: FlightOrigin | null): WindowRect | null => {
  const el = origin?.el ?? null;
  const rect = usable(el) ? whileShown(el, () => el.getBoundingClientRect()) : origin?.rect ?? null;
  if (!rect) return null;
  const line = barLine(el);
  return Number.isFinite(line) ? belowLine(rect, line) : rect;
};

type Box = { left: number; top: number; width: number; height: number };

export const objectTransform = (from: Box, panel: Box, subject: Box = panel): string => {
  const scale = Math.min(1, Math.max(0.04, from.width / Math.max(1, subject.width)));
  const c = { x: panel.left + panel.width / 2, y: panel.top + panel.height / 2 };
  const q = { x: subject.left + subject.width / 2, y: subject.top + subject.height / 2 };
  const o = { x: from.left + from.width / 2, y: from.top + from.height / 2 };
  const dx = o.x - c.x - scale * (q.x - c.x);
  const dy = o.y - c.y - scale * (q.y - c.y);
  return `translate(${dx.toFixed(1)}px, ${dy.toFixed(1)}px) scale(${scale.toFixed(3)})`;
};

export interface FlightGrow { width: boolean; height: boolean }

const posesOf = (from: WindowRect, to: WindowRect) =>
  windowPoses({ rect: from, anchor: { x: from.left, y: from.top } }, { rect: to, anchor: { x: to.left, y: to.top } });

const paintedSize = (rect: WindowRect, panel: WindowRect, grow?: FlightGrow) => ({
  width: grow?.width ? Math.max(rect.width, panel.width) : panel.width,
  height: grow?.height ? Math.max(rect.height, panel.height) : panel.height,
});

export function edgeBleed(c0: number, b0: number, end: number, full: number): number {
  const seen = Math.min(c0, b0);
  if (seen <= end + 0.01) return b0 <= end + 0.01 ? full : 0;
  return c0 <= b0 ? Math.min(full, Math.max(0, c0 - end)) : full;
}

/** Shared by dialogs and the stage-local star card. Content stays at 1:1 throughout. */
export function dialogWindowFrames(from: WindowRect, to: WindowRect, panel: WindowRect, radius: number, bleedEnd = false, grow?: FlightGrow): Keyframe[] {
  const poses = posesOf(from, to);
  const last = poses.length - 1;
  return poses.map((pose, i) => {
    const dx = pose.anchor.x - panel.left, dy = pose.anchor.y - panel.top;
    let clipRect: WindowRect = pose.rect;
    if (bleedEnd && i === last) {
      const full = Math.max(radius, 36);
      const prev = poses[last - 1]!;
      const paint = paintedSize(prev.rect, panel, grow);
      const right = edgeBleed(prev.rect.left + prev.rect.width, prev.anchor.x + paint.width, pose.rect.left + pose.rect.width, full);
      const bottom = edgeBleed(prev.rect.top + prev.rect.height, prev.anchor.y + paint.height, pose.rect.top + pose.rect.height, full);
      clipRect = { left: pose.rect.left - full, top: pose.rect.top - full, width: pose.rect.width + full + right, height: pose.rect.height + full + bottom };
    }
    return { offset: WINDOW_OFFSETS[i], transform: `translate(${dx}px, ${dy}px)`,
      // DOMRect dimensions are prototype getters, so spreading the object loses them.
      clipPath: windowInset(clipRect, { left: panel.left + dx, top: panel.top + dy, width: panel.width, height: panel.height }, radius, true) };
  });
}

export function dialogFillFrames(from: WindowRect, to: WindowRect, panel: WindowRect, fill: { width: number; height: number }, radius: number): Keyframe[] {
  const open = -64;
  return posesOf(from, to).map((pose, i) => {
    const size = paintedSize(pose.rect, panel, { width: true, height: true });
    const right = fill.width > panel.width + 0.5 ? fill.width - size.width : open;
    const bottom = fill.height > panel.height + 0.5 ? fill.height - size.height : open;
    return { offset: WINDOW_OFFSETS[i], clipPath: `inset(${open}px ${Number(right.toFixed(2))}px ${Number(bottom.toFixed(2))}px ${open}px round ${radius}px)` };
  });
}


const sources = new WeakMap<HTMLElement, HTMLElement>();
function hideSource(backdrop: HTMLElement, el: HTMLElement | null | undefined) {
  if (!el?.isConnected) return;
  el.setAttribute('data-dialog-source', '');
  sources.set(backdrop, el);
}
function showSource(backdrop: HTMLElement) {
  const el = sources.get(backdrop);
  sources.delete(backdrop);
  el?.removeAttribute('data-dialog-source');
}


interface Live {
  flight: Flight;
  backdrop: HTMLElement;
  panel: HTMLElement;
  origin: FlightOrigin | null;
  closing: boolean;
}

const lives = new WeakMap<HTMLElement, Live>();
const ghosts = new Set<Live>();
const escapes = new WeakMap<HTMLElement, () => void>();
const forgetEscape = (backdrop: HTMLElement) => { escapes.get(backdrop)?.(); escapes.delete(backdrop); };
const landings = new WeakMap<HTMLElement, { promise: Promise<void>; resolve: () => void }>();
export const whenDialogLanded = (panel: HTMLElement | null | undefined): Promise<void> =>
  (panel ? landings.get(panel)?.promise : undefined) ?? Promise.resolve();

const removeGhost = (live: Live) => {
  showSource(live.backdrop);
  live.backdrop.remove();
  ghosts.delete(live);
};

const detachAsPicture = (backdrop: HTMLElement, panel: HTMLElement) => {
  panel.removeAttribute('data-modal-dialog');
  panel.removeAttribute('role');
  backdrop.setAttribute('aria-hidden', 'true');
  backdrop.inert = true;
  backdrop.style.pointerEvents = 'none';
  document.body.appendChild(backdrop);
};

const subjectOf = (panel: HTMLElement): Box => {
  const el = panel.querySelector<HTMLElement>('[data-flight-subject]');
  const box = (el ?? panel).getBoundingClientRect();
  return box.width && box.height ? box : panel.getBoundingClientRect();
};

const fillBehind = (source: HTMLElement): string | null => {
  const opaque = (style: CSSStyleDeclaration) => style.backgroundImage !== 'none' || !/^(transparent|rgba\(.*,\s*0\))$/.test(style.backgroundColor);
  if (opaque(getComputedStyle(source))) return null;
  for (let el = source.parentElement; el && el !== document.body; el = el.parentElement) {
    const style = getComputedStyle(el);
    if (opaque(style)) return `${style.backgroundImage === 'none' ? '' : `${style.backgroundImage}, `}${style.backgroundColor}`;
  }
  return null;
};

const radiusFor = (origin: FlightOrigin | null, panel: HTMLElement) => {
  const from = currentRect(origin);
  return Math.min(parseFloat(getComputedStyle(origin?.el ?? panel).borderRadius) || 16, (from?.height ?? 32) / 2, (from?.width ?? 32) / 2, 24);
};

function build(backdrop: HTMLElement, panel: HTMLElement, origin: FlightOrigin | null, closing: boolean, veilFrom = NEARLY_HIDDEN): Live | null {
  const bare = panel.classList.contains('is-bare');
  const windowEl = panel.querySelector<HTMLElement>('[data-dialog-window]');
  const objectMode = bare && !windowEl;
  const final = panel.getBoundingClientRect();
  const target = windowEl ? windowEl.getBoundingClientRect() : final;
  const from = currentRect(origin);
  const live: Live = { flight: null as unknown as Flight, backdrop, panel, origin, closing };
  const flight = createFlight({
    duration: objectMode ? OBJECT_MS : WINDOW_MS,
    onLand: (direction) => {
      forgetEscape(backdrop);
      if (lives.get(backdrop) === live) lives.delete(backdrop);
      if (direction > 0 && !live.closing) {
        landings.get(panel)?.resolve();
        return;
      }
      landings.get(panel)?.resolve();
      removeGhost(live);
    },
  });
  live.flight = flight;
  const shapeEase = closing ? CLOSE_EASE : WINDOW_EASE;
  if (closing) flight.add(backdrop, CLOSE_VEIL, { pseudoElement: '::before' });
  else flight.add(backdrop, [{ opacity: Math.max(NEARLY_HIDDEN, veilFrom) }, { opacity: 1, offset: 0.7 }, { opacity: 1 }], { easing: 'ease-out', pseudoElement: '::before' });
  if (closing && from) flight.add(panel, [{ opacity: NEARLY_HIDDEN }, { opacity: 1, offset: CLOSE_FADE }, { opacity: 1 }]);
  if (objectMode) {
    const start = from ? objectTransform(from, final, subjectOf(panel)) : 'scale(.96)';
    flight.add(panel, [{ transform: start }, { transform: 'none' }], { easing: shapeEase, hold: closing ? LAND_HOLD : 0 });
    if (!from) flight.add(panel, [{ opacity: NEARLY_HIDDEN }, { opacity: 1, offset: 0.4 }, { opacity: 1 }]);
  } else {
    const start = from ?? { left: target.left + target.width / 2 - 20, top: target.top + target.height / 2 - 20, width: 40, height: 40 };
    if (!bare) {
      panel.classList.add('is-flying');
      flight.own(() => panel.classList.remove('is-flying'));
    }
    const source = origin?.el?.isConnected ? origin.el : null;
    const grow: FlightGrow | undefined = source && from && (from.width > final.width + 0.5 || from.height > final.height + 0.5)
      ? { width: from.width > final.width + 0.5, height: from.height > final.height + 0.5 } : undefined;
    const shape = { easing: shapeEase, hold: closing ? LAND_HOLD : 0 };
    flight.add(panel, dialogWindowFrames(start, target, final, radiusFor(origin, windowEl ?? panel), true, grow), shape);
    const content = windowEl
      ? (windowEl.querySelector<HTMLElement>('[data-dialog-content]') ?? windowEl)
      : panel.querySelector<HTMLElement>(':scope > .dialog-scroll');
    const replica = source ? whileShown(source, () => cardReplica(source).el) : null;
    if (replica) {
      replica.querySelectorAll('[data-dialog-source]').forEach((node) => node.removeAttribute('data-dialog-source'));
      replica.dataset.dialogReplica = '';
      const cut = from ? Math.max(0, from.top - source!.getBoundingClientRect().top) : 0;
      Object.assign(replica.style, { position: 'absolute', left: '0', top: `${-Math.round(cut * 10) / 10}px`, zIndex: '4', pointerEvents: 'none' });
      const fill = source ? fillBehind(source) : null;
      if (fill) replica.style.background = fill;
      panel.appendChild(replica);
      settleCssAnimations(replica);
      flight.own(() => replica.remove());
      flight.add(replica, [{ opacity: 1 }, { opacity: 1 }]);
      if (grow && from) {
        const fill = { width: Math.max(final.width, from.width), height: Math.max(final.height, from.height) };
        panel.style.setProperty('--flight-w', `${fill.width}px`);
        panel.style.setProperty('--flight-h', `${fill.height}px`);
        flight.own(() => { panel.style.removeProperty('--flight-w'); panel.style.removeProperty('--flight-h'); });
        const frames = dialogFillFrames(start, target, final, fill, parseFloat(getComputedStyle(panel).borderTopLeftRadius) || 0);
        const glass = panel.querySelector<HTMLElement>(':scope > .dialog-glass');
        if (glass) flight.add(glass, frames, shape);
        flight.add(panel, frames, { ...shape, pseudoElement: '::after' });
      }
    }
    if (content) flight.add(content, [{ opacity: NEARLY_HIDDEN }, { opacity: NEARLY_HIDDEN }]);
    hideSource(backdrop, source);
    if (closing && source) {
      source.setAttribute('data-dialog-source', 'fading');
      flight.add(source, [{ opacity: 1 }, { opacity: 1, offset: SOURCE_IN[0] }, { opacity: 0, offset: SOURCE_IN[1] }, { opacity: 0 }]);
    }
    const earliest = 0.04;
    if (closing) {
      if (content) flight.add(content, [{ opacity: NEARLY_HIDDEN }, { opacity: NEARLY_HIDDEN, offset: onShape(CLOSE_CONTENT_OUT[0]) }, { opacity: 1, offset: onShape(CLOSE_CONTENT_OUT[1]) }, { opacity: 1 }]);
      if (replica) flight.add(replica, [{ opacity: 1 }, { opacity: 1, offset: onShape(CLOSE_REPLICA_IN[0]) }, { opacity: 0, offset: onShape(CLOSE_REPLICA_IN[1]) }, { opacity: 0 }]);
    } else {
      requestAnimationFrame(() => requestAnimationFrame(() => {
        if (flight.running()) flight.handoff({ show: content ? [content] : [], hide: replica ? [replica] : [] }, earliest);
      }));
    }
  }
  if (closing) flight.startFromEnd();
  lives.set(backdrop, live);
  return live;
}

const quickOpen = (backdrop: HTMLElement, panel: HTMLElement) => {
  for (const old of [...ghosts]) { old.flight.dispose(); removeGhost(old); }
  backdrop.animate([{ opacity: NEARLY_HIDDEN }, { opacity: 1 }], { duration: 180, easing: 'ease-out', fill: 'backwards', pseudoElement: '::before' });
  const quick = panel.animate([{ opacity: 0, transform: 'scale(.95)' }, { opacity: 1, transform: 'none' }], { duration: 180, easing: 'ease-out', fill: 'backwards' });
  const done = () => landings.get(panel)?.resolve();
  quick.finished.then(done, done);
};

const quickClose = (backdrop: HTMLElement, panel: HTMLElement) => {
  detachAsPicture(backdrop, panel);
  for (const animation of backdrop.getAnimations({ subtree: true })) animation.cancel();
  const timing = { duration: 180, easing: 'ease-in', fill: 'forwards' as const };
  panel.animate([{ opacity: 1, transform: 'none' }, { opacity: 0, transform: 'scale(.95)' }], timing);
  const fade = backdrop.animate([{ opacity: 1 }, { opacity: 0 }], { ...timing, pseudoElement: '::before' });
  const cleanup = () => { showSource(backdrop); backdrop.remove(); };
  fade.finished.then(cleanup, cleanup);
};

const open = (backdrop: HTMLElement, panel: HTMLElement, origin: FlightOrigin | null, onEscape?: () => void) => {
  let resolve: () => void = () => undefined;
  landings.set(panel, { promise: new Promise<void>((r) => { resolve = r; }), resolve: () => resolve() });
  if (reducedMotion()) { quickOpen(backdrop, panel); return; }
  let resumeAt: number | null = null;
  let veilFrom = NEARLY_HIDDEN;
  for (const previous of [...ghosts]) {
    if (!previous.flight.running()) {
      previous.flight.dispose();
      removeGhost(previous);
      continue;
    }
    const same = previous.origin?.el && previous.origin.el === origin?.el
      && previous.panel.classList.contains('is-bare') === panel.classList.contains('is-bare');
    if (same && resumeAt === null) {
      resumeAt = shapeOfClose(previous.flight.progress());
      previous.flight.dispose();
      removeGhost(previous);
    } else {
      const seenVeil = Number(getComputedStyle(previous.backdrop, '::before').opacity) || NEARLY_HIDDEN;
      veilFrom = Math.max(veilFrom, seenVeil);
      previous.backdrop.classList.add('is-ghost-panel-only');
      previous.flight.hurry(RELAY_CLOSE_MS);
    }
  }
  const live = build(backdrop, panel, origin, false, veilFrom);
  if (!live) return;
  if (resumeAt !== null) {
    const at = openTimeFor(resumeAt);
    for (const animation of live.flight.anims) animation.currentTime = at * live.flight.duration;
  }
  if (onEscape) escapes.set(backdrop, onMotionEscape(() => { if (!live.flight.running()) return false; forgetEscape(backdrop); onEscape(); return true; }));
};

const close = (backdrop: HTMLElement, panel: HTMLElement, origin: FlightOrigin | null) => {
  forgetEscape(backdrop);
  if (backdrop.isConnected || backdrop.dataset.dissolved !== undefined) { showSource(backdrop); landings.get(panel)?.resolve(); return; }
  if (reducedMotion()) { quickClose(backdrop, panel); return; }
  detachAsPicture(backdrop, panel);
  settleCssAnimations(backdrop);
  for (const old of [...ghosts]) {
    if (old !== lives.get(backdrop) && !old.flight.running()) {
      old.flight.dispose();
      removeGhost(old);
    }
  }
  const current = lives.get(backdrop);
  const interruptedAt = current && current.flight.running() ? current.flight.progress() : null;
  current?.flight.dispose();
  const live = build(backdrop, panel, origin, true);
  if (live) {
    if (interruptedAt !== null && interruptedAt < 1) {
      const pos = Math.max(16, onShape(closeTimeFor(interruptedAt)) * live.flight.duration);
      const rate = pos < 150 ? -(pos / 150) : -1;
      for (const animation of live.flight.anims) {
        animation.currentTime = pos;
        animation.playbackRate = rate;
      }
    }
    ghosts.add(live);
  } else {
    removeGhost({ backdrop, panel, origin, closing: true, flight: null as unknown as Flight });
  }
};

export const dialogFlight = { open, close };
