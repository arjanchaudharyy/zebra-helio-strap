import { exemptFromSettle, hurryAnimation } from './interrupt';
import { HANDOFF_SPAN } from './timing';

export type Direction = 1 | -1;

export interface FlightOptions {
  duration: number;
  onLand?: (direction: Direction) => void;
  keepOnLand?: boolean;
}

export const NEARLY_HIDDEN = 0.001;

const hiddenBy = new WeakMap<Element, Animation>();

export function whileShown<T>(el: Element, fn: () => T): T {
  const hide = hiddenBy.get(el);
  if (!hide || hide.playState === 'idle') return fn();
  hide.cancel();
  try {
    return fn();
  } finally {
    hide.play();
  }
}

export function handoffFrames(from: number, to: number, at: number, span = HANDOFF_SPAN): Keyframe[] {
  const start = Math.min(Math.max(0, at), 1);
  const end = Math.min(1, start + span);
  const frames: Keyframe[] = [{ opacity: from, offset: 0 }];
  if (start > 0) frames.push({ opacity: from, offset: start });
  if (end > start) frames.push({ opacity: to, offset: end });
  else frames[frames.length - 1] = { opacity: to, offset: start };
  if (end < 1) frames.push({ opacity: to, offset: 1 });
  return frames;
}

export interface Flight {
  readonly duration: number;
  readonly direction: Direction;
  readonly anims: readonly Animation[];
  readonly main: Animation | null;
  add: (el: Element, keyframes: Keyframe[], options?: { easing?: string; pseudoElement?: string; hold?: number }) => Animation;
  handoff: (parts: { show?: Element[]; hide?: Element[] }, notBefore?: number) => void;
  cover: (el: Element | null | undefined) => void;
  own: (cleanup: () => void) => void;
  turn: () => boolean;
  startFromEnd: () => void;
  hurry: (ms: number) => void;
  running: () => boolean;
  progress: () => number;
  dispose: () => void;
}

export function createFlight({ duration, onLand, keepOnLand = false }: FlightOptions): Flight {
  const anims: Animation[] = [];
  const covered: Array<{ el: Element; hide: Animation }> = [];
  const cleanups: Array<() => void> = [];
  let direction: Direction = 1;
  let disposed = false;
  let landed = false;
  const late: Animation[] = [];

  const time = (): number => {
    const value = Number(anims[0]?.currentTime ?? 0);
    return Number.isFinite(value) ? Math.min(duration, Math.max(0, value)) : 0;
  };
  const running = () => !disposed && !landed && anims.length > 0 && anims.some((a) => a.playState === 'running' || a.pending);

  const land = () => {
    if (landed || disposed) return;
    landed = true;
    const end = direction;
    if (!keepOnLand) dispose();
    onLand?.(end);
  };

  const add: Flight['add'] = (el, keyframes, options = {}) => {
    const hold = Math.min(0.9, Math.max(0, options.hold ?? 0)) * duration;
    const animation = exemptFromSettle(el.animate(keyframes, {
      duration: duration - hold, delay: hold, easing: options.easing ?? 'linear', fill: 'both', ...(options.pseudoElement ? { pseudoElement: options.pseudoElement } : {}),
    }));
    if (anims.length) {
      animation.currentTime = time();
      if (direction < 0) animation.playbackRate = -Math.abs(anims[0]!.playbackRate || 1);
      else animation.playbackRate = anims[0]!.playbackRate || 1;
    } else {
      animation.addEventListener('finish', () => land());
    }
    anims.push(animation);
    return animation;
  };

  const handoff: Flight['handoff'] = ({ show = [], hide = [] }, notBefore = 0) => {
    if (disposed) return;
    const at = Math.max(notBefore, time() / Math.max(duration, 1));
    if (!landed && at + HANDOFF_SPAN * 0.5 < 1) {
      for (const el of show) add(el, handoffFrames(NEARLY_HIDDEN, 1, at));
      for (const el of hide) add(el, handoffFrames(1, 0, at));
      return;
    }
    const span = Math.max(90, duration * HANDOFF_SPAN);
    for (const el of show) late.push(exemptFromSettle(el.animate([{ opacity: NEARLY_HIDDEN }, { opacity: 1 }], { duration: span, easing: 'linear', fill: 'both' })));
    for (const el of hide) late.push(exemptFromSettle(el.animate([{ opacity: 1 }, { opacity: 0 }], { duration: span, easing: 'linear', fill: 'both' })));
  };

  const cover: Flight['cover'] = (el) => {
    if (!el || disposed) return;
    const hide = exemptFromSettle(el.animate([{ opacity: 0 }, { opacity: 0 }], { duration: 1, fill: 'forwards' }));
    hiddenBy.set(el, hide);
    covered.push({ el, hide });
  };

  const turn = () => {
    if (!running()) return false;
    direction = direction > 0 ? -1 : 1;
    for (const animation of anims) {
      const rate = -(animation.playbackRate || 1);
      if (typeof animation.updatePlaybackRate === 'function') animation.updatePlaybackRate(rate);
      else animation.playbackRate = rate;
    }
    return true;
  };

  const startFromEnd = () => {
    if (disposed || landed) return;
    direction = -1;
    for (const animation of anims) {
      animation.currentTime = duration;
      animation.playbackRate = -Math.abs(animation.playbackRate || 1);
    }
  };

  const hurry = (ms: number) => {
    if (!running()) return;
    for (const animation of anims) hurryAnimation(animation, ms);
  };

  const dispose = () => {
    if (disposed) return;
    disposed = true;
    for (const animation of anims) animation.cancel();
    for (const animation of late) animation.cancel();
    for (const { el, hide } of covered) {
      hide.cancel();
      if (hiddenBy.get(el) === hide) hiddenBy.delete(el);
    }
    covered.length = 0;
    for (const cleanup of cleanups.splice(0)) {
      try { cleanup(); } catch { }
    }
  };

  return {
    duration,
    get direction() { return direction; },
    anims,
    get main() { return anims[0] ?? null; },
    add,
    handoff,
    cover,
    own: (cleanup) => { if (disposed) cleanup(); else cleanups.push(cleanup); },
    turn,
    startFromEnd,
    hurry,
    running,
    progress: () => time() / Math.max(duration, 1),
    dispose,
  };
}
