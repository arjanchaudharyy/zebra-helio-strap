import { ref } from 'vue';
import { backend, toUserMessage } from '../lib/bridge';
import type { AiTaskSummary, AiTaskTemplate } from '../lib/bridge/types';
import type { Workout } from '../types';
import { defineMessages, messagesOf } from '../i18n';

const messages = defineMessages(
  { loadFailed: 'Could not load task data' },
);

const taskList = ref<AiTaskSummary[]>([]);
const templates = ref<AiTaskTemplate[]>([]);
const recentWorkouts = ref<Workout[]>([]);
const libraryError = ref<string | null>(null);
let pinnedIds: string[] = [];

const fail = (error: unknown) => {
  libraryError.value = toUserMessage(error, messagesOf(messages).loadFailed);
};

const loadTaskList = async () => {
  try {
    taskList.value = await backend.aiTaskList();
  } catch (error) {
    fail(error);
  }
};

const loadTemplates = async () => {
  try {
    templates.value = await backend.aiTemplateList();
  } catch (error) {
    fail(error);
  }
};

const ensureWorkouts = async (ids: string[]) => {
  pinnedIds = [...ids];
  const known = new Set(recentWorkouts.value.map((workout) => workout.workout_id));
  const missing = ids.filter((id) => !known.has(id));
  if (!missing.length) return;
  const fetched = await Promise.all(missing.map((id) => backend.getWorkoutDetail(id).catch(() => null)));
  const have = new Set(recentWorkouts.value.map((workout) => workout.workout_id));
  const extra = fetched.filter((workout): workout is Workout => workout !== null && !have.has(workout.workout_id));
  if (extra.length) recentWorkouts.value = [...recentWorkouts.value, ...extra];
};

const loadRecentWorkouts = async (limit = 60) => {
  try {
    recentWorkouts.value = await backend.getRecentWorkouts(limit);
    await ensureWorkouts(pinnedIds);
  } catch (error) {
    fail(error);
  }
};

export function useAiTaskLibrary() {
  return {
    taskList,
    templates,
    recentWorkouts,
    libraryError,
    loadTaskList,
    loadTemplates,
    loadRecentWorkouts,
    ensureWorkouts,
  };
}
