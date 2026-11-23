import { computed, ref } from 'vue';
import { backend, isDesktop, toUserMessage } from '../lib/bridge';
import { locale } from '../i18n';
import { extractPlan, type ExtractFailure } from '../lib/trainingPlan/extract';
import { workoutToInput } from '../lib/trainingPlan/edit';
import type {
  PlanDraftPreview,
  PlanPublishAction,
  PlanPublishResult,
  PlanPublishRecord,
  TrainingPlanState,
  PlanDocument,
} from '../types/trainingPlan';

export type PlanNotice =
  | { kind: 'sent' }
  | { kind: 'unconfirmed' }
  | { kind: 'rejected'; errorCode: string | null }
  | { kind: 'partial'; weeks: string[] }
  | { kind: 'not_needed' }
  | { kind: 'nothing_to_undo' }
  | { kind: 'undone' }
  | { kind: 'cleared' }
  | { kind: 'invalid' }
  | { kind: 'paste_failed'; failure: ExtractFailure }
  | { kind: 'clipboard_not_a_plan' }
  | { kind: 'error'; text: string };

const state = ref<TrainingPlanState | null>(null);
const draftId = ref<string | null>(null);
const preview = ref<PlanDraftPreview | null>(null);
const busy = ref(false);
const notice = ref<PlanNotice | null>(null);
const clearPending = ref(false);
const clearScope = ref<'all' | 'days'>('all');
let pendingAction: PlanPublishAction | null = null;
const document = ref<PlanDocument | null>(null);

const accepted = ref(true);
const editingSent = ref(false);
const revision = ref(0);

let loadSeq = 0;
let lastStateJson = '';

const load = async () => {
  if (!isDesktop()) return;
  const mine = ++loadSeq;
  try {
    const next = await backend.trainingPlanState();
    if (mine !== loadSeq) return;
    const json = JSON.stringify(next);
    if (json === lastStateJson && state.value) return;
    lastStateJson = json;
    state.value = next;
  } catch {
  }
};

const loadPreview = async (id: string) => {
  const next = await backend.trainingPlanPreview(id);
  preview.value = next;
  draftId.value = id;
};

const resume = async () => {
  await load();
  if (draftId.value || !state.value) return;
  const open = state.value.drafts[0];
  if (!open) return;
  try {
    await loadPreview(open.id);
    document.value = open.document;
    accepted.value = open.origin !== 'mcp';
  } catch {
  }
};

const report = (error: unknown): PlanNotice => ({ kind: 'error', text: toUserMessage(error, '') });

const paste = async (reply: string): Promise<boolean> => {
  notice.value = null;
  const found = extractPlan(reply);
  if (!found.ok) {
    notice.value = { kind: 'paste_failed', failure: found.failure };
    return false;
  }
  return startDraft(found.document);
};

const startDraft = async (next: PlanDocument): Promise<boolean> => {
  busy.value = true;
  try {
    const id = await backend.trainingPlanSaveDraft(next, true);
    document.value = next;
    accepted.value = true;
    await loadPreview(id);
    await load();
    revision.value++;
    return true;
  } catch (error) {
    notice.value = report(error);
    return false;
  } finally {
    busy.value = false;
  }
};

const readClipboardPlan = async (): Promise<string | null> => {
  if (busy.value) return null;
  notice.value = null;
  try {
    const reply = await backend.readClipboardText();
    if (!extractPlan(reply).ok) { notice.value = { kind: 'clipboard_not_a_plan' }; return null; }
    return reply;
  } catch (error) { notice.value = report(error); return null; }
};
const receiveFromClipboard = async (): Promise<boolean> => {
  const reply = await readClipboardPlan();
  return reply ? paste(reply) : false;
};

const editActive = async (today: string): Promise<boolean> => {
  if (busy.value || draftId.value) return !!draftId.value;
  const planned = state.value?.planned.filter((workout) => workout.date >= today) ?? [];
  if (!planned.length) return false;
  const last = planned.map((workout) => workout.date).sort().pop()!;
  const next: PlanDocument = { format: 'zeppbridge-plan/3', from: today, to: last, workouts: planned.map(workoutToInput) };
  busy.value = true;
  try {
    const id = await backend.trainingPlanSaveDraft(next, false);
    document.value = next;
    accepted.value = true;
    editingSent.value = true;
    await loadPreview(id);
    await load();
    revision.value++;
    return true;
  } catch (error) {
    notice.value = report(error);
    return false;
  } finally {
    busy.value = false;
  }
};

