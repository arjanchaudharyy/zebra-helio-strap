import type { IconName } from '../../components/Icon.vue';
import type { AiTaskCategory, AiTaskCategoryRange } from '../bridge/types';
import { aiTaskTextFor } from './copy';

export const AI_TASK_CATEGORY_ORDER: readonly AiTaskCategory[] = [
  'workout',
  'sleep',
  'recovery',
  'resting_hr',
  'heart_rate',
  'training',
  'body',
  'personal_note',
  'attachment',
];

export interface AiTaskCategoryMeta {
  category: AiTaskCategory;
  labelCode: string;
  icon: IconName;
  tint: string;
  defaultDaysBefore: number;
  hasWindow: boolean;
}

export const AI_TASK_CATEGORY_META: Readonly<Record<AiTaskCategory, AiTaskCategoryMeta>> = {
  workout: { category: 'workout', labelCode: 'ui.ai_task.cat.workout', icon: 'run', tint: 'var(--activity)', defaultDaysBefore: 13, hasWindow: true },
  sleep: { category: 'sleep', labelCode: 'ui.ai_task.cat.sleep', icon: 'moon', tint: 'var(--sleep-light)', defaultDaysBefore: 13, hasWindow: true },
  recovery: { category: 'recovery', labelCode: 'ui.ai_task.cat.recovery', icon: 'spark', tint: 'var(--readiness)', defaultDaysBefore: 13, hasWindow: true },
  resting_hr: { category: 'resting_hr', labelCode: 'ui.ai_task.cat.resting_hr', icon: 'heart', tint: 'var(--heart)', defaultDaysBefore: 13, hasWindow: true },
  heart_rate: { category: 'heart_rate', labelCode: 'ui.ai_task.cat.heart_rate', icon: 'activity', tint: 'var(--heart)', defaultDaysBefore: 13, hasWindow: true },
  training: { category: 'training', labelCode: 'ui.ai_task.cat.training', icon: 'training-load', tint: 'var(--training)', defaultDaysBefore: 13, hasWindow: true },
  body: { category: 'body', labelCode: 'ui.ai_task.cat.body', icon: 'user', tint: 'var(--pace)', defaultDaysBefore: 13, hasWindow: true },
  personal_note: { category: 'personal_note', labelCode: 'ui.ai_task.cat.personal_note', icon: 'edit', tint: 'var(--muted)', defaultDaysBefore: 13, hasWindow: false },
  attachment: { category: 'attachment', labelCode: 'ui.ai_task.cat.attachment', icon: 'file', tint: 'var(--muted)', defaultDaysBefore: 13, hasWindow: false },
};

export const defaultCategoryRanges = (): AiTaskCategoryRange[] =>
  AI_TASK_CATEGORY_ORDER.map((category) => ({
    category,
    enabled: AI_TASK_CATEGORY_META[category].hasWindow,
    days_before: AI_TASK_CATEGORY_META[category].defaultDaysBefore,
    include_workout_day: true,
    excluded_metrics: [],
  }));

export const categoryLabel = (category: AiTaskCategory): string =>
  aiTaskTextFor(AI_TASK_CATEGORY_META[category].labelCode) ?? category;

export const CATEGORY_DAY_CHOICES: readonly number[] = [7, 14, 30];

export const shownDayChoice = (daysBefore: number, choices: readonly number[]): number => {
  if (choices.includes(daysBefore)) return daysBefore;
  if (choices.includes(daysBefore + 1)) return daysBefore + 1;
  return daysBefore;
};

export const categoryRangeOf = (
  categories: AiTaskCategoryRange[],
  category: AiTaskCategory,
): AiTaskCategoryRange =>
  categories.find((range) => range.category === category) ?? {
    category,
    enabled: AI_TASK_CATEGORY_META[category].hasWindow,
    days_before: AI_TASK_CATEGORY_META[category].defaultDaysBefore,
    include_workout_day: true,
    excluded_metrics: [],
  };

export const withCategoryRange = (
  categories: AiTaskCategoryRange[],
  next: AiTaskCategoryRange,
): AiTaskCategoryRange[] => {
  const index = categories.findIndex((range) => range.category === next.category);
  if (index < 0) return [...categories, next];
  const out = [...categories];
  out[index] = next;
  return out;
};
