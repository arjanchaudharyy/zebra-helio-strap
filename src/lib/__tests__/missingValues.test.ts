import { describe, expect, it } from 'vitest';
import {
  coveredWindowValue,
  finiteOrNull,
  minutesToHours,
  stageMinutesForBar,
} from '../missingValues';


describe('a missing value never becomes 0', () => {
  it('null, undefined and NaN become null; a real 0 stays', () => {
    expect(finiteOrNull(null)).toBeNull();
    expect(finiteOrNull(undefined)).toBeNull();
    expect(finiteOrNull(Number.NaN)).toBeNull();
    expect(finiteOrNull(Number.POSITIVE_INFINITY)).toBeNull();
    expect(finiteOrNull(0)).toBe(0);
    expect(finiteOrNull(12)).toBe(12);
  });

  it('missing REM does not draw a 0-minute bar', () => {
    expect(stageMinutesForBar(null)).toBeNull();
    expect(stageMinutesForBar(undefined)).toBeNull();
    expect(stageMinutesForBar(0)).toBe(0);
    expect(stageMinutesForBar(48)).toBe(48);
  });

  it('missing minutes do not become 0.0 hours', () => {
    expect(minutesToHours(null)).toBeNull();
    expect(minutesToHours(undefined)).toBeNull();
    expect(minutesToHours(Number.NaN)).toBeNull();
    expect(minutesToHours(0)).toBe(0);
    expect(minutesToHours(90)).toBe(1.5);
    expect(minutesToHours(6)).toBe(0.1);
  });

  it('a window with 0 days of data does not plot a 0.0 load', () => {
    expect(coveredWindowValue(0, 0)).toBeNull();
    expect(coveredWindowValue(12.4, 0)).toBeNull();
    expect(coveredWindowValue(0, 3)).toBe(0);
    expect(coveredWindowValue(18, 7)).toBe(18);
  });
});
