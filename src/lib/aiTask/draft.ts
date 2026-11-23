import type {
  AiTask,
  AiTaskTemplate,
} from '../bridge/types';
import { defaultCategoryRanges } from './categories';

export const AI_TASK_SCHEMA_VERSION = 1 as const;
export const AI_TASK_PROMPT_MAX = 500;

export const newTaskDraft = (): AiTask => ({
  schema_version: AI_TASK_SCHEMA_VERSION,
  id: '',
  title: '',
  template_id: null,
  workout_ids: [],
  categories: defaultCategoryRanges(),
  detail_level: 'standard',
  prompt: '',
  personal_note: '',
  attachments: [],
  include_precise_gps: false,
  mcp_shared: false,
  created_at: '',
  updated_at: '',
});

export const applyTemplateToDraft = (task: AiTask, template: AiTaskTemplate): AiTask => {
  const contentEnabled = new Map(
    task.categories
      .filter((range) => range.category === 'personal_note' || range.category === 'attachment')
      .map((range) => [range.category, range.enabled]),
  );
  return {
    ...task,
    template_id: template.id,
    categories: template.categories.map((range) => ({
      ...range,
      excluded_metrics: [...(range.excluded_metrics ?? [])],
      enabled: contentEnabled.get(range.category) ?? range.enabled,
    })),
    detail_level: template.detail_level,
  };
};

export const dropLegacyMetricExclusions = (task: AiTask): { task: AiTask; droppedCount: number } => {
  let droppedCount = 0;
  const categories = task.categories.map((range) => {
    const excluded = range.excluded_metrics ?? [];
    if (!excluded.length) return range;
    droppedCount += excluded.length;
    return { ...range, excluded_metrics: [] };
  });
  return droppedCount ? { task: { ...task, categories }, droppedCount } : { task, droppedCount };
};

export const taskSnapshot = (task: AiTask): string =>
  JSON.stringify({
    title: task.title,
    template_id: task.template_id,
    workout_ids: [...task.workout_ids].sort(),
    categories: [...task.categories]
      .map((range) => ({
        category: range.category,
        enabled: range.enabled,
        days_before: range.days_before,
        include_workout_day: range.include_workout_day,
        excluded_metrics: [...(range.excluded_metrics ?? [])].sort(),
        ...(range.picked_days?.length ? { picked_days: [...range.picked_days].sort() } : {}),
      }))
      .sort((a, b) => a.category.localeCompare(b.category)),
    detail_level: task.detail_level,
    prompt: task.prompt,
    personal_note: task.personal_note,
    attachments: task.attachments
      .map((ref) => {
        const { display_name, ...rest } = ref;
        return {
          id: rest.id,
          path: rest.path,
          display_name,
          kind: rest.kind,
          byte_len: rest.byte_len,
          added_at: rest.added_at,
        };
      })
      .sort((a, b) => a.id.localeCompare(b.id)),
    include_precise_gps: task.include_precise_gps,
    mcp_shared: task.mcp_shared,
  });

export const isTaskDirty = (task: AiTask, baselineSnapshot: string): boolean =>
  taskSnapshot(task) !== baselineSnapshot;
