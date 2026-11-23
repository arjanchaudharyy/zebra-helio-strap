import { describe, expect, it } from 'vitest';
import { planGuide } from '../planGuide';
import { extractPlan } from '../../trainingPlan/extract';

describe('planGuide', () => {
  const now = new Date(2026, 9, 2);

  it('the example uses future dates from today and is a code block that paste-back can read', () => {
    const guide = planGuide(now);
    expect(guide).toContain('"from":"2026-10-03"');
    expect(guide).toContain('"date":"2026-10-04"');
    const read = extractPlan(guide);
    expect(read.ok).toBe(true);
    if (read.ok) {
      expect(read.document.format).toBe('zeppbridge-plan/3');
      expect(read.document.workouts).toHaveLength(2);
      expect(read.document.to).toBe('2026-10-06');
      for (const workout of read.document.workouts) {
        expect(workout.focus).toBeTruthy();
        expect(workout.description).toBeTruthy();
      }
    }
  });

  // Source bug: the English example names are longer than the 14-character limit the guide states.
  it('example workout names stay within the 14-character watch limit', () => {
    const read = extractPlan(planGuide(now));
    expect(read.ok).toBe(true);
    if (read.ok) {
      for (const workout of read.document.workouts) expect(workout.name.length).toBeLessThanOrEqual(14);
    }
  });

  it('teaches only identifiers the parser accepts and the watch supports, limits match backend checks (56 days, 40 steps, 50 repeats)', () => {
    const guide = planGuide(now);
    for (const word of ['running', 'cycling', 'pool_swim', 'open_water_swim', 'treadmill', 'indoor', 'warmup', 'cooldown', 'hr 135-150', 'pace 5:00-5:30', '5km', 'focus', 'description']) {
      expect(guide).toContain(word);
    }
    expect(guide).toContain('56');
    expect(guide).toContain('40');
    expect(guide).toContain('50');
    expect(guide).toContain('14');
    expect(guide).toContain('Walking, hiking');
    expect(guide).toContain('rest note');
  });
});
