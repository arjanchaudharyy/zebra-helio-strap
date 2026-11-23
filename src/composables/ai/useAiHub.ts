import { computed, effectScope, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { backend } from '../../lib/bridge';
import { autoTaskTitle } from '../../lib/aiTask/title';
import { directionText, templateName } from '../../lib/aiTask/prompt';
import { displayableWorkouts } from '../../lib/workouts';
import { addDays, dayKey, daysBetween } from '../../lib/aiTask/bridgeScale';
import { dayRows } from '../../lib/trainingPlan/week';
import { absorbPlanned, deleteDay, deleteWorkout, insertDay, moveDay, retypeWorkout } from '../../lib/trainingPlan/reshape';
import { workoutToInput } from '../../lib/trainingPlan/edit';
import type { PlanCheck, PlanDocument, PlanDraftPreview, PlanSport } from '../../types/trainingPlan';
import { useAiTaskDraft } from '../useAiTaskDraft';
import { useAiTaskLibrary } from '../useAiTaskLibrary';
import { useAiTaskPreview } from '../useAiTaskPreview';
import { useBridgeStrip } from '../useBridgeStrip';
import { useExchanges } from '../useExchanges';
import { useSyncController } from '../useSyncController';
import { useTrainingPlan } from '../useTrainingPlan';
import { useBridgeText } from '../../components/ai/bridge/bridge.i18n';
import { usePlanText } from '../../components/plan/usePlanText';

const today = ref(dayKey());
const demo = ref(false);
const handedOff = ref(0);
let pendingEdits: Array<(document: PlanDocument) => PlanDocument> = [];
let pendingDates: string[] = [];
let editTimer = 0;
const editWaiters: Array<() => void> = [];
let started = false;
let mounted = 0;
let timer = 0;

const tick = () => {
  if (document.hidden) return;
  const now = dayKey();
  if (now !== today.value) today.value = now;
  const plan = useTrainingPlan();
  if (!plan.busy.value) void plan.resume();
};
const onVisible = () => { if (!document.hidden) tick(); };

export const planViewOf = (check: PlanCheck, start: string): PlanDraftPreview => {
  const last = check.to && check.to > addDays(start, 6) ? check.to : addDays(start, 6);
  const count = Math.min(57, Math.max(7, daysBetween(start, last) + 1));
  return {
    check,
    window: { start },
    days: Array.from({ length: count }, (_, i) => {
      const date = addDays(start, i);
      const after = check.workouts.filter((w) => w.date === date);
      return { date, change: after.length ? 'unchanged' as const : 'rest' as const, before: [], after, rest: check.rest?.find((r) => r.date === date) ?? null };
    }),
  };
};

const bootstrap = async () => {
  const ctl = useAiTaskDraft();
  const library = useAiTaskLibrary();
  const plan = useTrainingPlan();
  const history = useExchanges();
  if (started) {
    void plan.resume();
    void history.load();
    return;
  }
  started = true;
  const coverage = useAiTaskPreview();
  const strips = useBridgeStrip();
  effectScope(true).run(() => {
    coverage.watchDraft(ctl.draft);
    const { dataRevision } = useSyncController();
    watch(dataRevision, () => { void library.loadRecentWorkouts(); void coverage.loadPreview(ctl.draft.value); void plan.load(); });
    watch(plan.revision, () => { void history.load(); void strips.load(); });
    watch(today, (day) => { strips.end.value = day; });
  });
  void history.load(); void library.loadTemplates(); void library.loadTaskList(); void library.loadRecentWorkouts(); void plan.resume();
  try {
    const prefs = await backend.getUserPrefs();
    demo.value = !!prefs.demo_mode;
    const fromLink = new URLSearchParams(window.location.search).get('task');
    if (demo.value && !ctl.draft.value.id && !fromLink) await ctl.loadTask('demo-full-task');
    if (!ctl.draft.value.personal_note) ctl.setPersonalNote(prefs.ai_profile_note ?? '');
  } catch {
  }
};

export const useAiDerived = () => {
  const ctl = useAiTaskDraft();
  const library = useAiTaskLibrary();
  const { draft } = ctl;
  const { templates, recentWorkouts } = library;
  const choices = computed(() => displayableWorkouts(recentWorkouts.value));
  const picked = computed(() => choices.value.filter((w) => draft.value.workout_ids.includes(w.workout_id)));
  const template = computed(() => templates.value.find((p) => p.id === draft.value.template_id) ?? null);
  const title = computed(() => autoTaskTitle(draft.value, picked.value, template.value ? templateName(template.value) : null));
  const direction = computed(() => directionText(template.value));
  return { choices, picked, template, title, direction, handedOff };
};

export const useAiHub = () => {
  const plan = useTrainingPlan();
  const history = useExchanges();
  const t = useBridgeText();
  const { t: pt } = usePlanText();
  const derived = useAiDerived();

  onMounted(() => {
    void bootstrap();
    mounted += 1;
    if (mounted === 1) {
      timer = window.setInterval(tick, 15000);
      document.addEventListener('visibilitychange', onVisible);
    }
  });
  onBeforeUnmount(() => {
    mounted -= 1;
    if (!mounted) {
      window.clearInterval(timer);
      document.removeEventListener('visibilitychange', onVisible);
    }
  });


  const future = computed<PlanDraftPreview | null>(() => {
    if (plan.preview.value) return plan.preview.value;
    const active = plan.state.value?.planned.filter((w) => w.date >= today.value) ?? [];
    const latest = history.exchanges.value.find((e) => e.publish_state === 'sent' && !e.undone && e.plan?.to && e.plan.to >= today.value);
    if (!active.length && !latest) return null;
    const endings = [active[active.length - 1]?.date, latest?.plan?.to, today.value].filter((d): d is string => !!d).sort();
    const last = endings[endings.length - 1]!;
    return planViewOf({
      from: today.value, to: last, workouts: active, issues: [], summary: latest?.plan?.summary,
      rest: latest?.plan?.rest?.filter((r) => r.date >= today.value),
    }, today.value);
  });
  const rows = computed(() => (future.value ? dayRows(future.value, today.value) : []));
  const stamped = computed(() => !plan.preview.value && ['sent', 'partial'].includes(plan.state.value?.last_publish?.state ?? ''));
  const written = computed(() => plan.document.value?.workouts ?? []);
  const editable = computed(() => (plan.preview.value ? plan.accepted.value : !!plan.state.value?.planned.length));

  const edit = async (change: (document: PlanDocument) => PlanDocument, dates: string[] = []): Promise<void> => {
    if (!plan.document.value && !(await plan.editActive(today.value))) return;
    if (!plan.document.value || !plan.accepted.value) return;
    const planned = (plan.state.value?.planned ?? []).map(workoutToInput);
    await plan.reshape(change(absorbPlanned(plan.document.value, planned, dates, today.value)));
  };
  const queueEdit = (change: (document: PlanDocument) => PlanDocument, dates: string[] = []): Promise<void> => {
    pendingEdits.push(change);
    pendingDates.push(...dates);
    window.clearTimeout(editTimer);
    const flush = async () => {
      if (plan.busy.value) { editTimer = window.setTimeout(() => void flush(), 120); return; }
      const batch = pendingEdits;
      const touched = pendingDates;
      pendingEdits = [];
      pendingDates = [];
      const waiters = editWaiters.splice(0);
      try {
        if (batch.length) await edit((doc) => batch.reduce((current, step) => step(current), doc), touched);
      } finally {
        for (const done of waiters) done();
      }
    };
    editTimer = window.setTimeout(() => void flush(), 350);
    return new Promise((resolve) => { editWaiters.push(resolve); });
  };
  const changeDay = (from: string, to: string) => edit((doc) => moveDay(doc, from, to), [from, to]);
  const insertAt = (from: string, to: string) => edit((doc) => insertDay(doc, from, to), [from, to]);
  const removeDay = (date: string) => edit((doc) => deleteDay(doc, date), [date]);
  const retype = (index: number, sport: PlanSport) => edit((doc) => retypeWorkout(doc, index, sport));
  const removeWorkout = (index: number) => edit((doc) => deleteWorkout(doc, index));

  const notice = computed(() => {
    const n = plan.notice.value;
    if (!n) return null;
    switch (n.kind) {
      case 'sent': return demo.value ? t.value.demoDelivered : pt.value.noticeSent;
      case 'undone': return pt.value.noticeUndone;
      case 'cleared': return pt.value.noticeClearedAll;
      case 'unconfirmed': return pt.value.noticeUnconfirmed;
      case 'rejected': return pt.value.noticeRejected;
      case 'partial': return pt.value.noticePartial(n.weeks.map((w) => w.slice(5).replace('-', '/')).join(', '));
      case 'not_needed': return pt.value.noticeNotNeeded;
      case 'nothing_to_undo': return pt.value.noticeNothingToUndo;
      case 'invalid': return pt.value.noticeInvalid;
      case 'clipboard_not_a_plan': return t.value.receiveEmpty;
      case 'paste_failed': return { empty: pt.value.pasteEmpty, no_json: pt.value.pasteNoJson, not_a_plan: pt.value.pasteNotPlan }[n.failure];
      case 'error': return n.text || pt.value.noticeError;
    }
    return null;
  });

  return {
    ...derived, today, demo,
    future, rows, stamped, written, editable, notice,
    edit, queueEdit, changeDay, insertAt, removeDay, retype, removeWorkout,
  };
};
