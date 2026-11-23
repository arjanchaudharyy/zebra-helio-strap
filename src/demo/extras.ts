import type { DemoData } from './dataset';
import type { DemoHandler } from './operations';
import { demoStrengthCatalog } from './strength';
import { createDemoPush } from './push';
import { demoText } from './texts';
import { at, dayKey, iso } from './rng';
import type { CloudCompanionState, CloudConfigInput, CloudSetupCommand } from '../lib/bridge/types';
import type { SleepStageSlice, Workout, WorkoutCodeLabel, WorkoutFitOverride } from '../types';

const unknownWorkout = (now: Date, back: number, hour: number, code: number, minutes: number, avg: number): Workout => {
  const start = at(now, -back, hour, 30);
  const end = new Date(start.getTime() + minutes * 60_000);
  return {
    workout_id: String(Math.floor(start.getTime() / 1000)), workout_type: `unknown:${code}`, normalized_type: `unknown:${code}`,
    type_source: 'unknown_code', user_override: null, effective_type: `unknown:${code}`, custom_label: null, zepp_type: code, zepp_source: null,
    start_time: iso(start), end_time: iso(end), moving_seconds: minutes * 60, calories: Math.round(minutes * 7.4),
    avg_hr: avg, max_hr: avg + 31, min_hr: avg - 34, elevation_gain_m: null, elevation_loss_m: null,
    training_load: Math.round(minutes * 1.1), training_effect: 2.6, anaerobic_training_effect: 1.2, rpe: null,
    avg_cadence_spm: null, max_cadence_spm: null, avg_stride_cm: null, total_steps: null,
    gps_available: false, sample_count: minutes * 6 + 1, source_scope: 'device', device_id: 'DEMO0000BAND01',
    synced_at: iso(new Date(end.getTime() + 12 * 60_000)), hr_zones: [],
  };
};

const napSlices = (wake: Date, now: Date): SleepStageSlice[] => {
  const day = wake.getDay();
  if (day !== 0 && day !== 6) return [];
  const naps: Array<[hour: number, minute: number, stages: Array<[string, number]>]> = [
    [13, 40, day === 0 ? [['light', 7], ['deep', 9], ['light', 6], ['rem', 4], ['awake', 2]] : [['light', 9], ['deep', 6], ['light', 5], ['awake', 1]]],
  ];
  if (day === 6) naps.push([17, 25, [['light', 14], ['awake', 2]]]);
  const out: SleepStageSlice[] = [];
  for (const [hour, minute, stages] of naps) {
    let cursor = at(wake, 0, hour, minute).getTime();
    const slices = stages.map(([stage, minutes]) => {
      const slice = { stage, start_time: new Date(cursor).toISOString(), end_time: new Date(cursor + minutes * 60_000).toISOString() };
      cursor += minutes * 60_000;
      return slice;
    });
    if (cursor <= now.getTime()) out.push(...slices);
  }
  return out;
};

