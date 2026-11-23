import { afterEach, describe, expect, it, vi } from 'vitest';
import { springBack } from '../../popoverMorph';
import { resetLinearProbe, SPRINGS, springCurve } from '../spring';

describe('spring easing', () => {
  it('goes from 0 to 1 with a sensible settle time; may overshoot a little but does not jump', () => {
    for (const spring of Object.values(SPRINGS)) {
      const { easing, duration } = springCurve(spring);
      const points = easing.slice('linear('.length, -1).split(',').map(Number);
      expect(points[0]).toBe(0);
      expect(points[points.length - 1]).toBe(1);
      expect(points).toHaveLength(41);
      expect(Math.max(...points)).toBeLessThan(1.15);
      expect(duration).toBeGreaterThan(250);
      expect(duration).toBeLessThan(1600);
    }
  });

  it('the same spring is computed only once', () => {
    expect(springCurve(SPRINGS.deal)).toBe(springCurve(SPRINGS.deal));
  });

  describe('when the engine does not support linear()', () => {
    afterEach(() => {
      vi.unstubAllGlobals();
      resetLinearProbe();
    });

    it('falls back to a cubic bezier with the same duration; an overshooting spring uses the one with bounce', () => {
      const linear = springCurve(SPRINGS.pop);
      const settle = springCurve(SPRINGS.settle);
      vi.stubGlobal('CSS', { supports: () => false });
      resetLinearProbe();
      const pop = springCurve(SPRINGS.pop);
      expect(pop.easing).toBe('cubic-bezier(.2, 1.15, .32, 1)');
      expect(pop.duration).toBe(linear.duration);
      expect(springCurve(SPRINGS.settle)).toEqual({ easing: 'cubic-bezier(.4, .6, .2, 1)', duration: settle.duration });
      expect(springBack(SPRINGS.settle, { x: 0, y: 40 }, { x: 0, y: -0.8 }).easing.startsWith('cubic-bezier(')).toBe(true);
    });
  });
});
