import type { PlanIntensity } from '../../types/trainingPlan';

export const INTENSITY_COLOR: Record<PlanIntensity, string> = {
  warmup: 'var(--altitude)',
  active: 'var(--training)',
  interval: 'var(--heart)',
  recovery: 'var(--activity)',
  rest: 'var(--subtle)',
  cooldown: 'var(--sleep-light)',
};

export const INTENSITY_ORDER: readonly PlanIntensity[] = ['warmup', 'active', 'interval', 'recovery', 'cooldown', 'rest'];

export const usedIntensities = (segments: { intensity: PlanIntensity }[]): PlanIntensity[] => {
  const used = new Set(segments.map((segment) => segment.intensity));
  return INTENSITY_ORDER.filter((intensity) => used.has(intensity));
};
