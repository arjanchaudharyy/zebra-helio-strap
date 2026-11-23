import { KILOGRAMS_PER_POUND } from './units';
import type { StrengthWeightUnit } from '../types';

export function splitSetDuration(ms: number | null | undefined): { minutes: number; seconds: number; digits: 0 | 1 | 2 } | null {
  if (ms == null || !Number.isFinite(ms) || ms <= 0) return null;
  const totalCs = Math.floor(ms / 10);
  const minutes = Math.floor(totalCs / 6000);
  const cs = totalCs % 6000;
  const digits = cs % 100 === 0 ? 0 : cs % 10 === 0 ? 1 : 2;
  return { minutes, seconds: cs / 100, digits };
}

export function volumeUnitFor(units: ReadonlyArray<StrengthWeightUnit | null | undefined>, preferred: 'kg' | 'lb'): 'kg' | 'lb' {
  const seen = new Set(units.filter((unit): unit is StrengthWeightUnit => unit === 'kg' || unit === 'lb'));
  return seen.size === 1 ? [...seen][0] as 'kg' | 'lb' : preferred;
}

export const kgIn = (kg: number, unit: 'kg' | 'lb'): number => (unit === 'lb' ? kg / KILOGRAMS_PER_POUND : kg);
