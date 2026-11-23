import { lessMotion } from './lessMotion';
import { autoLite } from './tier';
import { DEVELOP_EASE, DEVELOP_MS } from './timing';

export type InkPhase = 'latent' | 'develop' | 'solid';

export const DEVELOP_GUARD_MS = 2000;
export const DEVELOP_DELAY_MS = 80;

const played = new Set<string>();

export const shouldDevelop = (key: string): boolean => !lessMotion() && !autoLite.value && !played.has(key);
export const markDeveloped = (key: string): void => { played.add(key); };
export const resetDevelop = (): void => { played.clear(); };

export const developWash = (wash: HTMLElement, done: () => void, duration = DEVELOP_MS): Animation | null => {
  if (lessMotion() || typeof wash.animate !== 'function') { done(); return null; }
  const run = wash.animate([{ transform: 'scaleX(1)' }, { transform: 'scaleX(0)' }], { duration, easing: DEVELOP_EASE, fill: 'forwards' });
  run.finished.then(done, done);
  return run;
};
