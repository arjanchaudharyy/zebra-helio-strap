import { computed } from 'vue';
import type { IconName } from '../../components/Icon.vue';
import { AI_TASK_CATEGORY_META, categoryLabel, categoryRangeOf } from '../../lib/aiTask/categories';
import { emptyCategories } from '../../lib/aiTask/emptyCategories';
import type { AiTask, AiTaskCategory, AiTaskCoverage, AiTaskPreview } from '../../lib/bridge/types';
import { useAiTaskDraft } from '../useAiTaskDraft';
import { useAiTaskPreview } from '../useAiTaskPreview';
import { useBridgeStrip } from '../useBridgeStrip';
import { useStageText } from '../../components/ai/stage/stage.i18n';
import { useBridgeText } from '../../components/ai/bridge/bridge.i18n';
import { defineMessages, useMessages } from '../../i18n';
import { formatDate, formatDistance } from '../../lib/format';
import { workoutDisplayLabel } from '../../lib/workouts';
import { pageQuery } from '../../lib/pageQueries';
import { peek } from '../../lib/readCache';
import { useAiDerived } from './useAiHub';
import type { Workout } from '../../types';

const anchorMessages = defineMessages(
  { thisWorkout: 'This workout' },
);

export const STAGE_DATA_ORDER: readonly AiTaskCategory[] = ['sleep', 'recovery', 'resting_hr', 'heart_rate', 'workout', 'training', 'body'];

export interface StageCardModel {
  id: string;
  kind: 'data' | 'profile';
  category: AiTaskCategory | null;
  icon: IconName;
  tint: string;
  title: string;
  line: string;
  have: number | null;
  total: number | null;
  sub: string | null;
}

const coverageOf = (rows: AiTaskCoverage[], category: AiTaskCategory) => {
  const mine = rows.filter((row) => row.category === category);
  return mine.find((row) => row.workout_id === null) ?? mine[0] ?? null;
};

const rangeKey = (task: AiTask, category: AiTaskCategory) => {
  const range = categoryRangeOf(task.categories, category);
  return JSON.stringify([range.enabled, range.days_before, range.include_workout_day, [...(range.picked_days ?? [])].sort(), [...task.workout_ids].sort()]);
};

export const freshCoverage = (preview: AiTaskPreview | null, computedFor: AiTask | null, draft: AiTask, category: AiTaskCategory) => {
  if (!preview || !computedFor || rangeKey(computedFor, category) !== rangeKey(draft, category)) return null;
  return coverageOf(preview.coverage, category);
};

export const useStageCards = () => {
  const { draft } = useAiTaskDraft();
  const { preview, previewTask } = useAiTaskPreview();
  const strips = useBridgeStrip();
  const s = useStageText();
  const b = useBridgeText();
  const empty = computed(() => emptyCategories(strips.rows.value));
  const anchor = useMessages(anchorMessages);
  const { picked: pickedWorkouts } = useAiDerived();

  const soloWorkout = (): Workout | null => {
    const range = categoryRangeOf(draft.value.categories, 'workout');
    const [id, ...rest] = draft.value.workout_ids;
    if (!id || rest.length || range.days_before !== 0 || range.picked_days?.length) return null;
    return pickedWorkouts.value.find((w) => w.workout_id === id) ?? peek(pageQuery.workoutDetail(id))?.value ?? null;
  };

  const dataCard = (category: AiTaskCategory): StageCardModel => {
    const meta = AI_TASK_CATEGORY_META[category];
    const range = categoryRangeOf(draft.value.categories, category);
    const row = freshCoverage(preview.value, previewTask.value, draft.value, category);
    const picked = range.picked_days?.length ?? 0;
    const solo = category === 'workout' ? soloWorkout() : null;
    if (solo) {
      return {
        id: category, kind: 'data', category, icon: meta.icon, tint: meta.tint, title: anchor.value.thisWorkout,
        line: formatDate(solo.start_time),
        have: row ? row.days_with_data : null,
        total: row ? row.days_in_range : null,
        sub: [workoutDisplayLabel(solo), formatDistance(solo.distance_meters, '')].filter(Boolean).join(' '),
      };
    }
    const extra = category === 'workout' ? draft.value.workout_ids.length : 0;
    return {
      id: category, kind: 'data', category, icon: meta.icon, tint: meta.tint, title: categoryLabel(category),
      line: picked ? s.value.picked(picked) : s.value.recent(range.days_before + 1),
      have: row ? row.days_with_data : null,
      total: row ? row.days_in_range : null,
      sub: extra ? s.value.extraWorkouts(extra) : row && !row.days_with_data ? s.value.noRecord : null,
    };
  };

  const enabled = (category: AiTaskCategory) => categoryRangeOf(draft.value.categories, category).enabled;
  const off = computed(() => STAGE_DATA_ORDER.filter((category) => !enabled(category)));

  const cards = computed<StageCardModel[]>(() => {
    const out = STAGE_DATA_ORDER.filter(enabled).map(dataCard);
    const note = draft.value.personal_note.trim();
    out.push({
      id: 'personal_note', kind: 'profile', category: 'personal_note', icon: 'user', tint: 'var(--muted)', title: b.value.profile,
      line: note ? s.value.profileFilled : s.value.profileEmpty, have: null, total: null, sub: note ? note.slice(0, 28) : null,
    });
    return out;
  });

  return { cards, off, empty, dataCard };
};
