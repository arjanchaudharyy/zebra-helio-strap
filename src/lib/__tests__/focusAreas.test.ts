import { describe, expect, it } from 'vitest';
import { FOCUS_AREAS, FOCUS_CHOICES, MODULE_AREA, areasForChoice, focusChoiceOf, focusSplit, normalizeAreas } from '../focusAreas';

describe('normalizeAreas', () => {
  it('keeps known areas only, removes duplicates, keeps click order', () => {
    expect(normalizeAreas(['training', 'sleep', 'training', 'daily', 'nonsense', 3])).toEqual(['training', 'sleep', 'daily']);
  });

  it('treats a non-array as no choice', () => {
    expect(normalizeAreas(null)).toEqual([]);
    expect(normalizeAreas('sleep')).toEqual([]);
    expect(normalizeAreas(undefined)).toEqual([]);
  });
});

describe('focusSplit', () => {
  it('no choice means no filter (page as is)', () => {
    expect(focusSplit([])).toBeNull();
  });

  it('all three chosen is the same as none, with no fake layout', () => {
    expect(focusSplit(['sleep', 'daily', 'training'])).toBeNull();
  });

  it('chosen modules come first in fixed order, the others fold', () => {
    expect(focusSplit(['training', 'sleep'])).toEqual({
      matched: ['sleep', 'training'],
      folded: ['heart', 'steps', 'body'],
    });
  });

  it('daily only: heart / steps / body first, sleep and training fold', () => {
    expect(focusSplit(['daily'])).toEqual({
      matched: ['heart', 'steps', 'body'],
      folded: ['sleep', 'training'],
    });
  });

  it('treats unknown input as no choice', () => {
    expect(focusSplit(['whatever'])).toBeNull();
  });
});

describe('focus slider choice', () => {
  it('one area stops on that slot, everything else is "balanced"', () => {
    expect(focusChoiceOf(['sleep'])).toBe('sleep');
    expect(focusChoiceOf([])).toBe('all');
    expect(focusChoiceOf(['sleep', 'training'])).toBe('all');
    expect(focusChoiceOf(['nonsense'])).toBe('all');
  });

  it('"balanced" writes an empty set, the others write one area', () => {
    expect(areasForChoice('all')).toEqual([]);
    expect(areasForChoice('training')).toEqual(['training']);
    expect(FOCUS_CHOICES[0]).toBe('all');
  });
});

describe('MODULE_AREA', () => {
  it('each of the five modules has one area, and every area is a choice', () => {
    for (const area of Object.values(MODULE_AREA)) expect(FOCUS_AREAS).toContain(area);
  });
});
