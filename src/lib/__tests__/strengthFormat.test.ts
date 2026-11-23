import { describe, expect, it } from 'vitest';
import { kgIn, splitSetDuration, volumeUnitFor } from '../strengthFormat';
import { expectsRoute } from '../workoutGps';

describe('splitSetDuration', () => {
  it('shows whole seconds without decimals', () => {
    expect(splitSetDuration(38_000)).toEqual({ minutes: 0, seconds: 38, digits: 0 });
    expect(splitSetDuration(72_000)).toEqual({ minutes: 1, seconds: 12, digits: 0 });
    expect(splitSetDuration(60_000)).toEqual({ minutes: 1, seconds: 0, digits: 0 });
  });
  it('shows decimals only for a fraction, drops trailing zeros, and truncates', () => {
    expect(splitSetDuration(22_467)).toEqual({ minutes: 0, seconds: 22.46, digits: 2 });
    expect(splitSetDuration(22_500)).toEqual({ minutes: 0, seconds: 22.5, digits: 1 });
  });
  it('does not invent a duration when there is none', () => {
    expect(splitSetDuration(null)).toBeNull();
    expect(splitSetDuration(0)).toBeNull();
    expect(splitSetDuration(-5)).toBeNull();
  });
});

describe('volumeUnitFor', () => {
  it('uses the shared set unit, and the setting only for mixed units', () => {
    expect(volumeUnitFor(['kg', 'kg', null], 'lb')).toBe('kg');
    expect(volumeUnitFor(['lb', 'lb'], 'kg')).toBe('lb');
    expect(volumeUnitFor(['kg', 'lb'], 'lb')).toBe('lb');
    expect(volumeUnitFor([], 'kg')).toBe('kg');
  });
  it('converts units', () => {
    expect(kgIn(100, 'kg')).toBe(100);
    expect(kgIn(0.45359237, 'lb')).toBeCloseTo(1, 6);
  });
});

describe('expectsRoute', () => {
  it('a workout without distance expects no route', () => {
    expect(expectsRoute('strength', null)).toBe(false);
    expect(expectsRoute('strength', 0)).toBe(false);
  });
  it('an outdoor workout with distance expects a route', () => {
    expect(expectsRoute('outdoor_running', 5000)).toBe(true);
    expect(expectsRoute('outdoor_cycling', 20000)).toBe(true);
  });
  it('indoor, treadmill and pool workouts expect no GPS even with distance', () => {
    expect(expectsRoute('indoor_cycling', 20000)).toBe(false);
    expect(expectsRoute('treadmill', 5000)).toBe(false);
    expect(expectsRoute('pool_swimming', 1500)).toBe(false);
  });
});
