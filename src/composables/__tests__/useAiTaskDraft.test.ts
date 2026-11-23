import { beforeEach, describe, expect, it, vi } from 'vitest';
import { newTaskDraft } from '../../lib/aiTask/draft';
import type { AiTask, AiTaskTemplate } from '../../lib/bridge/types';
import type { Workout } from '../../types';

const listMock = vi.fn(async () => []);
const getMock = vi.fn();
const saveMock = vi.fn();
const workoutsMock = vi.fn(async () => []);
const workoutDetailMock = vi.fn(async (_id: string): Promise<Workout | null> => null);
const templatesMock = vi.fn(async (): Promise<AiTaskTemplate[]> => []);

vi.mock('../../lib/bridge', () => ({
  backend: {
    getUserPrefs: async () => ({ ai_profile_note: '' }),
    aiTaskList: () => listMock(),
    aiTaskGet: (id: string) => getMock(id),
    aiTaskSave: (task: AiTask) => saveMock(task),
    aiTaskDelete: vi.fn(async () => {}),
    aiTemplateList: () => templatesMock(),
    getRecentWorkouts: () => workoutsMock(),
    getWorkoutDetail: (id: string) => workoutDetailMock(id),
  },
  toUserMessage: (_error: unknown, fallback: string) => fallback,
}));

import { useAiTaskDraft } from '../useAiTaskDraft';
import { useAiTaskLibrary } from '../useAiTaskLibrary';

const template = (patch: Partial<AiTaskTemplate> = {}): AiTaskTemplate => ({
  schema_version: 1,
  id: 'tpl-1',
  builtin: true,
  name: 'T',
  name_code: null,
  categories: [],
  detail_level: 'detailed',
  prompt_template: 'seed',
  prompt_code: null,
  default_summaries: [],
  required_series: [],
  created_at: '',
  updated_at: '',
  ...patch,
});

