import { describe, expect, it } from 'vitest';
import { bridgeRect, gapBetween, groupState, lensSpan, mergeAmount, poseStyle, RESTING, springFrames, stretchPose, unionRect } from '../glassGroup';
import { squash } from '../segmentGlass';

describe('pose matches the old style', () => {
  it('writes no scale when not stretched', () => {
    expect(poseStyle(stretchPose(0, 300))).toEqual({});
    expect(stretchPose(0, 300, true).active).toBe(true);
  });

  it('a pull left pins the right end, a pull right pins the left end, with the same scale formula', () => {
    expect(poseStyle(stretchPose(-12, 300))).toEqual({ scale: squash(1 + 12 / 300), transformOrigin: 'right center' });
    expect(poseStyle(stretchPose(9, 300))).toEqual({ scale: squash(1 + 9 / 300), transformOrigin: 'left center' });
    expect(stretchPose(5, 0).sx).toBe(6);
  });
});

describe('springFrames', () => {
  it('no spring with no stretch and no fling, or with zero width', () => {
    expect(springFrames(0, 0, 300)).toBeNull();
    expect(springFrames(10, 0, 0)).toBeNull();
  });

  it('plain release: three frames, 460ms, back to 1', () => {
    const spring = springFrames(-15, 0, 300)!;
    expect(spring.duration).toBe(460);
    expect(spring.frames).toHaveLength(3);
    expect(spring.frames[0]).toMatchObject({ scale: squash(1 + 15 / 300), transformOrigin: 'right center' });
    expect(spring.frames[spring.frames.length - 1]).toEqual({ scale: '1 1', transformOrigin: 'right center' });
  });

  it('hard fling: one more overshoot frame, 560ms, the pull back is 30% of the overshoot', () => {
    const spring = springFrames(0, 0.03, 300)!;
    expect(spring.duration).toBe(560);
    expect(spring.frames).toHaveLength(4);
    expect(spring.frames[1]).toMatchObject({ scale: squash(1.03), offset: 0.26, transformOrigin: 'left center' });
    expect(spring.frames[2]).toMatchObject({ scale: squash(1 - 0.03 * 0.3), offset: 0.62 });
  });
});

describe('lensSpan', () => {
  it('interpolates between neighbors and clamps at both ends', () => {
    expect(lensSpan([40, 80, 60], 0.5, false)).toBe(60);
    expect(lensSpan([40, 80, 60], 5, false)).toBe(60);
    expect(lensSpan([40, 80, 60], -1, false)).toBe(40);
    expect(lensSpan([], 1, false)).toBe(0);
  });

  it('wraps the ends when looping', () => {
    expect(lensSpan([40, 80, 60], 2.5, true)).toBe(50);
    expect(lensSpan([40, 80, 60], -0.5, true)).toBe(50);
  });
});

describe('grouping', () => {
  const a = { x: 0, y: 0, w: 100, h: 40 };

  it('gap: positive when apart, 0 when touching, negative when overlapping', () => {
    expect(gapBetween(a, { x: 130, y: 0, w: 50, h: 40 })).toBe(30);
    expect(gapBetween(a, { x: 100, y: 0, w: 50, h: 40 })).toBe(0);
    expect(gapBetween(a, { x: 90, y: 0, w: 50, h: 40 })).toBe(-10);
    expect(gapBetween(a, { x: 103, y: 44, w: 10, h: 10 })).toBe(5);
  });

  it('merge amount: 0 when far, 1 when touching, smooth between', () => {
    expect(mergeAmount(30, 24)).toBe(0);
    expect(mergeAmount(0, 24)).toBe(1);
    expect(mergeAmount(-5, 24)).toBe(1);
    expect(mergeAmount(12, 24)).toBeCloseTo(0.5);
    expect(mergeAmount(18, 24)).toBeLessThan(0.25);
  });

  it('group state: union rect, active when any member is active', () => {
    expect(unionRect([])).toBeNull();
    const state = groupState([
      { id: 'a', rect: a, pose: RESTING },
      { id: 'b', rect: { x: 104, y: 0, w: 60, h: 40 }, pose: { ...RESTING, active: true } },
      { id: 'c', rect: null, pose: RESTING },
    ], 24);
    expect(state.active).toBe(true);
    expect(state.merged).toBe(true);
    expect(state.union).toEqual({ x: 0, y: 0, w: 164, h: 40 });
    expect(groupState([{ id: 'a', rect: a, pose: RESTING }], 24)).toMatchObject({ active: false, amount: 0, merged: false });
  });
});

describe('bridgeRect (the D2 neck)', () => {
  const pill = { x: 200, y: 0, w: 120, h: 36 };

  it('vertical: centered on the horizontal overlap, 8px into each end', () => {
    const tip = { x: 100, y: 46, w: 380, h: 90 };
    expect(bridgeRect(pill, tip, 'y', 24)).toEqual({ x: 246, y: 28, w: 28, h: 26 });
    expect(bridgeRect(tip, pill, 'y', 24)).toEqual({ x: 246, y: 28, w: 28, h: 26 });
  });

  it('horizontal: thickness is half the shorter piece', () => {
    const saved = { x: 330, y: 2, w: 90, h: 32 };
    expect(bridgeRect(pill, saved, 'x', 24)).toEqual({ x: 312, y: 10, w: 26, h: 16 });
  });

  it('no neck when too far, on another row, or with no overlap on the other axis', () => {
    expect(bridgeRect(pill, { x: 360, y: 0, w: 60, h: 36 }, 'x', 24)).toBeNull();
    expect(bridgeRect(pill, { x: 0, y: 50, w: 90, h: 36 }, 'x', 24)).toBeNull();
    expect(bridgeRect(pill, { x: 0, y: 46, w: 100, h: 36 }, 'y', 24)).toBeNull();
  });
});
