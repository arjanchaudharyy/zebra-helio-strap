import type { TourStep } from './chapters';

export const TOUR_VERSION = 2;

export type TourEnd = 'done' | 'skip' | 'pause' | 'abort';

export const chaptersToMark = <T extends string>(end: TourEnd, chapters: readonly T[], current: number): T[] => {
  if (end === 'done') return [...chapters];
  if (end === 'skip') return chapters[current] ? [chapters[current]!] : [];
  return [];
};

export const storedTourVersion = (raw: string | null): number => {
  const value = Number.parseInt(raw ?? '', 10);
  return Number.isFinite(value) && value > 0 ? value : 1;
};

export const needsRetour = (firstLaunchHandled: boolean, rawVersion: string | null): boolean =>
  firstLaunchHandled && storedTourVersion(rawVersion) < TOUR_VERSION;

export const presentSteps = (steps: readonly TourStep[], visible: (css: string) => boolean): number[] =>
  steps.flatMap((step, index) => {
    if (step.target === null) return [index];
    if (step.open === 'pickDays') return visible('.pick-days') ? [index] : [];
    return visible(step.target) ? [index] : [];
  });
