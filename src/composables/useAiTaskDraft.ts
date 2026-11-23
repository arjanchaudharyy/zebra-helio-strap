import { computed, ref } from 'vue';
import { backend, toUserMessage } from '../lib/bridge';
import type {
  AiTask,
  AiTaskAttachmentRef,
  AiTaskCategory,
  AiTaskCategoryRange,
  AiTaskDetailLevel,
  AiTaskTemplate,
} from '../lib/bridge/types';
import {
  applyTemplateToDraft,
  dropLegacyMetricExclusions,
  isTaskDirty,
  newTaskDraft,
  taskSnapshot,
} from '../lib/aiTask/draft';
import { AI_TASK_CATEGORY_META, categoryRangeOf, withCategoryRange } from '../lib/aiTask/categories';
import { createUndoStack } from '../lib/aiTask/undoStack';
import { useAiTaskLibrary } from './useAiTaskLibrary';
import { defineMessages, messagesOf } from '../i18n';

const messages = defineMessages(
  { loadFailed: 'Could not load task', saveFailed: 'Could not save task', deleteFailed: 'Could not delete task', untitled: 'Untitled task' },
);
const copy = () => messagesOf(messages);

type Selection = Pick<AiTask, 'categories' | 'workout_ids' | 'template_id' | 'detail_level'>;

export interface DraftChange {
  seq: number;
  kind: 'category' | 'workout' | 'template';
  category?: AiTaskCategory;
  workoutId?: string;
  included: boolean;
}

const library = useAiTaskLibrary();
const draft = ref<AiTask>(newTaskDraft());
const baseline = ref(taskSnapshot(draft.value));
const undoStack = createUndoStack<Selection>();
const undoDepth = ref(0);
const lastChange = ref<DraftChange | null>(null);
let changeSeq = 0;
const noteChange = (change: Omit<DraftChange, 'seq'>) => {
  changeSeq += 1;
  lastChange.value = { ...change, seq: changeSeq };
};
const busy = ref<false | 'load' | 'save' | 'delete'>(false);
const lastError = ref<string | null>(null);
const savedNotice = ref(false);
const legacyNotice = ref(false);
let savedNoticeTimer: ReturnType<typeof setTimeout> | undefined;

const dirty = computed(() => isTaskDirty(draft.value, baseline.value));
const canUndo = computed(() => undoDepth.value > 0);

const cloneRanges = (ranges: AiTaskCategoryRange[]) =>
  ranges.map((range) => ({ ...range, excluded_metrics: [...(range.excluded_metrics ?? [])] }));

const rememberSelection = () => {
  const { categories, workout_ids, template_id, detail_level } = draft.value;
  undoStack.push({ categories: cloneRanges(categories), workout_ids: [...workout_ids], template_id, detail_level });
  undoDepth.value = undoStack.size;
};

const clearUndo = () => {
  undoStack.clear();
  undoDepth.value = 0;
  lastChange.value = null;
};

const markBaseline = () => {
  baseline.value = taskSnapshot(draft.value);
};

const patchDraft = (patch: Partial<AiTask>) => {
  draft.value = { ...draft.value, ...patch };
};

const patchRange = (category: AiTaskCategory, patch: Partial<AiTaskCategoryRange>) => {
  const next = { ...categoryRangeOf(draft.value.categories, category), ...patch };
  patchDraft({ categories: withCategoryRange(draft.value.categories, next) });
};

let beforeTemplate: Selection | null = null;

const replaceDraft = (task: AiTask) => {
  beforeTemplate = null;
  draft.value = task;
  clearUndo();
  markBaseline();
  savedNotice.value = false;
};

let draftGen = 0;
let busyOwner = 0;
let opSeq = 0;
let saveChain: Promise<unknown> = Promise.resolve();
let savesInFlight = 0;

const beginBusy = (kind: 'load' | 'save' | 'delete') => {
  opSeq += 1;
  busyOwner = opSeq;
  busy.value = kind;
  return opSeq;
};
const endBusy = (op: number) => {
  if (busyOwner === op) busy.value = false;
};

