import { describe, expect, it } from 'vitest';
import { CLOSE_VEIL, SOURCE_IN } from '../motion/dialogFlight';

describe('dialog close timeline', () => {
  const veilGone = Math.max(...CLOSE_VEIL.filter((frame) => Number(frame.opacity) < 0.01).map((frame) => Number(frame.offset ?? 0)));

  it('clears the veil before the real card starts to come back', () => {
    expect(veilGone).toBeGreaterThanOrEqual(SOURCE_IN[1]);
  });

  it('starts clearing the veil as soon as the close starts', () => {
    const fullUntil = Math.min(...CLOSE_VEIL.filter((frame) => Number(frame.opacity) >= 1 && frame.offset !== undefined).map((frame) => Number(frame.offset)));
    expect(fullUntil).toBeGreaterThan(0.95);
  });

  it('brings the real card back before the panel fades', () => {
    expect(SOURCE_IN[0]).toBeGreaterThan(0.12);
    expect(SOURCE_IN[0]).toBeLessThan(SOURCE_IN[1]);
  });
});
