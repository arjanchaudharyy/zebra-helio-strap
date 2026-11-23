import type { AiTask } from '../bridge/types';
import type { Workout } from '../../types';
import { defineMessages, messagesOf } from '../../i18n';
import { formatDate } from '../format';
import { workoutDisplayLabel } from '../workouts';
import { AI_TASK_CATEGORY_META } from './categories';
import { pickedDayCount } from './pickedDays';

const messages = defineMessages(
  { recentDays: (days: number) => `Last ${days} days`, pickedDays: (days: number) => `${days} picked ${days === 1 ? 'day' : 'days'}`, andMore: (count: number) => ` +${count - 1} more` },
);

export const recentWindowDays = (task: Pick<AiTask, 'categories'>): number =>
  task.categories
    .filter((range) => range.enabled && AI_TASK_CATEGORY_META[range.category].hasWindow)
    .reduce((max, range) => Math.max(max, range.days_before + (range.include_workout_day ? 1 : 0)), 0) || 14;

export const autoTaskTitle = (
  task: Pick<AiTask, 'categories'>,
  selectedWorkouts: Workout[],
  templateLabel: string | null,
  now: Date = new Date(),
): string => {
  const t = messagesOf(messages);
  const [first] = [...selectedWorkouts].sort((a, b) => a.start_time.localeCompare(b.start_time));
  const picked = pickedDayCount(task.categories);
  const subject = first
    ? `${workoutDisplayLabel(first)}${selectedWorkouts.length > 1 ? t.andMore(selectedWorkouts.length) : ''}`
    : picked ? t.pickedDays(picked) : t.recentDays(recentWindowDays(task));
  const head = templateLabel ?? subject;
  const date = formatDate(first ? first.start_time : now.toISOString());
  return `${head} · ${date}`;
};