const loadTask = async (id: string) => {
  const op = beginBusy('load');
  draftGen += 1;
  const gen = draftGen;
  lastError.value = null;
  try {
    const task = await backend.aiTaskGet(id);
    if (gen !== draftGen) return;
    const cleaned = dropLegacyMetricExclusions(task);
    legacyNotice.value = cleaned.droppedCount > 0;
    replaceDraft(cleaned.task);
    await library.ensureWorkouts(task.workout_ids);
  } catch (error) {
    if (gen === draftGen) lastError.value = toUserMessage(error, copy().loadFailed);
    throw error;
  } finally {
    endBusy(op);
  }
};

let emptyCategories = new Set<AiTaskCategory>();
let emptyAppliedGen = -1;
const applyEmptyDefaults = () => {
  if (draft.value.id || dirty.value || emptyAppliedGen === draftGen || !emptyCategories.size) return;
  emptyAppliedGen = draftGen;
  if (!draft.value.categories.some((range) => range.enabled && emptyCategories.has(range.category))) return;
  draft.value = { ...draft.value, categories: draft.value.categories.map((range) => (emptyCategories.has(range.category) ? { ...range, enabled: false } : range)) };
  markBaseline();
};
const setEmptyCategories = (next: Set<AiTaskCategory>) => {
  emptyCategories = next;
  applyEmptyDefaults();
};

const resetDraft = () => {
  draftGen += 1;
  replaceDraft(newTaskDraft());
  applyEmptyDefaults();
  lastError.value = null;
  legacyNotice.value = false;
  const gen = draftGen;
  void backend.getUserPrefs().then(prefs => {
    if (gen === draftGen && !draft.value.personal_note) setPersonalNote(prefs.ai_profile_note ?? '');
  }).catch(() => undefined);
};

const showSavedNotice = () => {
  savedNotice.value = true;
  clearTimeout(savedNoticeTimer);
  savedNoticeTimer = setTimeout(() => {
    savedNotice.value = false;
  }, 4000);
};

const saveDraft = (fallbackTitle?: string): Promise<AiTask> => {
  const run = savesInFlight === 0
    ? saveNow(fallbackTitle)
    : saveChain.then(() => saveNow(fallbackTitle));
  savesInFlight += 1;
  const settled = run.catch(() => undefined).finally(() => {
    savesInFlight -= 1;
  });
  saveChain = settled;
  return run;
};

const saveNow = async (fallbackTitle?: string): Promise<AiTask> => {
  const op = beginBusy('save');
  const gen = draftGen;
  const sentSnapshot = taskSnapshot(draft.value);
  lastError.value = null;
  try {
    const task = { ...draft.value };
    if (!task.title.trim()) task.title = fallbackTitle?.trim() || copy().untitled;
    const saved = await backend.aiTaskSave(task);
    if (gen === draftGen) {
      if (taskSnapshot(draft.value) === sentSnapshot) {
        draft.value = saved;
      } else {
        draft.value = { ...draft.value, id: saved.id, created_at: saved.created_at, updated_at: saved.updated_at };
      }
      baseline.value = taskSnapshot(saved);
      showSavedNotice();
    }
    void library.loadTaskList();
    return saved;
  } catch (error) {
    if (gen === draftGen) lastError.value = toUserMessage(error, copy().saveFailed);
    throw error;
  } finally {
    endBusy(op);
  }
};

const deleteTask = async (id: string) => {
  const op = beginBusy('delete');
  lastError.value = null;
  try {
    await backend.aiTaskDelete(id);
    if (draft.value.id === id) resetDraft();
    await library.loadTaskList();
  } catch (error) {
    lastError.value = toUserMessage(error, copy().deleteFailed);
    throw error;
  } finally {
    endBusy(op);
  }
};

const setTemplate = (template: AiTaskTemplate | null) => {
  if ((template?.id ?? null) === draft.value.template_id) return;
  rememberSelection();
  if (template && draft.value.template_id === null) {
    const { categories, workout_ids, template_id, detail_level } = draft.value;
    beforeTemplate = { categories: cloneRanges(categories), workout_ids: [...workout_ids], template_id, detail_level };
  }
  if (!template) beforeTemplate = null;
  draft.value = template ? applyTemplateToDraft(draft.value, template) : { ...draft.value, template_id: null };
  noteChange({ kind: 'template', included: template !== null });
};

