import { describe, expect, it } from 'vitest';
import type { Workout } from '../../types';
import {
  displayableWorkouts,
  hasWorkoutIdentity,
  isDisplayableWorkout,
  workoutDisplayLabel,
  workoutDisplayType,
  workoutDurationMinutes,
} from '../workouts';

const base = (overrides: Partial<Workout> = {}): Workout =>
  ({
    workout_id: 'w-1',
    workout_type: 'run',
    normalized_type: 'run',
    type_source: 'catalog',
    effective_type: 'run',
    start_time: '2026-03-01T08:00:00+08:00',
    end_time: '2026-03-01T08:40:00+08:00',
    source_scope: 'device',
    gps_available: false,
    sample_count: 0,
    ...overrides,
  }) as Workout;

describe('which workouts can show in the UI', () => {
  it('shows automatically detected trail running', () => {
    const trail = base({
      zepp_type: 7,
      workout_type: 'trail_running',
      normalized_type: 'trail_running',
      effective_type: 'trail_running',
      type_source: 'numeric_mapped',
    });
    expect(workoutDisplayLabel(trail)).toBe('Trail Running');
    expect(workoutDisplayLabel(base({
      ...trail, user_override: 'open_water_swimming', effective_type: 'open_water_swimming',
    }))).toBe('Open Water Swimming');
  });

  it('does not count a decoded empty shell as a workout', () => {
    const shell = base({
      end_time: '2026-03-01T08:00:00+08:00',
      distance_meters: undefined,
      calories: undefined,
      avg_hr: undefined,
    });
    expect(isDisplayableWorkout(shell)).toBe(false);
  });

  it('needs only one real metric', () => {
    expect(isDisplayableWorkout(base({ calories: 210 }))).toBe(true);
    expect(isDisplayableWorkout(base({ distance_meters: 5000 }))).toBe(true);
    expect(isDisplayableWorkout(base())).toBe(true);
  });

  it('does not count a 0 value as a metric', () => {
    const zeros = base({
      end_time: '2026-03-01T08:00:00+08:00',
      distance_meters: 0,
      calories: 0,
      avg_hr: 0,
      max_hr: 0,
    });
    expect(isDisplayableWorkout(zeros)).toBe(false);
  });

  it('gives no identity to a record with an invalid timestamp', () => {
    expect(hasWorkoutIdentity(base({ start_time: 'not a date' }))).toBe(false);
    expect(hasWorkoutIdentity(base({ workout_type: '   ' }))).toBe(false);
  });

  it('keeps the original order when it filters', () => {
    const list = [base({ workout_id: 'a', calories: 100 }), base({ workout_id: 'b', calories: 200 })];
    expect(displayableWorkouts(list).map((item) => item.workout_id)).toEqual(['a', 'b']);
  });
});

describe('duration', () => {
  it('uses start and end time, also across time zones', () => {
    expect(workoutDurationMinutes(base())).toBe(40);
  });

  it('returns null, not a negative or 0, when the end is not after the start', () => {
    expect(workoutDurationMinutes(base({ end_time: '2026-03-01T07:00:00+08:00' }))).toBeNull();
    expect(workoutDurationMinutes(base({ end_time: '2026-03-01T08:00:00+08:00' }))).toBeNull();
  });

  it('returns null when a time is missing', () => {
    expect(workoutDurationMinutes({ start_time: '2026-03-01T08:00:00+08:00' })).toBeNull();
  });
});

describe('workout type display', () => {
  it('prefers the user override to the cloud type', () => {
    const overridden = base({ effective_type: 'hiking', workout_type: 'run' });
    expect(workoutDisplayType(overridden)).toBe('hiking');
  });

  it('uses the custom name given to a custom code', () => {
    const custom = base({
      workout_type: 'unknown:226',
      normalized_type: 'unknown:226',
      effective_type: 'unknown:226',
      custom_label: 'Squash',
    });
    expect(workoutDisplayLabel(custom)).toBe('Squash');
  });

  it('shows an unnamed code as unrecognized and does not guess a workout', () => {
    const custom = base({
      workout_type: 'unknown:226',
      normalized_type: 'unknown:226',
      effective_type: 'unknown:226',
      custom_label: undefined,
    });
    expect(workoutDisplayLabel(custom)).toBe('Unrecognized workout (code 226)');
  });

  it('shows the catalog name of a workout type', () => {
    const rucking = base({
      workout_type: 'rucking',
      normalized_type: 'rucking',
      effective_type: 'rucking',
    });
    expect(workoutDisplayLabel(rucking)).toBe('Rucking');
  });

  it('does not let a custom name replace a known type', () => {
    const known = base({ effective_type: 'run', custom_label: 'Made up' });
    expect(workoutDisplayLabel(known)).toBe('Outdoor Running');
  });
});