const reshape = async (next: PlanDocument) => {
  if (busy.value || !draftId.value || !accepted.value) return;
  busy.value = true;
  try {
    if (!(await backend.trainingPlanUpdateDraft(draftId.value, next))) throw new Error('');
    document.value = next;
    await loadPreview(draftId.value);
    await load(); revision.value++;
  } catch (error) { notice.value = report(error); }
  finally { busy.value = false; }
};

const discard = async () => {
  const id = draftId.value;
  if (!id) return;
  busy.value = true;
  try {
    await backend.trainingPlanDiscard(id);
    draftId.value = null;
    document.value = null;
    preview.value = null;
    notice.value = null;
    editingSent.value = false;
    await load();
    revision.value++;
  } catch (error) {
    notice.value = report(error);
  } finally {
    busy.value = false;
  }
};

const recordNotice = (record: PlanPublishRecord | null, records: PlanPublishRecord[], fallback: PlanNotice): PlanNotice => {
  if (!record) return fallback;
  if (record.state === 'sent') return fallback;
  if (record.state === 'rejected') return { kind: 'rejected', errorCode: record.error_code };
  if (record.state === 'partial') {
    const weeks = records.filter((row) => row.state === 'rejected' && row.role !== 'placeholder').map((row) => row.window_start);
    return { kind: 'partial', weeks: [...new Set(weeks)].sort() };
  }
  return { kind: 'unconfirmed' };
};

const run = async (action: PlanPublishAction, confirmClear: boolean): Promise<void> => {
  busy.value = true;
  notice.value = null;
  clearPending.value = false;
  try {
    const result: PlanPublishResult = await backend.trainingPlanPublish(action, confirmClear, locale.value);
    const { outcome, record } = result;
    const records = result.records ?? [];
    switch (outcome.outcome) {
      case 'send': {
        const done: PlanNotice = action.kind === 'undo' ? { kind: 'undone' } : action.kind === 'clear' ? { kind: 'cleared' } : { kind: 'sent' };
        notice.value = recordNotice(record, records, done);
        if (action.kind === 'draft' && record?.state !== 'rejected') {
          draftId.value = null;
          preview.value = null;
          document.value = null;
          editingSent.value = false;
        } else if (action.kind === 'draft' && draftId.value) {
          await loadPreview(draftId.value);
        }
        break;
      }
      case 'not_needed':
        notice.value = { kind: 'not_needed' };
        if (action.kind === 'draft') { draftId.value = null; preview.value = null; document.value = null; editingSent.value = false; }
        break;
      case 'nothing_to_undo':
        notice.value = { kind: 'nothing_to_undo' };
        break;
      case 'needs_clear_confirmation':
        pendingAction = action;
        clearScope.value = action.kind === 'clear' ? 'all' : 'days';
        clearPending.value = true;
        break;
      case 'invalid':
        notice.value = { kind: 'invalid' };
        if (draftId.value) await loadPreview(draftId.value);
        break;
    }
  } catch (error) {
    notice.value = report(error);
  } finally {
    busy.value = false;
    await load();
    revision.value++;
  }
};

const publish = () => {
  if (busy.value || !accepted.value || blocking.value.length) return Promise.resolve();
  const id = draftId.value;
  if (id) return run({ kind: 'draft', id }, false);
  return Promise.resolve();
};
const undo = () => run({ kind: 'undo' }, false);
const resend = () => (busy.value ? Promise.resolve() : run({ kind: 'resend' }, false));
const clear = (confirmed = false) => run({ kind: 'clear' }, confirmed);

const dismissNotice = () => { notice.value = null; };
const cancelClear = () => { clearPending.value = false; pendingAction = null; };
const confirmClear = () => { const action = pendingAction; pendingAction = null; return action ? run(action,true) : Promise.resolve(); };

/** Errors and unverified shapes both block delivery; warnings do not. The backend enforces the same gate. */
const blocking = computed(() => preview.value?.check.issues.filter((issue) => issue.severity !== 'warning') ?? []);
const unverified = computed(() => preview.value?.check.issues.filter((issue) => issue.severity === 'unverified') ?? []);

export const useTrainingPlan = () => ({
  state, draftId, preview, busy, notice, clearPending, clearScope, blocking, unverified,
  document, accepted, revision, editingSent, readClipboardPlan, receiveFromClipboard, reshape, editActive,
  accept: () => { accepted.value = true; },
  load, resume, paste, startDraft, discard, publish, undo, resend, clear, dismissNotice, cancelClear, confirmClear,
});