describe('useAiTaskDraft', () => {
  const draft = useAiTaskDraft();
  const library = useAiTaskLibrary();

  beforeEach(() => {
    draft.resetDraft();
    listMock.mockClear();
    getMock.mockReset();
    saveMock.mockReset();
    workoutsMock.mockReset();
    workoutsMock.mockResolvedValue([]);
    workoutDetailMock.mockReset();
    workoutDetailMock.mockResolvedValue(null);
    templatesMock.mockReset();
    templatesMock.mockResolvedValue([]);
  });

  it('category join and leave go on the undo stack; undo restores the state before the action', () => {
    draft.setCategoryEnabled('sleep', false);
    expect(draft.canUndo.value).toBe(true);
    draft.undo();
    expect(draft.draft.value.categories.find((r) => r.category === 'sleep')?.enabled).toBe(true);
    expect(draft.canUndo.value).toBe(false);
  });

  it('join, then leave, then undo returns to member', () => {
    draft.setCategoryEnabled('body', false);
    draft.setCategoryEnabled('body', true);
    draft.undo();
    expect(draft.draft.value.categories.find((r) => r.category === 'body')?.enabled).toBe(false);
  });

  it('toggleWorkout adds and removes on each call', () => {
    draft.toggleWorkout('w-1');
    expect(draft.draft.value.workout_ids).toEqual(['w-1']);
    draft.toggleWorkout('w-1');
    expect(draft.draft.value.workout_ids).toEqual([]);
  });

  it('drag-and-drop workout selection is idempotent and undoable step by step', () => {
    draft.setWorkoutSelected('w-1', true);
    draft.setWorkoutSelected('w-1', true);
    expect(draft.draft.value.workout_ids).toEqual(['w-1']);
    draft.setWorkoutSelected('w-1', false);
    expect(draft.draft.value.workout_ids).toEqual([]);
    draft.undo();
    expect(draft.draft.value.workout_ids).toEqual(['w-1']);
    draft.undo();
    expect(draft.draft.value.workout_ids).toEqual([]);
  });

  it('applying a template sets only the focus and suggested ranges, keeps the user question, and can be undone', () => {
    draft.setPrompt('my own words');
    draft.setTemplate(template());
    expect(draft.draft.value.prompt).toBe('my own words');
    expect(draft.draft.value.template_id).toBe('tpl-1');
    expect(draft.draft.value.detail_level).toBe('detailed');
    draft.undo();
    expect(draft.draft.value.template_id).toBeNull();
    expect(draft.draft.value.detail_level).toBe('standard');
  });

  it('cleared question: everything returns to the state before the template (categories, days, template); text keeps it', () => {
    const days = draft.draft.value.categories.map((range) => range.days_before);
    draft.setTemplate(template({ detail_level: 'detailed' }));
    draft.setWindowDays(29);
    draft.setPrompt('plan next week');
    draft.setPrompt('plan next');
    expect(draft.draft.value.template_id).toBe('tpl-1');
    draft.setPrompt('   ');
    expect(draft.draft.value.template_id).toBeNull();
    expect(draft.draft.value.detail_level).toBe('standard');
    expect(draft.draft.value.categories.map((range) => range.days_before)).toEqual(days);
    draft.undo();
    expect(draft.draft.value.template_id).toBe('tpl-1');
  });

  it('loading a task with old metric exclusions: exclusions dropped, notice set, not dirty', async () => {
    const stored = newTaskDraft();
    stored.id = 't-legacy';
    stored.categories = stored.categories.map((range) =>
      range.category === 'recovery' ? { ...range, excluded_metrics: ['stress'] } : range);
    getMock.mockResolvedValue(stored);
    await draft.loadTask('t-legacy');
    expect(draft.draft.value.categories.find((r) => r.category === 'recovery')?.excluded_metrics).toEqual([]);
    expect(draft.legacyNotice.value).toBe(true);
    expect(draft.dirty.value).toBe(false);
    draft.dismissLegacyNotice();
    expect(draft.legacyNotice.value).toBe(false);
  });

  it('a clean task shows no notice; loading a clean task again clears the notice', async () => {
    const legacy = newTaskDraft();
    legacy.id = 't-old';
    legacy.categories = legacy.categories.map((range) =>
      range.category === 'sleep' ? { ...range, excluded_metrics: ['rem_minutes'] } : range);
    getMock.mockResolvedValue(legacy);
    await draft.loadTask('t-old');
    expect(draft.legacyNotice.value).toBe(true);
    getMock.mockResolvedValue({ ...newTaskDraft(), id: 't-clean' });
    await draft.loadTask('t-clean');
    expect(draft.legacyNotice.value).toBe(false);
  });

  it('writing a personal note adds the personal_note category to the task', () => {
    expect(draft.draft.value.categories.find((r) => r.category === 'personal_note')?.enabled).toBe(false);
    draft.setPersonalNote('context');
    expect(draft.draft.value.categories.find((r) => r.category === 'personal_note')?.enabled).toBe(true);
  });

  it('adding an attachment turns on the attachment category; removing all turns it off', () => {
    draft.addAttachments([{
      id: 'a-1', path: 'C:\\x\\f.pdf', display_name: 'f.pdf',
      kind: 'pdf', byte_len: 1, added_at: 't',
    }]);
    expect(draft.draft.value.categories.find((r) => r.category === 'attachment')?.enabled).toBe(true);
    draft.removeAttachment('a-1');
    expect(draft.draft.value.categories.find((r) => r.category === 'attachment')?.enabled).toBe(false);
  });

  it('replaceAttachment keeps the id and changes the path', () => {
    draft.addAttachments([{
      id: 'a-1', path: 'C:\\x\\old.pdf', display_name: 'old.pdf',
      kind: 'pdf', byte_len: 1, added_at: 't',
    }]);
    draft.replaceAttachment('a-1', {
      id: 'other', path: 'C:\\x\\new.pdf', display_name: 'new.pdf',
      kind: 'pdf', byte_len: 2, added_at: 't2',
    });
    const att = draft.draft.value.attachments[0];
    expect(att.id).toBe('a-1');
    expect(att.path).toBe('C:\\x\\new.pdf');
  });

  it('saveDraft: an empty title gets a default name; success clears dirty and stores the id', async () => {
    saveMock.mockImplementation(async (task: AiTask) => ({
      ...task, id: 'server-id', created_at: 'c', updated_at: 'u',
    }));
    draft.setTitle('   ');
    const saved = await draft.saveDraft();
    expect(saved.id).toBe('server-id');
    expect(draft.draft.value.id).toBe('server-id');
    expect(draft.dirty.value).toBe(false);
    expect(listMock).toHaveBeenCalled();
  });

  it('loadTask loads the backend task and clears undo', async () => {
    const stored = { ...newTaskDraft(), id: 't-9', title: 'saved task' };
    getMock.mockResolvedValue(stored);
    draft.setCategoryEnabled('sleep', false);
    await draft.loadTask('t-9');
    expect(draft.draft.value.title).toBe('saved task');
    expect(draft.canUndo.value).toBe(false);
    expect(draft.dirty.value).toBe(false);
  });

  it('loadTask failure passes the error through, lastError gets a fallback message', async () => {
    getMock.mockRejectedValue(new Error('gone'));
    await expect(draft.loadTask('x')).rejects.toThrow('gone');
    expect(draft.lastError.value).toBeTruthy();
  });

  it('"no template" removes template_id; ranges the template wrote stay', () => {
    draft.setTemplate(template());
    draft.setTemplate(null);
    expect(draft.draft.value.template_id).toBeNull();
    expect(draft.draft.value.detail_level).toBe('detailed');
  });

  it('saveDraft falls back to the automatic title from the caller', async () => {
    saveMock.mockImplementation(async (task: AiTask) => ({ ...task, id: 's-2' }));
    const saved = await draft.saveDraft('Recovery run · 9/24');
    expect(saved.title).toBe('Recovery run · 9/24');
  });

  const beyondWorkout = (id: string) => ({
    workout_id: id,
    workout_type: 'running',
    normalized_type: 'running',
    type_source: 'numeric_mapped',
    effective_type: 'running',
    start_time: '2024-01-01T00:00:00Z',
    end_time: '2024-01-01T01:00:00Z',
    source_scope: 'cloud',
  });

  it('linked workout not in the recent list: loadTask fetches it by id', async () => {
    getMock.mockResolvedValue({ ...newTaskDraft(), id: 't-7', workout_ids: ['w-old'] });
    workoutDetailMock.mockResolvedValue(beyondWorkout('w-old'));
    await library.loadRecentWorkouts();
    await draft.loadTask('t-7');
    expect(workoutDetailMock).toHaveBeenCalledWith('w-old');
    expect(library.recentWorkouts.value.some((w) => w.workout_id === 'w-old')).toBe(true);
  });

  it('the fetch does not depend on load order: task first, recent list later still fills it', async () => {
    getMock.mockResolvedValue({ ...newTaskDraft(), id: 't-8', workout_ids: ['w-old2'] });
    workoutDetailMock.mockResolvedValue(beyondWorkout('w-old2'));
    await draft.loadTask('t-8');
    await library.loadRecentWorkouts();
    expect(library.recentWorkouts.value.some((w) => w.workout_id === 'w-old2')).toBe(true);
  });

  const deferred = <T,>() => {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((r) => { resolve = r; });
    return { promise, resolve };
  };
  const echoSave = (id: string) => async (task: AiTask) => ({ ...task, id: task.id || id, created_at: 'c', updated_at: 'u' });

  it('R02: a new draft with the same name as a saved task creates a new task, not an overwrite', async () => {
    library.taskList.value = [{ id: 'old', title: 'Last 14 days' } as never];
    saveMock.mockImplementation(echoSave('new-id'));
    draft.setTitle('Last 14 days');
    const saved = await draft.saveDraft();
    expect(saveMock.mock.calls[0][0].id).toBe('');
    expect(saved.id).toBe('new-id');
    library.taskList.value = [];
  });

  it('R03: a title changed during save is not overwritten by the receipt and stays dirty', async () => {
    const gate = deferred<AiTask>();
    saveMock.mockImplementation(() => gate.promise);
    draft.setTitle('A');
    const pending = draft.saveDraft();
    draft.setTitle('A edited');
    gate.resolve({ ...draft.draft.value, title: 'A', id: 'sid', created_at: 'c', updated_at: 'u' });
    await pending;
    expect(draft.draft.value.title).toBe('A edited');
    expect(draft.draft.value.id).toBe('sid');
    expect(draft.dirty.value).toBe(true);
  });

  it('R03: load A then B, B shows even when A arrives last', async () => {
    const a = deferred<AiTask>();
    const b = deferred<AiTask>();
    getMock.mockImplementation((id: string) => (id === 'A' ? a.promise : b.promise));
    const la = draft.loadTask('A');
    const lb = draft.loadTask('B');
    b.resolve({ ...newTaskDraft(), id: 'B', title: 'B' });
    await lb;
    expect(draft.busy.value).toBe(false);
    a.resolve({ ...newTaskDraft(), id: 'A', title: 'A' });
    await la;
    expect(draft.draft.value.id).toBe('B');
  });

  it('R03: new draft typed while A is loading, the A receipt does not overwrite it', async () => {
    const a = deferred<AiTask>();
    getMock.mockImplementation(() => a.promise);
    const la = draft.loadTask('A');
    draft.resetDraft();
    draft.setTitle('New one');
    a.resolve({ ...newTaskDraft(), id: 'A', title: 'A' });
    await la;
    expect(draft.draft.value.id).toBe('');
    expect(draft.draft.value.title).toBe('New one');
  });

  it('R03: switch to B during the A save, the A receipt does not change B', async () => {
    getMock.mockResolvedValue({ ...newTaskDraft(), id: 'A', title: 'A' });
    await draft.loadTask('A');
    const gate = deferred<AiTask>();
    saveMock.mockImplementation(() => gate.promise);
    const pending = draft.saveDraft();
    getMock.mockResolvedValue({ ...newTaskDraft(), id: 'B', title: 'B' });
    await draft.loadTask('B');
    gate.resolve({ ...newTaskDraft(), id: 'A', title: 'A', created_at: 'c', updated_at: 'u2' });
    await pending;
    expect(draft.draft.value.id).toBe('B');
    expect(draft.dirty.value).toBe(false);
  });

  it('R03: two saves in a row run in order, the second uses the id from the first', async () => {
    const gate = deferred<AiTask>();
    saveMock.mockImplementationOnce(() => gate.promise).mockImplementation(echoSave('unused'));
    draft.setTitle('Twice');
    const first = draft.saveDraft();
    const second = draft.saveDraft();
    expect(saveMock).toHaveBeenCalledTimes(1);
    gate.resolve({ ...draft.draft.value, id: 'once', created_at: 'c', updated_at: 'u' });
    await first;
    await second;
    expect(saveMock).toHaveBeenCalledTimes(2);
    expect(saveMock.mock.calls[1][0].id).toBe('once');
  });
});