const setCategoryEnabled = (category: AiTaskCategory, enabled: boolean) => {
  if (categoryRangeOf(draft.value.categories, category).enabled === enabled) return;
  rememberSelection();
  patchRange(category, { enabled });
  noteChange({ kind: 'category', category, included: enabled });
};

const setWindowDays = (days: number) => {
  const next = Math.max(0, Math.floor(days));
  const windows = draft.value.categories.filter((range) => AI_TASK_CATEGORY_META[range.category].hasWindow);
  if (windows.every((range) => range.days_before === next && !range.picked_days?.length)) return;
  rememberSelection();
  patchDraft({
    categories: draft.value.categories.map((range) => (
      AI_TASK_CATEGORY_META[range.category].hasWindow ? { ...range, days_before: next, picked_days: [] } : range
    )),
  });
};

const applyRangeToAll = (category: AiTaskCategory) => {
  const source = categoryRangeOf(draft.value.categories, category);
  const picked = [...(source.picked_days ?? [])];
  const same = (range: AiTaskCategoryRange) => range.days_before === source.days_before
    && (range.picked_days ?? []).join() === picked.join();
  const windows = draft.value.categories.filter((range) => AI_TASK_CATEGORY_META[range.category].hasWindow);
  if (windows.every(same)) return;
  rememberSelection();
  patchDraft({
    categories: draft.value.categories.map((range) => (
      AI_TASK_CATEGORY_META[range.category].hasWindow ? { ...range, days_before: source.days_before, picked_days: [...picked] } : range
    )),
  });
};

const applyPicks = (byCategory: Map<AiTaskCategory, string[]>, today: string, workoutIds: string[] = []) => {
  rememberSelection();
  patchDraft({
    ...(workoutIds.length ? { workout_ids: [...new Set(workoutIds)] } : {}),
    categories: draft.value.categories.map((range) => {
      if (!AI_TASK_CATEGORY_META[range.category].hasWindow) return range;
      const days = [...new Set(byCategory.get(range.category) ?? [])].sort();
      if (!days.length) return { ...range, enabled: false, picked_days: [] };
      const span = Math.max(0, Math.round((Date.parse(`${today}T00:00:00Z`) - Date.parse(`${days[0]}T00:00:00Z`)) / 86_400_000));
      return { ...range, enabled: true, days_before: span, picked_days: days };
    }),
  });
};

const mergePicks = (byCategory: Map<AiTaskCategory, string[]>, today: string, workoutIds: string[] = []) => {
  if (!byCategory.size && !workoutIds.length) return;
  rememberSelection();
  patchDraft({
    ...(workoutIds.length ? { workout_ids: [...new Set([...draft.value.workout_ids, ...workoutIds])] } : {}),
    categories: draft.value.categories.map((range) => {
      const fresh = byCategory.get(range.category);
      if (!fresh?.length || !AI_TASK_CATEGORY_META[range.category].hasWindow) return range;
      const days = [...new Set([...(range.picked_days ?? []), ...fresh])].sort();
      const span = Math.max(0, Math.round((Date.parse(`${today}T00:00:00Z`) - Date.parse(`${days[0]}T00:00:00Z`)) / 86_400_000));
      return { ...range, enabled: true, days_before: span, picked_days: days };
    }),
  });
  if (workoutIds.length) patchRange('workout', { enabled: true });
};