export const createDemoExtras = (data: DemoData): Record<string, DemoHandler> => {
  const now = data.now;

  const labels = new Map<number, { label: string; updatedAt: string }>([[227, { label: demoText().codeLabel, updatedAt: iso(at(now, -30, 21)) }]]);
  for (const [back, hour, code, minutes, avg] of [[12, 16, 227, 58, 128], [33, 16, 227, 64, 131], [61, 10, 227, 52, 126], [26, 15, 123, 41, 118]] as const) {
    const workout = unknownWorkout(now, back, hour, code, minutes, avg);
    data.workouts.push(workout);
    data.workoutDays.add(dayKey(new Date(workout.start_time)));
  }
  data.workouts.sort((a, b) => b.start_time.localeCompare(a.start_time));
  const applyLabels = () => {
    for (const w of data.workouts) if (w.workout_type.startsWith('unknown:')) w.custom_label = labels.get(w.zepp_type ?? -1)?.label ?? null;
  };
  applyLabels();
  const unknownCodes = (): WorkoutCodeLabel[] => {
    const counts = new Map<number, number>();
    for (const w of data.workouts) if (w.workout_type.startsWith('unknown:') && w.zepp_type != null) counts.set(w.zepp_type, (counts.get(w.zepp_type) ?? 0) + 1);
    return [...counts].sort((a, b) => b[1] - a[1] || a[0] - b[0])
      .map(([code, records]) => ({ zeppType: code, label: labels.get(code)?.label ?? '', records, updatedAt: labels.get(code)?.updatedAt ?? '' }));
  };

  const fits = new Map<string, WorkoutFitOverride>();
  const importFit = (workout: Workout, fileName: string, importedAt: string): WorkoutFitOverride => {
    const previous = fits.get(workout.workout_id);
    const originalDistanceM = previous ? previous.originalDistanceM : workout.distance_meters ?? null;
    const originalMovingSeconds = previous ? previous.originalMovingSeconds : workout.moving_seconds ?? null;
    const distanceM = originalDistanceM == null ? null : Math.round(originalDistanceM * 1.031);
    const override: WorkoutFitOverride = { workoutId: workout.workout_id, fileName, sha256: 'f'.repeat(64), byteLen: 48_213, importedAt,
      distanceM, movingSeconds: originalMovingSeconds, originalDistanceM, originalMovingSeconds };
    if (distanceM != null) workout.distance_meters = distanceM;
    fits.set(workout.workout_id, override);
    return override;
  };
  const fitTarget = data.workouts.find(w => w.effective_type === 'running' && !w.gps_available && Date.parse(w.start_time) < at(now, -20).getTime());
  if (fitTarget) importFit(fitTarget, 'phone-run.fit', iso(at(now, -18, 22, 6)));
  const findWorkout = (id: unknown) => {
    const w = data.workouts.find(item => item.workout_id === id);
    if (!w) throw Object.assign(new Error('Unknown sample workout'), { code: 'err.workout.not_found' });
    return w;
  };

  let cloud: CloudCompanionState = {
    supported: true, configured: true, config_unreadable: false,
    state_dir: 'C:/Users/demo/AppData/Local/ZeppBridge-Cloud', uploader_dir: 'C:/Users/demo/ZeppBridge-Cloud/uploader',
    config: { site: 'health.example.com', bin_dir: null, data_dir: null, sync: true, retention_days: 90, summary_days: 30, full_resolution_workouts: false },
    site_url: 'https://health.example.com',
    last_run: { at: iso(at(now, 0, 6, 52)), mode: 'scheduled', result: 'ok', exit_code: 0, log_line: 'upload ok: 14 days, 9 workouts' },
    readers: [{ label: 'Coach', created_at: iso(at(now, -21, 20)) }],
    task_follow: true, task_upload: true, running: false, default_bin_dir: 'C:/Users/demo/ZeppBridge',
  };
  const cloudCopy = () => structuredClone(cloud);

  return {
    system_region_locale: () => navigator.language || null,
    start_fresh_library: () => null,
    ai_handoff_autopaste: () => { throw new Error('demo: auto paste only runs in the desktop app'); },
    ai_handoff_cancel: () => null,

    get_strength_catalog: () => demoStrengthCatalog(now),

    get_sleep_nap_stages: a => {
      const sleep = data.sleeps.find(s => s.sleep_id === a.sleepId);
      return sleep ? napSlices(new Date(sleep.end_time), now) : [];
    },

    get_unknown_workout_codes: () => unknownCodes(),
    set_workout_code_label: a => {
      const code = Number(a.zeppType), label = String(a.label ?? '').trim();
      if (label) labels.set(code, { label, updatedAt: new Date().toISOString() }); else labels.delete(code);
      applyLabels();
      return unknownCodes();
    },

    get_workout_fit_override: a => fits.get(String(a.workoutId)) ?? null,
    import_workout_fit: a => importFit(findWorkout(a.workoutId), String(a.path ?? 'workout.fit').split(/[\\/]/).pop() || 'workout.fit', new Date().toISOString()),
    remove_workout_fit: a => {
      const fit = fits.get(String(a.workoutId));
      if (!fit) return false;
      const w = findWorkout(a.workoutId);
      w.distance_meters = fit.originalDistanceM ?? undefined;
      w.moving_seconds = fit.originalMovingSeconds;
      fits.delete(w.workout_id);
      return true;
    },

    cloud_companion_state: () => cloudCopy(),
    cloud_companion_save_config: a => {
      const { uploader_dir, ...config } = a.input as CloudConfigInput;
      cloud = { ...cloud, config, uploader_dir: uploader_dir ?? cloud.uploader_dir, site_url: `https://${config.site.replace(/^https?:\/\//, '').replace(/\/+$/, '')}` };
      return cloudCopy();
    },
    cloud_companion_upload: () => {
      cloud = { ...cloud, last_run: { at: new Date().toISOString(), mode: 'manual', result: 'ok', exit_code: 0, log_line: 'upload ok: 14 days, 9 workouts' } };
      return cloudCopy();
    },
    cloud_companion_setup: a => {
      const command = a.command as CloudSetupCommand, label = String(a.label ?? '').trim();
      if (command === 'add-reader' && label) cloud = { ...cloud, readers: [...cloud.readers, { label, created_at: new Date().toISOString() }] };
      if (command === 'revoke-reader') cloud = { ...cloud, readers: cloud.readers.filter(r => r.label !== label) };
      const output = command === 'task-commands' ? 'schtasks /Create /SC DAILY /ST 06:50 /TN "ZeppBridge Cloud upload" /TR "<uploader>\\upload.cmd"' : null;
      return { state: cloudCopy(), output };
    },
    cloud_companion_open: () => null,
    ...createDemoPush(),
  };
};
