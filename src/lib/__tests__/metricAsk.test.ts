import { describe, expect, it } from 'vitest';
import { daysEndingOn, sleepAskPresets, workoutAskPresets } from '../metricAsk';

describe('workoutAskPresets', () => {
  it('every question carries this one workout', () => {
    for (const preset of workoutAskPresets()) {
      expect(preset.categories).toContain('workout');
      expect(preset.daysBy?.workout).toBe(1);
    }
  });

  it('the review brings the detailed workout plus the week around it', () => {
    const review = workoutAskPresets().find((preset) => preset.key === 'workoutReview')!;
    expect(review.detail).toBe('detailed');
    expect(review.categories).toEqual(expect.arrayContaining(['training', 'recovery', 'sleep', 'resting_hr']));
  });
});

describe('sleepAskPresets', () => {
  it('always brings sleep, recovery and resting heart rate', () => {
    for (const preset of sleepAskPresets()) expect(preset.categories).toEqual(expect.arrayContaining(['sleep', 'recovery', 'resting_hr']));
  });
});

describe('daysEndingOn', () => {
  it('lists the days up to and including the end day, across a month edge', () => {
    expect(daysEndingOn('2026-10-02', 4)).toEqual(['2026-09-29', '2026-09-30', '2026-10-01', '2026-10-02']);
  });

  it('is empty for a bad date', () => {
    expect(daysEndingOn('nope', 3)).toEqual([]);
  });
});
