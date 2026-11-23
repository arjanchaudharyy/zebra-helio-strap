import type { AiTaskCategoryRange } from '../bridge/types';

export const pickedDayCount = (categories: Pick<AiTaskCategoryRange, 'enabled' | 'picked_days'>[]): number =>
  new Set(categories.filter((range) => range.enabled).flatMap((range) => range.picked_days ?? [])).size;
