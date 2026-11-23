import { describe, expect, it } from 'vitest';
import type { Workout } from '../../../types';
import type { AiTask, AiTaskCategory } from '../../bridge/types';
import { newTaskDraft } from '../draft';
import { formatDate } from '../../format';
import { workoutDisplayLabel } from '../../workouts';
import { autoTaskTitle, recentWindowDays } from '../title';

const workout = (patch: Partial<Workout> = {}): Workout => ({
  workout_id: 'w-1',
  workout_type: 'run',
  normalized_type: 'run',
  type_source: 'numeric_mapped',
  effective_type: 'run',
  start_time: '2026-03-10T08:00:00+08:00',
  end_time: '2026-03-10T09:00:00+08:00',
  distance_meters: 10000,
  source_scope: 'device',
  ...patch,
});

const setRange = (
  task: AiTask,
  category: AiTaskCategory,
  patch: Partial<AiTask['categories'][number]>,
): AiTask => ({
  ...task,
  categories: task.categories.map((range) =>
    range.category === category ? { ...range, ...patch } : range),
});

describe('recentWindowDays', () => {
  it('default draft has all windows on at 14 days, gives 14', () => {
    expect(recentWindowDays(newTaskDraft())).toBe(14);
  });

  it('takes the longest lookback of enabled window categories; disabled and windowless categories do not count', () => {
    let task = newTaskDraft();
    task = setRange(task, 'sleep', { days_before: 29 });
    task = setRange(task, 'heart_rate', { enabled: false, days_before: 60 });
    task = setRange(task, 'personal_note', { enabled: true, days_before: 99 });
    expect(recentWindowDays(task)).toBe(30);
  });

  it('all window categories off, falls back to 14', () => {
    let task = newTaskDraft();
    for (const range of task.categories) {
      task = setRange(task, range.category, { enabled: false });
    }
    expect(recentWindowDays(task)).toBe(14);
  });
});

describe('autoTaskTitle', () => {
  it('exactly one workout: `workout name · start date`', () => {
    const run = workout();
    expect(autoTaskTitle(newTaskDraft(), [run], null)).toBe(
      `${workoutDisplayLabel(run)} · ${formatDate(run.start_time)}`,
    );
  });

  it('several workouts: head is the earliest one + "+N more", date is the earliest start', () => {
    const early = workout({ workout_id: 'w-early', start_time: '2026-03-08T07:00:00+08:00' });
    const mid = workout({ workout_id: 'w-mid' });
    const late = workout({ workout_id: 'w-late', start_time: '2026-03-12T08:00:00+08:00' });
    const title = autoTaskTitle(newTaskDraft(), [mid, late, early], null);
    expect(title.startsWith(workoutDisplayLabel(early))).toBe(true);
    expect(title.endsWith(`· ${formatDate(early.start_time)}`)).toBe(true);
    expect(title).toContain(' +2 more');
    expect(title).not.toBe(`${workoutDisplayLabel(early)} · ${formatDate(early.start_time)}`);
  });

  it('no workout: "Last N days · today", N follows the enabled windows', () => {
    const now = new Date('2026-09-24T12:00:00+08:00');
    expect(autoTaskTitle(newTaskDraft(), [], null, now)).toBe(
      `Last 14 days · ${formatDate(now.toISOString())}`,
    );
    const task = setRange(newTaskDraft(), 'sleep', { days_before: 29 });
    const title = autoTaskTitle(task, [], null, now);
    expect(title.startsWith('Last 30 days')).toBe(true);
    expect(title.endsWith(`· ${formatDate(now.toISOString())}`)).toBe(true);
  });

  it('a template replaces the whole head with its name; the date rule stays', () => {
    const run = workout();
    expect(autoTaskTitle(newTaskDraft(), [run], 'Recovery run')).toBe(
      `Recovery run · ${formatDate(run.start_time)}`,
    );
    const now = new Date('2026-09-24T12:00:00+08:00');
    const title = autoTaskTitle(newTaskDraft(), [], 'Recovery run', now);
    expect(title.startsWith('Recovery run')).toBe(true);
    expect(title.endsWith(`· ${formatDate(now.toISOString())}`)).toBe(true);
  });
});
