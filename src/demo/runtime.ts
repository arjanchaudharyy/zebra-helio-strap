import { buildDemoData, sleepStageSlices, metricSeries, type DemoData } from './dataset';
import { demoPrepare, demoPreview, demoTaskList, demoTemplates } from './ai';
import { createDemoPlan } from './plan';
import { demoDayStrip } from './bridge';
import { demoCapabilities, demoDevices, demoLedger, demoLocalApi, demoOverview, demoPrefs, demoStatus, demoWeeklyReport } from './facts';
import { dayKey } from './rng';
import { hostPost } from './host';
import { demoText } from './texts';
import { FALLBACK_APP_VERSION } from '../lib/appVersion';
import type { AiTask } from '../lib/bridge/types';
import type { PlanPublishAction } from '../types/trainingPlan';
import { demoBaselines, demoHeartExtremes, demoHourlySteps, demoInsight, demoTrainingBalance, demoWorkoutSeries, demoZones } from './details';
import { createDemoOperations } from './operations';
import { createDemoWorkoutEdits } from './workoutEdits';
import { createDemoImports } from './imports';
import { createDemoLogbook } from './logbook';
import { createDemoExtras } from './extras';
import { demoPlanHistory } from './plan';
import type { HeartRateZonePreference } from '../types';

type Handler = (args: Record<string, unknown>) => unknown;
type Callback = (event: { event: string; id: number; payload: unknown }) => void;

const STREAMS = ['heart_rate', 'daily_summary', 'sleep', 'hrv', 'wellness', 'workouts', 'workout_detail', 'weight'];
const sleep = (ms: number) => new Promise<void>((resolve) => { window.setTimeout(resolve, ms); });

