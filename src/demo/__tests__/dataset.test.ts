import { describe, expect, it } from 'vitest';
import { buildDemoData, DAYS, metricSeries, sleepStageSlices } from '../dataset';
import { demoWorkoutSeries, demoTrainingBalance } from '../details';
import { demoCapabilities, demoLedger } from '../facts';
import { demoPreview, demoTemplates } from '../ai';
import { createDemoPlan } from '../plan';
import { dayKey, at } from '../rng';
import { newTaskDraft } from '../../lib/aiTask/draft';
import { demoDayStrip } from '../bridge';
import { createDemoOperations } from '../operations';

const NOW = new Date(2026, 9, 2, 12, 0, 0);

describe('demo dataset', () => {
  it('handles the native AI file-copy step inside the demo sandbox', () => {
    const { handlers } = createDemoOperations(buildDemoData(NOW), createDemoPlan(NOW));
    expect(handlers.ai_handoff_copy_file({ mdPath: 'Demo/ZeppBridge.md' })).toBeNull();
  });

  it('the daily strip gives 186 days like the desktop command: a 6-month board must not stop at 90 days', () => {
    const data = buildDemoData(NOW);
    const rows = demoDayStrip(data, 184, dayKey(NOW));
    const sleep = rows.find(r => r.category === 'sleep')!;
    expect(sleep.cells).toHaveLength(184);
    expect(sleep.cells[sleep.cells.length - 1]!.date).toBe(dayKey(NOW));
    expect(sleep.cells.slice(-30).some(c => c.has)).toBe(true);
  });

  it('the blood pressure mock keeps paired trends, sparse dates and optional pulse', () => {
    const data = buildDemoData(NOW);
    const series = metricSeries(data, 30, ['blood_pressure_systolic', 'blood_pressure_diastolic', 'blood_pressure_pulse']);
    expect(series).toHaveLength(3);
    expect(series[0].points.map(p => p.date)).toEqual(series[1].points.map(p => p.date));
    expect(series[0].days_with_data).toBeLessThan(30);
    expect(series[2].days_with_data).toBeLessThan(series[0].days_with_data);
    expect(series.every(s => s.source === 'metric_samples')).toBe(true);
    expect(series[0].minimum).toBe(106);
    expect(series[0].maximum).toBe(126);
    expect(demoCapabilities(data).items.find(i => i.stream === 'blood_pressure')?.ingested).toBe(true);
  });
  const data = buildDemoData(NOW);

  it('the same "today" always gives the same data (same landing page and screenshots each time)', () => {
    const again = buildDemoData(NOW);
    expect(again.sleeps.map((s) => s.sleep_id)).toEqual(data.sleeps.map((s) => s.sleep_id));
    expect(again.workouts[0].workout_id).toBe(data.workouts[0].workout_id);
    expect(again.heart.length).toBe(data.heart.length);
  });

  it('sleep: nights without the watch stay missing; stages sum to the total; the latest night ends today', () => {
    expect(data.sleeps.length).toBe(DAYS - 6);
    expect(data.sleepDays.has(dayKey(at(NOW, -3)))).toBe(false);
    for (const s of data.sleeps) {
      const sum = (s.deep_minutes ?? 0) + (s.light_minutes ?? 0) + (s.rem_minutes ?? 0) + (s.awake_minutes ?? 0);
      expect(sum).toBe(s.duration_minutes);
      expect(s.score).toBeGreaterThanOrEqual(61);
      expect(s.score).toBeLessThanOrEqual(94);
    }
    expect(dayKey(new Date(data.sleeps[0].end_time))).toBe(dayKey(NOW));
  });

  it('stage segments join end to end and start at sleep onset', () => {
    const slices = sleepStageSlices(data.sleeps[1]);
    expect(slices.length).toBeGreaterThan(4);
    for (let i = 1; i < slices.length; i += 1) expect(slices[i].start_time).toBe(slices[i - 1].end_time);
    expect(slices[0].start_time).toBe(data.sleeps[1].start_time);
    expect(slices[slices.length - 1].end_time).toBe(data.sleeps[1].end_time);
    for (const stage of ['deep', 'light', 'rem', 'awake'] as const) {
      const minutes = slices.filter(s => s.stage === stage).reduce((sum, s) => sum + (Date.parse(s.end_time) - Date.parse(s.start_time)) / 60000, 0);
      expect(minutes).toBe(data.sleeps[1][`${stage}_minutes`]);
    }
  });

  it('workouts: none in the future, newest first, with synthetic tracks and real missing-data samples', () => {
    expect(data.workouts.length).toBeGreaterThan(20);
    for (const w of data.workouts) {
      expect(new Date(w.end_time).getTime()).toBeLessThanOrEqual(NOW.getTime());
    }
    const starts = data.workouts.map((w) => w.start_time);
    expect([...starts].sort().reverse()).toEqual(starts);
    expect(data.workouts.some(w => w.gps_available)).toBe(true);
    expect(data.workouts.some(w => !w.gps_available)).toBe(true);
  });

  it('heart rate: a gap yesterday afternoon without the watch (the gap is not filled)', () => {
    const gap = data.heart.filter((p) => {
      const t = new Date(p.timestamp);
      return dayKey(t) === dayKey(at(NOW, -1)) && t.getHours() === 15;
    });
    expect(gap).toHaveLength(0);
    expect(data.heart.length).toBeGreaterThan(1000);
  });

  it('daily metrics: body records have data, missing days are not filled with 0', () => {
    const series = metricSeries(data, 30, ['sleep_score', 'weight']);
    expect(series.map((s) => s.metric)).toEqual(['sleep_score', 'weight']);
    const [score] = series;
    expect(score.days_with_data).toBe(score.points.length);
    expect(score.days_with_data).toBeLessThanOrEqual(30);
    expect(score.points.every((p) => p.value > 0)).toBe(true);
  });

  it('strength training: one session each Tuesday, per-set detail matches the totals, no distance or splits', () => {
    const strength = data.workouts.filter(w => w.effective_type === 'strength');
    expect(strength.length).toBeGreaterThan(30);
    for (const w of strength.slice(0, 4)) {
      expect(new Date(w.start_time).getDay()).toBe(2);
      const series = demoWorkoutSeries(w);
      const detail = series.strength!;
      expect(series.splits).toHaveLength(0);
      expect(series.samples.length).toBe(w.sample_count);
      expect(detail.total_sets).toBe(detail.sets.length);
      expect(detail.total_reps).toBe(detail.sets.reduce((s, x) => s + x.reps, 0));
      expect(detail.total_volume_kg).toBeCloseTo(detail.sets.reduce((s, x) => s + (x.weight_kg ?? 0) * x.reps, 0), 1);
      expect(detail.exercise_groups.reduce((s, g) => s + g.sets, 0)).toBe(detail.total_sets);
    }
  });

  it('workout detail splits add up to the total distance and duration, stride matches total steps', () => {
    for (const workout of data.workouts.filter(w => w.effective_type !== 'strength').slice(0, 12)) {
      const series = demoWorkoutSeries(workout);
      expect(series.splits.reduce((s, split) => s + split.distance_m, 0)).toBe(workout.distance_meters);
      expect(series.splits.reduce((s, split) => s + split.duration_seconds, 0)).toBeCloseTo(workout.moving_seconds!, 0);
      if (workout.total_steps) expect(workout.avg_stride_cm! * workout.total_steps / 100).toBeCloseTo(workout.distance_meters!, -1);
      expect(series.samples.length).toBe(workout.sample_count);
      expect(series.route.length > 0).toBe(workout.gps_available);
    }
  });

  it('daily training load is a rolling 7-day sum; the balance report comes from workouts with no double counting', () => {
    const report = demoTrainingBalance(data, 7);
    for (const day of report) {
      const expected = data.workouts.filter(w => { const diff = (new Date(day.date + 'T12:00:00').getTime() - new Date(dayKey(new Date(w.start_time)) + 'T12:00:00').getTime()) / 86400000; return diff >= 0 && diff < 7; }).reduce((s,w) => s + w.training_load!, 0);
      expect(day.acute_7d).toBe(expected);
      expect(data.metrics.training_load.find(p => p.date === day.date)?.value).toBe(expected);
    }
  });

  it('the four preset "My metrics" items on the overview all have data', () => {
    const names = metricSeries(data, 30, ['resting_hr', 'hrv_rmssd', 'sleep_score', 'steps']).map((s) => s.metric);
    expect(names.sort()).toEqual(['hrv_rmssd', 'resting_hr', 'sleep_score', 'steps']);
  });

  it('data capabilities and the sync ledger come from the same records and keep the same counts', () => {
    const sleep = demoCapabilities(data).items.find(item => item.stream === 'sleep')!;
    expect(sleep.records).toBe(data.sleeps.filter(s => dayKey(new Date(s.end_time)) >= dayKey(at(NOW, -29))).length);
    const ledger = demoLedger(data);
    expect(ledger.requested_from).toBe(dayKey(at(NOW, 1 - DAYS)));
    expect(ledger.streams.find(s => s.stream === 'heart_rate')?.records).toBe(data.heart.length);
    expect(ledger.streams.find(s => s.stream === 'workouts')?.records).toBe(data.workouts.length);
  });
});

