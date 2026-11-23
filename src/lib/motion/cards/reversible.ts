
export const fromNow = (el: Element, props: Array<'transform' | 'opacity' | 'translate' | 'scale' | 'rotate'> = ['transform', 'opacity']): Keyframe => {
  const style = getComputedStyle(el);
  const frame: Keyframe = {};
  for (const prop of props) {
    const value = style.getPropertyValue(prop);
    frame[prop] = value || (prop === 'opacity' ? '1' : 'none');
  }
  return frame;
};

export const cancelOn = (el: Element): void => {
  for (const animation of el.getAnimations()) animation.cancel();
};

export const animateFromNow = (
  el: Element,
  to: Keyframe | Keyframe[],
  options: KeyframeAnimationOptions,
  props: Array<'transform' | 'opacity' | 'translate' | 'scale' | 'rotate'> = ['transform', 'opacity'],
): Animation => {
  const from = fromNow(el, props);
  cancelOn(el);
  const rest = (Array.isArray(to) ? to : [to]).map((frame) => ({ ...frame }));
  const last = rest[rest.length - 1]!;
  for (const prop of props) if (!(prop in last)) last[prop] = from[prop];
  const fill = options.fill === 'forwards' && options.delay ? 'both' : options.fill;
  return el.animate([from, ...rest], { ...options, fill });
};

export interface Sequence {
  next: () => number;
  live: (id: number) => boolean;
  readonly current: number;
}

export const sequence = (): Sequence => {
  let id = 0;
  return {
    next: () => (id += 1),
    live: (n) => n === id,
    get current() { return id; },
  };
};

export const HANDOFF_MS = 120;

export interface Relay extends Sequence {
  track: <T extends Animation | Animation[]>(animations: T) => T;
  handoff: (ms?: number) => number;
  settle: (ms?: number) => Promise<void>;
  readonly moving: boolean;
}

const hurry = (animation: Animation, ms: number) => {
  const timing = animation.effect?.getComputedTiming();
  const end = Number(timing?.endTime);
  const local = Number(timing?.localTime ?? 0);
  if (!Number.isFinite(end) || !Number.isFinite(local)) return;
  const left = (end - local) / (Math.abs(animation.playbackRate) || 1);
  if (left <= ms) return;
  const rate = animation.playbackRate * (left / Math.max(ms, 1));
  if (typeof animation.updatePlaybackRate === 'function') animation.updatePlaybackRate(rate);
  else animation.playbackRate = rate;
};

export const relay = (): Relay => {
  const seq = sequence();
  const flying = new Set<Animation>();
  const running = (a: Animation) => a.playState === 'running' || a.pending;
  return {
    next: seq.next,
    live: seq.live,
    get current() { return seq.current; },
    get moving() { return [...flying].some(running); },
    track: (animations) => {
      for (const a of Array.isArray(animations) ? animations : [animations]) {
        flying.add(a);
        void a.finished.catch(() => undefined).then(() => flying.delete(a));
      }
      return animations;
    },
    handoff: (ms = HANDOFF_MS) => {
      for (const a of flying) if (running(a)) hurry(a, ms);
      flying.clear();
      return seq.next();
    },
    settle: async (ms = HANDOFF_MS) => {
      const live = [...flying].filter(running);
      for (const a of live) hurry(a, ms);
      await Promise.all(live.map((a) => a.finished.catch(() => undefined)));
    },
  };
};