export const installDemoRuntime = (now: Date = new Date()): DemoData => {
  const data = buildDemoData(now);
  const plan = createDemoPlan(now);
  // A disposable website scene: measured low readiness + intervals scheduled today.
  // Never changes the visitor's library or the normal interactive demo dataset.
  if (window.__ZB_DEMO__?.showcase && window.__ZB_DEMO__.scene === 'easy') {
    const today = dayKey(now);
    const document = structuredClone(plan.state().drafts[0]!.document);
    const workout = document.workouts.find(item => item.steps.some(step => 'repeat' in step));
    if (workout) { workout.date = today; document.from = today; plan.saveDraft(document); }
    const reading = data.metrics.readiness.find(point => point.date === today);
    if (reading) reading.value = 48;
    else data.metrics.readiness.push({ date: today, value: 48 });
  }
  const operations = createDemoOperations(data, plan);
  let zonePreference: HeartRateZonePreference = { model: 'max_hr', maxBasis: 'workout_max' };
  let clipboard = JSON.stringify(plan.state().drafts[0].document, null, 2);
  const calls: string[] = [], missing: string[] = [];
  const callbacks = new Map<number, Callback>();
  const listeners = new Map<string, number[]>();
  let nextId = 1;
  let status = demoStatus(data);
  let excludedMetrics: string[] = [];
  let demoPicks: unknown[] = [];
  const feedbackDone = new Set<string>();
  const coachNotes: import('../types/coachNotes').CoachNote[] = [
    { id: 1, text: demoText().noteMine, state: 'accepted', source: 'user', source_name: null, created_at: '2026-10-12T12:00:00Z', updated_at: '2026-10-12T12:00:00Z', decided_at: '2026-10-12T12:00:00Z' },
    { id: 2, text: demoText().noteProposed, state: 'proposed', source: 'mcp', source_name: 'Claude', created_at: '2026-10-21T09:00:00Z', updated_at: '2026-10-21T09:00:00Z', decided_at: null },
  ];

  const emit = (event: string, payload: unknown) => {
    for (const id of listeners.get(event) ?? []) callbacks.get(id)?.({ event, id: 0, payload });
  };

  let syncing = false;
  const simulateSync = async () => {
    if (syncing) return { success: true, outcome: 'no_new_data' };
    syncing = true;
    for (let index = 1; index <= STREAMS.length; index += 1) {
      emit('sync://progress', { stream: STREAMS[index - 1], current: index, total: STREAMS.length, message: '', code: 'syncing', completed: false });
      await sleep(260);
    }
    const stamp = new Date().toISOString();
    status = { ...status, last_sync: stamp, last_cloud_sync_at: stamp };
    syncing = false;
    return { success: true, outcome: 'updated', started_at: stamp, finished_at: stamp, last_cloud_sync_at: stamp, total_records: 312, streams: [] };
  };

  const hoursOf = (hours: number) => {
    const from = data.now.getTime() - hours * 3_600_000;
    return data.heart.filter((point) => new Date(point.timestamp).getTime() >= from);
  };
  const page = <T,>(all: T[], args: Record<string, unknown>) => {
    const offset = Number(args.offset ?? 0);
    const limit = Number(args.limit ?? 200);
    return { items: all.slice(offset, offset + limit), total: all.length };
  };

  const openUrl = (url: unknown) => {
    hostPost('open-url', { url: String(url ?? '') });
    if (/chatgpt|claude|kimi|gemini|deepseek|qwen/i.test(String(url))) window.dispatchEvent(new Event('demo-handoff'));
    return null;
  };

  const handlers: Record<string, Handler> = {
    app_is_ready: () => true,
    'plugin:app|version': () => FALLBACK_APP_VERSION,
    'plugin:event|listen': (args) => {
      const id = Number(args.handler);
      listeners.set(String(args.event), [...(listeners.get(String(args.event)) ?? []), id]);
      return nextId++;
    },
    'plugin:event|unlisten': () => null,
    'plugin:webview|set_webview_zoom': () => null,
    'plugin:opener|open_url': (args) => openUrl(args.url),
    open_external_url: (args) => openUrl(args.url),
    'plugin:opener|reveal_item_in_dir': () => null,
    'plugin:drag|start_drag': () => null,
    set_tray_locale: () => null,
    set_auto_sync_prefs: () => null,
    self_update_supported: () => false,

    get_app_status: () => status,
    verify_auth: () => status,
    get_login_status: () => ({ state: 'connected', message: '', page_url: '' }),
    get_official_status: () => ({ state: 'connected', message_code: null, message: null, user_id_masked: '****0000', nickname: demoText().nickname, connected_at: Math.floor(data.now.getTime() / 1000) - 86_400 * 40, authorize_url: null }),
    get_user_prefs: () => demoPrefs(),
    get_local_api_status: () => demoLocalApi(),
    get_device_profiles: () => demoDevices(data),
    get_capability_overview: () => demoCapabilities(data),
    get_coverage_ledger: () => demoLedger(data),
    get_pending_restore: () => null,

    get_health_overview: () => demoOverview(data),
    get_heart_rate_series: (args) => hoursOf(Number(args.hours ?? 24)),
    get_stress_series: (args) => {
      const from = data.now.getTime() - Number(args.hours ?? 24) * 3_600_000;
      return data.stress.filter((point) => new Date(point.timestamp).getTime() >= from);
    },
    strap_live_start: () => null,
    strap_live_stop: () => null,
    strap_key_fetch: () => 1,
    strap_daily: () => Array.from({ length: 14 }, (_, i) => {
      const d = new Date(Date.now() - (13 - i) * 86400000).toISOString().slice(0, 10);
      return { date: d, hrvSdnnMs: null, stressAvg: 32, breathRate: 14.2, hrvRmssd: 48 + Math.round(8 * Math.sin(i / 2)), spo2Avg: 97 + (i % 3) * 0.6, skinTempC: 33.8 + (i % 4) * 0.15, restingHr: 55 + (i % 3), readiness: 72 + (i % 5) * 4 };
    }),
    strap_live_recent: () => Array.from({ length: 900 }, (_, i) => ({ t: Date.now() - (900 - i) * 2000, bpm: Math.round(64 + 6 * Math.sin(i / 40) + (i % 7) / 2), rr: [] })),
    strap_settings: () => ({ '08:01': { value: 254, list: null, choices: [0, 254, 1, 5, 10, 30] }, '08:02': { value: 130, list: null, choices: [0, 100, 110, 120, 130, 140, 150] },
      '08:03': { value: 50, list: null, choices: [0, 40, 45, 50] }, '08:32': { value: 80, list: null, choices: [0, 80, 85, 90] },
      '09:05': { value: null, list: [80, 100, 120, 140, 160, 180], choices: null } }),
    strap_set: () => null,
    ai_coach_status: () => ({ provider: 'sarvam', model: '', baseUrl: '', keys: ['sarvam'], ready: true }),
    ai_coach_configure: () => ({ provider: 'sarvam', model: '', baseUrl: '', keys: ['sarvam'], ready: true }),
    strap_alerts: () => ({ alerts: [], illness: null }),
    strap_sync_now: () => '0 new records',
    ai_coach_key_status: () => true,
    ai_coach_set_key: (args) => Boolean(String(args.key ?? '').trim()),
    ai_coach_ask: () => ({ model: 'demo', content: 'This is the demo. In the app, Sarvam answers here using your own numbers.' }),
    get_metric_series: (args) => metricSeries(data, Number(args.days ?? 7), (args.metrics as string[] | null) ?? null),
    get_metric_baselines: a => demoBaselines(data, (a.metrics as string[]).filter(m => !excludedMetrics.includes(m))),
    metric_exclusions: () => [...excludedMetrics],
    set_metric_excluded: a => {
      excludedMetrics = excludedMetrics.filter(m => m !== a.metric);
      if (a.excluded) excludedMetrics = [...excludedMetrics, String(a.metric)].sort();
      return [...excludedMetrics];
    },
    get_daily_heart_rate_extremes: a => demoHeartExtremes(data, Number(a.days ?? 30)),
    get_hourly_steps: a => demoHourlySteps(data, String(a.start), String(a.end)),
    get_training_balance: a => demoTrainingBalance(data, Number(a.days ?? 90)),
    get_heart_rate_zones: a => demoZones(data, zonePreference, Number(a.days ?? 30)),
    set_heart_rate_zone_preference: a => { zonePreference = { model: a.model as string | null, maxBasis: a.maxBasis as string | null, restingBasis: a.restingBasis as string | null, thresholdBasis: a.thresholdBasis as string | null }; return demoZones(data, zonePreference, Number(a.days ?? 30)); },
    get_recent_sleep: (args) => data.sleeps.slice(0, Number(args.limit ?? 20)),
    get_sleep_page: (args) => page(data.sleeps, args),
    get_sleep_detail: (args) => {
      const found = data.sleeps.find((item) => item.sleep_id === args.sleepId);
      return found ? { ...found, stages: sleepStageSlices(found) } : null;
    },
    get_recent_workouts: (args) => data.workouts.slice(0, Number(args.limit ?? 20)),
    get_workout_page: (args) => page(data.workouts, args),
    get_workout_detail: a => data.workouts.find(w => w.workout_id === a.workoutId) ?? null,
    get_workout_series: a => { const w = data.workouts.find(w => w.workout_id === a.workoutId); if (!w) throw Object.assign(new Error('Unknown sample workout'), { code: 'err.workout.not_found' }); return demoWorkoutSeries(w); },
    get_workout_insight: a => { const w = data.workouts.find(w => w.workout_id === a.workoutId); if (!w) throw Object.assign(new Error('Unknown sample workout'), { code: 'err.workout.not_found' }); return demoInsight(data, w); },
    set_workout_type_override: a => { const w = data.workouts.find(w => w.workout_id === a.workoutId); if (!w) throw Object.assign(new Error('Unknown sample workout'), { code: 'err.workout.not_found' }); w.user_override = a.userOverride as string | null; w.effective_type = w.user_override || w.normalized_type; return w; },
    get_weekly_report: () => demoWeeklyReport(data),

    start_incremental_sync: () => simulateSync(),
    start_history_sync: () => simulateSync(),

    ai_task_list: () => demoTaskList(data),
    ai_task_day_strip: (args) => demoDayStrip(data,Number(args.daysBefore ?? 14),String(args.end ?? dayKey(data.now))),
    training_plan_adherence: a => data.workouts.filter(w => w.start_time.slice(0, 10) >= String(a.from) && w.start_time.slice(0, 10) <= String(a.to)).map(w => ({ date: dayKey(new Date(w.start_time)), planned: { name: w.effective_type, sport: w.effective_type, seconds: w.moving_seconds, hr_low: 130, hr_high: 150 }, actual: [{ workout_id: w.workout_id, seconds: w.moving_seconds, avg_hr: w.avg_hr, sport: w.effective_type, compatible: true }], verdict: 'done' })),
    training_plan_update_draft: (args) => plan.updateDraft(args.document as import('../types/trainingPlan').PlanDocument),
    workout_feedback_prompt: () => {
      const run = data.workouts.find(w => w.effective_type.includes('running'));
      if (!run || feedbackDone.has(run.workout_id)) return null;
      const seconds = run.moving_seconds ?? 1800;
      return { date: dayKey(new Date(run.start_time)), planned: { plan_workout_id: 1, name: 'Tempo 6×800m', sport: 'running', seconds: Math.round(seconds / 0.92 / 60) * 60, meters: null, hr_low: 145, hr_high: 160 },
        actual: { workout_id: run.workout_id, sport: run.effective_type, start_time: run.start_time, end_time: run.end_time, seconds, meters: run.distance_meters ?? null, avg_hr: run.avg_hr ?? null },
        completion: 0.92, completion_basis: 'duration', expires_at: new Date(Date.parse(run.end_time) + 48 * 3600e3).toISOString() };
    },
    workout_feedback_get: () => null,
    workout_feedback_save: a => { feedbackDone.add(String((a.input as { workout_id: string }).workout_id)); },
    workout_feedback_dismiss: a => { feedbackDone.add(String(a.workoutId)); },
    workout_feedback_snooze: () => { data.workouts.forEach(w => feedbackDone.add(w.workout_id)); return dayKey(new Date(data.now.getTime() + 7 * 864e5)); },
    workout_feedback_adjusted: () => null,
    training_plan_history: () => demoPlanHistory(data.now, plan.state()),
    coach_notes_list: () => ({ accepted: coachNotes.filter(n => n.state === 'accepted'), proposed: coachNotes.filter(n => n.state === 'proposed').reverse() }),
    coach_note_add: a => { const id = coachNotes.length + 1; const now = new Date().toISOString(); coachNotes.push({ id, text: String(a.text).trim(), state: 'accepted', source: 'user', source_name: null, created_at: now, updated_at: now, decided_at: now }); return id; },
    coach_note_decide: a => { const note = coachNotes.find(n => n.id === Number(a.id)); if (note) { note.state = a.accept ? 'accepted' : 'rejected'; note.decided_at = new Date().toISOString(); } },
    coach_note_edit: a => { const note = coachNotes.find(n => n.id === Number(a.id)); if (note) note.text = String(a.text).trim(); },
    coach_note_delete: a => { const at = coachNotes.findIndex(n => n.id === Number(a.id)); if (at >= 0) coachNotes.splice(at, 1); },
    coach_notes_export: a => `C:/Users/demo/Desktop/ZeppBridge AI/${String(a.fileStem)}.md`,
    'plugin:clipboard-manager|read_text': () => clipboard,
    'plugin:clipboard-manager|write_text': a => { clipboard = String(a.text ?? ''); },
    ai_template_list: () => demoTemplates(),
    ai_task_save: (args) => ({ ...(args.task as AiTask), id: (args.task as AiTask)?.id || 'demo-task-1', updated_at: new Date().toISOString() }),
    ai_task_delete: () => null,
    ai_task_set_pinned: () => null,
    card_collection_get: () => demoPicks,
    card_collection_set: (args) => { demoPicks = (args.picks as unknown[]) ?? []; return demoPicks; },
    ai_task_preview: (args) => demoPreview(data, args.task as AiTask),
    ai_task_prepare: (args) => {
      const task = args.task as AiTask;
      operations.exchanges.unshift({ ...operations.exchanges[0], id: `demo-exchange-${operations.exchanges.length + 1}`, task_id: task.id, question: task.prompt || task.title, categories: task.categories, workout_ids: task.workout_ids, sent_at: now.toISOString(), plan_draft_id: null, document: null, received_at: null, plan: null });
      return demoPrepare(data, args.task as AiTask, String(args.coverageNote ?? ''));
    },

    training_plan_state: () => plan.state(),
    training_plan_preview: () => plan.preview(),
    training_plan_save_draft: a => plan.saveDraft(a.document as import('../types/trainingPlan').PlanDocument),
    training_plan_discard: () => plan.discard(),
    training_plan_set_ai_publish: a => plan.setAiPublish(!!a.allowed),
    training_plan_publish: (args) => plan.publish(args.action as PlanPublishAction, Boolean(args.confirmClear)),
  };
  Object.assign(handlers, operations.handlers, createDemoLogbook(data), createDemoWorkoutEdits(data), createDemoImports(data), createDemoExtras(data), {
    workout_feedback_toast_due: () => null, workout_feedback_toast_show: () => null,
    export_my_records: () => 128,
    export_for_unstrap: () => ({ path: 'exports/zeppbridge-for-unstrap-demo.zip', workouts: 12, heart_rate_points: 8400 }),
    clear_auth: () => { status = { ...status, configured: false, connection_state: 'not_configured' }; return status; },
    manual_auth: () => { status = demoStatus(data); return status; },
    recover_advanced_credentials: () => handlers.get_login_status({}),
    start_web_login: () => { status = demoStatus(data); return { state: 'connected', message: '', page_url: '' }; },
    cancel_web_login: () => ({ state: 'idle', message: '', page_url: '' }),
    start_official_login: () => handlers.get_official_status({}), recover_official_credentials: () => handlers.get_official_status({}), cancel_official_login: () => handlers.get_official_status({}),
    check_official_login_now: () => handlers.get_official_status({}),
    disconnect_official: () => ({ state: 'disconnected', message: null, message_code: null, user_id_masked: null, nickname: null, connected_at: null, authorize_url: null }),
    cancel_sync: () => null,
    get_workout_type_options: () => ['running', 'cycling', 'walking', 'hiking', 'pool_swim', 'strength', 'yoga'].map(key => ({ key, label: key, family: key })),
    'plugin:dialog|open': () => 'Demo/ZeppBridge', 'plugin:dialog|save': () => 'Demo/ZeppBridge.md',
    'plugin:dialog|message': () => true, 'plugin:process|restart': () => null,
    'plugin:window|is_maximized': () => false, 'plugin:window|is_focused': () => true,
  });
  Object.assign(window, { __ZB_DEMO_API__: { calls, missing, commands: Object.keys(handlers), data, plan,
    receivePlan: () => { plan.saveDraft(); const exchange = operations.exchanges[0]; exchange.plan_draft_id = 'demo-draft'; exchange.document = plan.state().drafts[0].document; exchange.received_at = now.toISOString(); exchange.plan = plan.preview().check; clipboard = JSON.stringify(exchange.document, null, 2); },
  } });

  try {
    if (!window.localStorage.getItem('zeppbridge-overview-pins')) {
      window.localStorage.setItem('zeppbridge-overview-pins', JSON.stringify(['resting_hr', 'hrv_rmssd', 'sleep_score', 'steps']));
    }
  } catch { }

  try {
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (text: string) => { clipboard = text; }, readText: async () => clipboard } });
  } catch { }

  const internals = {
    __zbDemo: true,
    metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } },
    transformCallback(callback: Callback) {
      const id = nextId++;
      callbacks.set(id, callback);
      return id;
    },
    unregisterCallback: (id: number) => { callbacks.delete(id); },
    convertFileSrc: (path: string) => path,
    async invoke(command: string, args: Record<string, unknown> = {}) {
      await sleep(18);
      calls.push(command);
      const handler = handlers[command];
      // Native IPC serializes Vue proxies. Keep the same boundary in the browser.
      if (handler) {
        const result = await handler(JSON.parse(JSON.stringify(args)));
        return result == null ? null : JSON.parse(JSON.stringify(result));
      }
      missing.push(command);
      throw new Error(`demo: no data for ${command}`);
    },
  };
  Object.defineProperty(window, '__ZB_IPC__', { configurable: false, writable: false, value: internals });
  try { Object.assign(window, { __TAURI_INTERNALS__: internals }); } catch { }
  try { Object.assign(window, { __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => undefined } }); } catch { }
  return data;
};
