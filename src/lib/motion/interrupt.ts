const FAST_MS = 90;

const waiting = new Set<() => void>();
const escapers: Array<() => boolean> = [];
const firstEscapers: Array<() => boolean> = [];
const exempt = new WeakSet<Animation>();

export const exemptFromSettle = (animation: Animation): Animation => {
  exempt.add(animation);
  return animation;
};

export const hurryAnimation = (animation: Animation, ms: number): void => {
  const left = remainingMs(animation);
  if (left <= ms) return;
  const next = animation.playbackRate * (left / Math.max(ms, 1));
  if (typeof CSSTransition !== 'undefined' && animation instanceof CSSTransition) {
    // A CSS transition can be replaced by the same navigation that hurries it.
    // Edge 156 crashes when the asynchronous rate update is still pending
    // (reproduced with DevTools slow playback, including header pseudo-elements).
    // Apply the rate synchronously and restore the position to avoid a jump.
    const position = animation.currentTime;
    animation.playbackRate = next;
    if (position !== null) animation.currentTime = position;
  } else if (typeof animation.updatePlaybackRate === 'function') animation.updatePlaybackRate(next);
  else animation.playbackRate = next;
};

export const onMotionEscape = (handler: () => boolean, options: { first?: boolean } = {}): (() => void) => {
  const list = options.first ? firstEscapers : escapers;
  list.push(handler);
  return () => {
    const index = list.lastIndexOf(handler);
    if (index >= 0) list.splice(index, 1);
  };
};

export const escapeMotion = (): boolean => {
  for (const handler of [...firstEscapers.slice().reverse(), ...escapers.slice().reverse()]) {
    if (handler()) return true;
  }
  return false;
};

export const onMotionSkip = (skip: () => void): (() => void) => {
  waiting.add(skip);
  return () => { waiting.delete(skip); };
};

export const remainingMs = (animation: Animation): number => {
  if (animation.playState !== 'running' && !animation.pending) return 0;
  const timing = animation.effect?.getComputedTiming();
  const end = Number(timing?.endTime);
  const local = Number(timing?.localTime ?? 0);
  if (!Number.isFinite(end) || !Number.isFinite(local)) return 0;
  const rate = Math.abs(animation.playbackRate) || 1;
  return animation.playbackRate < 0 ? local / rate : (end - local) / rate;
};

export const worthInterrupting = (animation: Animation): boolean => {
  const timing = animation.effect?.getComputedTiming();
  if (!timing || Number(timing.iterations) > 1) return false;
  const isTransition = typeof CSSTransition !== 'undefined' && animation instanceof CSSTransition;
  return !(isTransition && Number(timing.activeDuration) < 300);
};

const running = (): Animation[] =>
  typeof document !== 'undefined' && typeof document.getAnimations === 'function'
    ? document.getAnimations().filter((a) => !exempt.has(a) && remainingMs(a) > 0 && worthInterrupting(a))
    : [];

export const settleMotion = (ms = FAST_MS): boolean => {
  const hooks = [...waiting];
  waiting.clear();
  for (const hook of hooks) hook();
  const animations = running();
  for (const animation of animations) hurryAnimation(animation, ms);
  return hooks.length > 0 || animations.length > 0;
};

export const installMotionInterrupt = (): (() => void) => {
  const onKeydown = (event: KeyboardEvent) => {
    if (event.key !== 'Escape' || event.repeat || event.isComposing) return;
    if (!escapeMotion() && !settleMotion()) return;
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  window.addEventListener('keydown', onKeydown, true);
  return () => window.removeEventListener('keydown', onKeydown, true);
};
