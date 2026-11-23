import { describe, expect, it } from 'vitest';
import catalog from '../../assets/workouts/catalog.json';
import { workoutNameMessages } from '../workoutNames.i18n';
import { workoutLabel } from '../labels';

const tables = workoutNameMessages as unknown as { en: Record<string, string> };

describe('lib/workoutNames', () => {
  it('matches the bundled catalog', () => {
    for (const sport of catalog.sports) {
      expect(tables.en[sport.key], sport.key).toBe(catalog.sports.find((s) => s.key === sport.key)!.label_en);
    }
    expect(Object.keys(tables.en).length).toBe(new Set(catalog.sports.map((s) => s.key)).size);
  });

  it('names a workout by key', () => {
    expect(workoutLabel('trail_running')).toBe('Trail Running');
  });
});