const restoreAllData = () => {
  const fresh = newTaskDraft();
  const byCategory = new Map(fresh.categories.map((range) => [range.category, range]));
  rememberSelection();
  beforeTemplate = null;
  patchDraft({
    template_id: fresh.template_id,
    detail_level: fresh.detail_level,
    workout_ids: [],
    categories: draft.value.categories.map((range) => {
      const base = byCategory.get(range.category);
      if (!AI_TASK_CATEGORY_META[range.category].hasWindow || !base) return range;
      return { ...range, enabled: !emptyCategories.has(range.category), days_before: base.days_before, picked_days: [], excluded_metrics: [] };
    }),
  });
};
const atInitial = computed(() => {
  const fresh = newTaskDraft();
  const byCategory = new Map(fresh.categories.map((range) => [range.category, range]));
  return !draft.value.workout_ids.length && draft.value.template_id === fresh.template_id && draft.value.categories.every((range) => {
    const base = byCategory.get(range.category);
    if (!AI_TASK_CATEGORY_META[range.category].hasWindow || !base) return true;
    return range.enabled === !emptyCategories.has(range.category) && range.days_before === base.days_before && !range.picked_days?.length;
  });
});

const setWorkoutSelected = (workoutId: string, selected: boolean) => {
  const ids = draft.value.workout_ids;
  if (ids.includes(workoutId) === selected) return;
  rememberSelection();
  patchDraft({ workout_ids: selected ? [...ids, workoutId] : ids.filter((id) => id !== workoutId) });
  noteChange({ kind: 'workout', workoutId, included: selected });
};

const setPrompt = (prompt: string) => {
  patchDraft({ prompt });
  if (prompt.trim() || draft.value.template_id === null || !beforeTemplate) return;
  const restore = beforeTemplate;
  beforeTemplate = null;
  rememberSelection();
  patchDraft({ ...restore, categories: cloneRanges(restore.categories), workout_ids: [...restore.workout_ids], template_id: null });
  noteChange({ kind: 'template', included: false });
};

const undo = () => {
  const previous = undoStack.pop();
  undoDepth.value = undoStack.size;
  lastChange.value = null;
  if (previous) patchDraft(previous);
};

const setPersonalNote = (text: string) => {
  patchDraft({ personal_note: text });
  if (text.trim()) patchRange('personal_note', { enabled: true });
};

const addAttachments = (refs: AiTaskAttachmentRef[]) => {
  if (!refs.length) return;
  patchDraft({ attachments: [...draft.value.attachments, ...refs] });
  patchRange('attachment', { enabled: true });
};

const removeAttachment = (id: string) => {
  const attachments = draft.value.attachments.filter((ref) => ref.id !== id);
  patchDraft({ attachments });
  if (!attachments.length) patchRange('attachment', { enabled: false });
};

const replaceAttachment = (id: string, next: AiTaskAttachmentRef) => {
  patchDraft({ attachments: draft.value.attachments.map((ref) => (ref.id === id ? { ...next, id } : ref)) });
};

export function useAiTaskDraft() {
  return {
    draft,
    dirty,
    canUndo,
    lastChange,
    busy,
    lastError,
    savedNotice,
    loadTask,
    resetDraft,
    restoreAllData,
    atInitial,
    setEmptyCategories,
    saveDraft,
    deleteTask,
    setTemplate,
    setCategoryEnabled,
    setCategoryDays: (category: AiTaskCategory, days: number) => {
      const next = Math.max(0, Math.floor(days));
      const range = categoryRangeOf(draft.value.categories, category);
      if (range.days_before === next && !range.picked_days?.length) return;
      rememberSelection();
      patchRange(category, { days_before: next, picked_days: [] });
    },
    setWindowDays,
    applyRangeToAll,
    applyPicks,
    mergePicks,
    setIncludeWorkoutDay: (category: AiTaskCategory, include: boolean) =>
      patchRange(category, { include_workout_day: include }),
    legacyNotice,
    dismissLegacyNotice: () => { legacyNotice.value = false; },
    setWorkoutSelected,
    toggleWorkout: (workoutId: string) => setWorkoutSelected(workoutId, !draft.value.workout_ids.includes(workoutId)),
    undo,
    setPrompt,
    setPersonalNote,
    setTitle: (title: string) => patchDraft({ title }),
    setDetailLevel: (detail_level: AiTaskDetailLevel) => patchDraft({ detail_level }),
    setPreciseGps: (include_precise_gps: boolean) => patchDraft({ include_precise_gps }),
    setMcpShared: (mcp_shared: boolean) => patchDraft({ mcp_shared }),
    addAttachments,
    removeAttachment,
    replaceAttachment,
  };
}
