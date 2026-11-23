import type { PlanDayChange, PlanDayPreview, PlanDraftPreview, PlanHeldWorkout, PlanIssue, PlanWorkout } from '../../types/trainingPlan';

export const WINDOW_DAYS = 7;

export interface DayRow {
  rest?: import("../../types/trainingPlan").PlanRestDay | null;
  date: string;
  change: PlanDayChange;
  before: PlanWorkout[];
  after: PlanWorkout[];
  held: PlanHeldWorkout[];
  inWindow: boolean;
  isToday: boolean;
}

const addDays = (date: string, days: number): string => {
  const [year, month, day] = date.split('-').map(Number);
  const moved = new Date(Date.UTC(year, month - 1, day + days));
  return moved.toISOString().slice(0, 10);
};

export const windowEnd = (start: string): string => addDays(start, WINDOW_DAYS - 1);

export const dayRows = (preview: PlanDraftPreview, today: string): DayRow[] => {
  const end = windowEnd(preview.window.start);
  return preview.days.map((day: PlanDayPreview) => ({
    date: day.date,
    rest: day.rest,
    change: day.change,
    before: day.before,
    after: day.after,
    held: preview.check.held?.filter((item) => item.date === day.date) ?? [],
    inWindow: day.date >= preview.window.start && day.date <= end,
    isToday: day.date === today,
  }));
};

export interface ChangeCounts { added: number; replaced: number; removed: number }

export const changeCounts = (rows: DayRow[]): ChangeCounts => {
  const counts: ChangeCounts = { added: 0, replaced: 0, removed: 0 };
  for (const row of rows) {
    if (!row.inWindow) continue;
    if (row.change === 'added') counts.added += 1;
    else if (row.change === 'replaced') counts.replaced += 1;
    else if (row.change === 'removed') counts.removed += 1;
  }
  return counts;
};

export const issueDate = (issue: PlanIssue, written: { date: string }[]): string | null => {
  if (issue.workout === undefined) return null;
  const date = written[issue.workout]?.date ?? null;
  return date && /^\d{4}-\d{2}-\d{2}$/.test(date) ? date : null;
};

export const issueStepPath = (issue: PlanIssue): number[] =>
  issue.step ? issue.step.split('.').map(Number).filter((n) => Number.isFinite(n)) : [];
