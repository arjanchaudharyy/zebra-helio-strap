import { computed, ref } from 'vue';
import { defineMessages, messagesOf } from '../i18n';

export type DistanceUnit = 'metric' | 'imperial';

export const DISTANCE_UNITS: readonly DistanceUnit[] = ['metric', 'imperial'];

const STORAGE_KEY = 'zeppbridge-distance-unit';

export const KILOGRAMS_PER_POUND = 0.45359237;
export const CENTIMETRES_PER_INCH = 2.54;

export const METRES_PER_MILE = 1609.344;
export const METRES_PER_FOOT = 0.3048;

const isDistanceUnit = (value: unknown): value is DistanceUnit =>
  value === 'metric' || value === 'imperial';

const IMPERIAL_REGIONS = new Set(['US', 'LR', 'MM']);

const detectUnit = (): DistanceUnit => {
  if (typeof window === 'undefined') return 'metric';
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (isDistanceUnit(saved)) return saved;
  } catch {
  }
  const preferred = window.navigator.languages?.[0] ?? window.navigator.language ?? '';
  const region = preferred.split('-')[1]?.toUpperCase();
  return region && IMPERIAL_REGIONS.has(region) ? 'imperial' : 'metric';
};

const current = ref<DistanceUnit>(detectUnit());

export const distanceUnit = computed<DistanceUnit>(() => current.value);

export const setDistanceUnit = (value: DistanceUnit) => {
  if (!isDistanceUnit(value) || value === current.value) return;
  current.value = value;
  try {
    window.localStorage.setItem(STORAGE_KEY, value);
  } catch {
  }
};

const imperial = () => current.value === 'imperial';


const unitWords = defineMessages(
  { big: 'km', short: 'm', bigImperial: 'mi', shortImperial: 'ft' },
);

export const distanceUnitLabel = (): string => {
  const words = messagesOf(unitWords);
  return imperial() ? words.bigImperial : words.big;
};
export const shortDistanceUnitLabel = (): string => {
  const words = messagesOf(unitWords);
  return imperial() ? words.shortImperial : words.short;
};
export const distanceUnitOptionLabel = (unit: DistanceUnit): string => {
  const words = messagesOf(unitWords);
  return unit === 'imperial' ? words.bigImperial : words.big;
};

export const paceUnitLabel = (): string => (imperial() ? '/mi' : '/km');
export const paceAxisLabel = (): string => (imperial() ? 'min/mi' : 'min/km');

export const toBigDistance = (meters: number): number =>
  meters / (imperial() ? METRES_PER_MILE : 1000);

export const toShortDistance = (meters: number): number =>
  imperial() ? meters / METRES_PER_FOOT : meters;

export const bigDistanceThresholdMeters = (): number => (imperial() ? METRES_PER_MILE : 1000);

export const toElevation = (meters: number): number =>
  imperial() ? meters / METRES_PER_FOOT : meters;
export const elevationUnitLabel = (): string => shortDistanceUnitLabel();

export const paceSecondsPerBigUnit = (secondsPerKm: number): number =>
  imperial() ? secondsPerKm * (METRES_PER_MILE / 1000) : secondsPerKm;

export const paceMinutesPerBigUnit = (minutesPerKm: number): number =>
  imperial() ? minutesPerKm * (METRES_PER_MILE / 1000) : minutesPerKm;

export const toBodyMass = (kilograms: number): number =>
  imperial() ? kilograms / KILOGRAMS_PER_POUND : kilograms;

export const bodyMassUnitLabel = (): string => (imperial() ? 'lb' : 'kg');

export const toBodyHeight = (centimetres: number): number =>
  imperial() ? centimetres / CENTIMETRES_PER_INCH : centimetres;

export const bodyHeightUnitLabel = (): string => (imperial() ? 'in' : 'cm');
