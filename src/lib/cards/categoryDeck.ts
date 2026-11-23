import { AI_TASK_CATEGORY_META, categoryLabel } from '../aiTask/categories';
import { unitLabel } from '../aiTask/metrics';
import { formatDuration } from '../format';
import type { AiTaskCategory } from '../bridge/types';
import { categorySource, workoutSource, type DeckSource } from './sources';

const formatFor = (category: AiTaskCategory, unit: string | null) => (value: number) => (category === 'sleep'
  ? formatDuration(value)
  : `${value.toFixed(unit === 'kg' || unit === 'h' ? 1 : 0)}`);

export const categoryDeckSource = (category: AiTaskCategory, options: {
  unit: string | null;
  pickedWorkouts: () => string[];
  releaseWorkout: (id: string) => void;
}): DeckSource => {
  const tint = AI_TASK_CATEGORY_META[category].tint;
  const label = categoryLabel(category);
  if (category === 'workout') return workoutSource({ label, tint, picked: options.pickedWorkouts, release: options.releaseWorkout });
  return categorySource({
    category, label, tint, format: formatFor(category, options.unit),
    unit: category === 'sleep' || !options.unit ? undefined : unitLabel(options.unit),
  });
};
