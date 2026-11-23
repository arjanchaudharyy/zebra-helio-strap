import { afterEach, describe, expect, it } from 'vitest';
import { formatDistance } from '../format';
import { formatPaceSeconds } from '../metricSeries';
import {
  METRES_PER_FOOT,
  METRES_PER_MILE,
  distanceUnitLabel,
  paceUnitLabel,
  setDistanceUnit,
  toElevation,
} from '../units';


describe('imperial shows the same distance in other units', () => {
  afterEach(() => {
    setDistanceUnit('metric');
  });

  it('uses miles from one mile up and feet below', () => {
    setDistanceUnit('imperial');
    expect(formatDistance(METRES_PER_MILE)).toBe('1.00 mi');
    expect(formatDistance(METRES_PER_MILE - 1)).toBe(`${Math.round((METRES_PER_MILE - 1) / METRES_PER_FOOT)} ft`);
    expect(formatDistance(10_000)).toBe('6.21 mi');
  });

  it('moves the threshold with the unit, not always 1000 m', () => {
    expect(formatDistance(1500)).toBe('1.50 km');
    setDistanceUnit('imperial');
    expect(formatDistance(1500)).toContain('ft');
  });

  it('changes the pace suffix', () => {
    setDistanceUnit('imperial');
    expect(paceUnitLabel()).toBe('/mi');
  });

  it('converts seconds-per-km pace too', () => {
    expect(formatPaceSeconds(300)).toBe('5:00');
    setDistanceUnit('imperial');
    expect(formatPaceSeconds(300)).toBe('8:03');
  });

  it('converts elevation gain to feet', () => {
    expect(toElevation(100)).toBe(100);
    setDistanceUnit('imperial');
    expect(Math.round(toElevation(100))).toBe(328);
  });

  it('keeps a missing value missing, not 0, after a unit change', () => {
    setDistanceUnit('imperial');
    expect(formatDistance(undefined)).toBe('Not recorded');
    expect(formatDistance(0)).toBe('Not recorded');
  });

  it('uses unit symbols for the unit names', () => {
    expect(distanceUnitLabel()).toBe('km');
    setDistanceUnit('imperial');
    expect(distanceUnitLabel()).toBe('mi');
    expect(formatDistance(METRES_PER_MILE)).toBe('1.00 mi');
  });
});