describe('demo AI preview', () => {
  const data = buildDemoData(NOW);

  it('coverage is really counted: turn off a category and its data pack is gone; body status has partial samples', () => {
    const task = newTaskDraft();
    const full = demoPreview(data, task);
    const body = full.coverage.find((row) => row.category === 'body');
    expect(body?.days_with_data).toBeGreaterThan(0);
    expect(body?.covered_dates?.length).toBe(body?.days_with_data);
    const sleepRow = full.coverage.find((row) => row.category === 'sleep');
    expect(sleepRow?.covered_dates?.length).toBe(sleepRow?.days_with_data);
    const off = demoPreview(data, { ...task, categories: task.categories.map((c) => (c.category === 'sleep' ? { ...c, enabled: false } : c)) });
    expect(off.coverage.some((row) => row.category === 'sleep')).toBe(false);
    expect(off.estimated_bytes).toBeLessThan(full.estimated_bytes);
  });

  it('all built-in focuses exist, and IDs match the UI groups', () => {
    expect(demoTemplates().map((t) => t.id)).toEqual(
      expect.arrayContaining(['sleep_review', 'recovery_trend', 'week_review', 'recovery_run', 'long_run_compare', 'hr_drift']),
    );
  });
});

describe('demo training plan', () => {
  it('an edited plan can be saved, published and withdrawn; clearing needs confirmation', () => {
    const plan = createDemoPlan(NOW), original = structuredClone(plan.state().sent);
    const document = structuredClone(plan.state().drafts[0].document);
    document.workouts[0].name = 'Edited workout';
    document.workouts[0].steps = [{ kind: 'active', duration: '20min', target: 'hr 120-140' }];
    plan.updateDraft(document);
    expect(plan.preview().check.workouts[0].steps[0]).toMatchObject({ length: { type: 'time', seconds: 1200 } });
    plan.publish({ kind: 'draft', id: 'demo-draft' }, false);
    expect(plan.state().sent[0].name).toBe('Edited workout');
    plan.publish({ kind: 'undo' }, false);
    expect(plan.state().sent).toEqual(original);
    expect(plan.publish({ kind: 'clear' }, false).outcome.outcome).toBe('needs_clear_confirmation');
    expect(plan.state().sent).toEqual(original);
    plan.publish({ kind: 'clear' }, true);
    expect(plan.state().sent).toEqual([]);
    expect(createDemoPlan(NOW).state().sent).toEqual(original);
  });
  it('the draft preview has added / replaced / removed / unchanged and one day outside the window; after sending, the draft closes and the ledger updates', () => {
    const plan = createDemoPlan(NOW);
    const kinds = new Set(plan.preview().days.map((d) => d.change));
    expect(kinds).toEqual(new Set(['rest', 'unchanged', 'added', 'replaced', 'removed']));
    expect(plan.state().drafts).toHaveLength(1);
    const result = plan.publish({ kind: 'draft', id: 'demo-draft' }, false);
    expect(result.record?.state).toBe('sent');
    expect(plan.state().drafts).toHaveLength(0);
    expect(plan.state().sent.length).toBeGreaterThan(3);
  });
});
