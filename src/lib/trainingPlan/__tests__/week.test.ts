import { describe, expect, it } from 'vitest';
import { changeCounts, dayRows, issueDate, issueStepPath, windowEnd } from '../week';
import type { PlanDayPreview, PlanDraftPreview, PlanIssue, PlanWorkout } from '../../../types/trainingPlan';

const workout = (date: string): PlanWorkout => ({ date, sport: 'running', name: 'w', steps: [] });
const day = (date: string, change: PlanDayPreview['change'], after: PlanWorkout[] = [], before: PlanWorkout[] = []): PlanDayPreview =>
  ({ date, change, after, before });

const preview = (days: PlanDayPreview[]): PlanDraftPreview => ({
  check: { from: null, to: null, workouts: [], issues: [] },
  window: { start: '2026-10-02' },
  days,
});

describe('windowEnd', () => {
  it('the window is 7 days inclusive: start + 6', () => {
    expect(windowEnd('2026-10-02')).toBe('2026-10-08');
    expect(windowEnd('2026-12-29')).toBe('2027-01-04');
  });
});

describe('dayRows', () => {
  const rows = dayRows(preview([
    day('2026-10-02', 'unchanged', [workout('2026-10-02')]),
    day('2026-10-08', 'added', [workout('2026-10-08')]),
    day('2026-10-09', 'added', [workout('2026-10-09')]),
  ]), '2026-10-02');

  it('marks which days are in the window and which is today; days outside are not sent', () => {
    expect(rows.map((row) => row.inWindow)).toEqual([true, true, false]);
    expect(rows.map((row) => row.isToday)).toEqual([true, false, false]);
  });

  it('counts changes only inside the window: an "added" day outside does not count', () => {
    expect(changeCounts(rows)).toEqual({ added: 1, replaced: 0, removed: 0 });
  });
});

describe('changeCounts', () => {
  it('counts added, replaced and removed apart; rest and unchanged do not count', () => {
    const rows = dayRows(preview([
      day('2026-10-02', 'rest'),
      day('2026-10-03', 'unchanged', [workout('2026-10-03')], [workout('2026-10-03')]),
      day('2026-10-04', 'added', [workout('2026-10-04')]),
      day('2026-10-05', 'replaced', [workout('2026-10-05')], [workout('2026-10-05')]),
      day('2026-10-06', 'removed', [], [workout('2026-10-06')]),
      day('2026-10-07', 'added', [workout('2026-10-07')]),
    ]), '2026-10-02');
    expect(changeCounts(rows)).toEqual({ added: 2, replaced: 1, removed: 1 });
  });
});

describe('issue location', () => {
  const issue = (patch: Partial<PlanIssue>): PlanIssue => ({ severity: 'error', message: '', message_code: 'x', params: {}, ...patch });

  it('issueDate: maps the workout index of the issue in the draft to its date; no workout gives no date', () => {
    const workouts = [workout('2026-10-03'), workout('2026-10-04')];
    expect(issueDate(issue({ workout: 1 }), workouts)).toBe('2026-10-04');
    expect(issueDate(issue({ workout: 5 }), workouts)).toBeNull();
    expect(issueDate(issue({}), workouts)).toBeNull();
    expect(issueDate(issue({ workout: 0 }), [{ date: 'Oct 3' }])).toBeNull();
  });

  it('issueStepPath: 2 gives [2], 2.1 gives [2, 1], no step gives []', () => {
    expect(issueStepPath(issue({ step: '2' }))).toEqual([2]);
    expect(issueStepPath(issue({ step: '2.1' }))).toEqual([2, 1]);
    expect(issueStepPath(issue({}))).toEqual([]);
  });
});
