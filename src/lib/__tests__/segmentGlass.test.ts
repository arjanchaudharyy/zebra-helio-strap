import { describe, expect, it } from 'vitest';
import { flickDeform, LIFT_PAD_X, liftRect, rubberStretch, stretchLimit } from '../segmentGlass';
import { shiftInto } from '../edgeSafe';

describe('liftRect', () => {
  it('follows the thumb width and centers the label (no scale from the width at lift time)', () => {
    const narrow = liftRect({ left: 100, width: 60, top: 3, height: 32 }, 300, 3);
    const wide = liftRect({ left: 100, width: 130, top: 3, height: 32 }, 300, 3);
    expect(narrow.w).toBe(60 + LIFT_PAD_X * 2);
    expect(wide.w).toBe(130 + LIFT_PAD_X * 2);
    expect(narrow.x + narrow.w / 2).toBe(130);
    expect(wide.x + wide.w / 2).toBe(165);
  });

  it('stays inside the track at both ends and stays centered on the thumb', () => {
    const first = liftRect({ left: 3, width: 60, top: 3, height: 32 }, 300, 3);
    expect(first.x).toBeGreaterThanOrEqual(0);
    expect(first.x + first.w / 2).toBe(33);
    expect(first.w).toBeGreaterThanOrEqual(60);
    const last = liftRect({ left: 237, width: 60, top: 3, height: 32 }, 300, 3);
    expect(last.x + last.w).toBeLessThanOrEqual(300);
    expect(last.x + last.w / 2).toBe(267);
  });
});

describe('rubberStretch', () => {
  it('resists more as it stretches, never reaches the limit, and follows the finger direction', () => {
    const max = stretchLimit(400);
    expect(rubberStretch(0, max)).toBe(0);
    expect(rubberStretch(-40, max)).toBeLessThan(0);
    expect(rubberStretch(40, max)).toBeGreaterThan(0);
    expect(rubberStretch(4000, max)).toBeLessThan(max);
    expect(rubberStretch(80, max) - rubberStretch(40, max)).toBeLessThan(rubberStretch(40, max));
  });

  it('caps the deform at any speed', () => {
    expect(Math.abs(flickDeform(-50))).toBeLessThanOrEqual(0.035);
    expect(flickDeform(-2)).toBeLessThan(0);
  });
});

describe('shiftInto', () => {
  const box = { left: 100, right: 500 };
  it('pushes right with a margin when it overflows the left', () => expect(shiftInto(40, 200, box)).toBe(68));
  it('pushes left when it overflows the right', () => expect(shiftInto(400, 560, box)).toBe(-68));
  it('does not move when it fits', () => expect(shiftInto(150, 300, box)).toBe(0));
});
