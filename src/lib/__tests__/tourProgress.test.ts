import { describe, expect, it } from 'vitest';
import { ALL_CHAPTERS, SECTIONS, TOUR_STEPS, type TourStep } from '../tour/chapters';
import { tourMessages } from '../../components/tour/tour.i18n';
import { TOUR_VERSION, chaptersToMark, needsRetour, presentSteps, storedTourVersion } from '../tour/progress';

describe('tour: when a chapter counts as seen', () => {
  it('done marks the whole run, skip marks only the current chapter, pause and abort mark nothing', () => {
    const chapters = ['intro', 'home'] as const;
    expect(chaptersToMark('done', chapters, 1)).toEqual(['intro', 'home']);
    expect(chaptersToMark('skip', chapters, 0)).toEqual(['intro']);
    expect(chaptersToMark('pause', chapters, 1)).toEqual([]);
    expect(chaptersToMark('abort', chapters, 0)).toEqual([]);
  });

  it('an old install on an old tour version retours; a fresh install or the current version does not', () => {
    expect(storedTourVersion(null)).toBe(1);
    expect(storedTourVersion('garbage')).toBe(1);
    expect(needsRetour(true, null)).toBe(true);
    expect(needsRetour(true, '1')).toBe(true);
    expect(needsRetour(true, String(TOUR_VERSION))).toBe(false);
    expect(needsRetour(false, null)).toBe(false);
  });
});

describe('tour: in-place steps cover only what is on screen', () => {
  const steps: TourStep[] = [
    { target: '.a' },
    { target: '.missing' },
    { target: '.card', open: 'pickDays' },
    { target: null },
  ];

  it('drops steps with no target and keeps the original indexes (copy numbers stay the same)', () => {
    const visible = (css: string) => css === '.a' || css === '.pick-days';
    expect(presentSteps(steps, visible)).toEqual([0, 2, 3]);
  });

  it('a step that opens the deck first checks for the pick-days button', () => {
    expect(presentSteps(steps, (css) => css === '.a')).toEqual([0, 3]);
  });

  it('an empty page shows no steps', () => {
    expect(presentSteps(steps.slice(0, 3), () => false)).toEqual([]);
  });
});

describe('tour: chapters', () => {
  it('intro + nine sections, every chapter has steps, intro has 6 steps', () => {
    expect(SECTIONS).toHaveLength(9);
    expect(ALL_CHAPTERS).toHaveLength(10);
    for (const id of ALL_CHAPTERS) expect(TOUR_STEPS[id].length).toBeGreaterThan(0);
    expect(TOUR_STEPS.intro).toHaveLength(6);
  });

  it('every step has English copy and no extra keys', () => {
    for (const bundle of [tourMessages.en]) {
      for (const id of ALL_CHAPTERS) {
        const copy = bundle.steps[id] as Record<string, string>;
        const keys = Object.keys(copy).filter((key) => /^s\d+$/.test(key));
        expect(keys.sort()).toEqual(TOUR_STEPS[id].map((_, index) => `s${index + 1}`).sort());
      }
    }
  });

  it('the sandbox changes pages: every step names its route', () => {
    for (const id of ALL_CHAPTERS) for (const step of TOUR_STEPS[id]) expect(step.route).toMatch(/^\//);
  });
});
