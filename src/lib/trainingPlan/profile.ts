import type {
  PlanIntensity,
  PlanStep,
  PlanStepNode,
  PlanWorkout,
} from '../../types/trainingPlan';

export const APPROX_SECONDS_PER_KM = 360;

export interface ProfileSegment {
  intensity: PlanIntensity;
  seconds: number;
  approx: boolean;
  target: 'heart_rate' | 'pace' | 'power' | 'open';
  low: number | null;
  high: number | null;
  round: number | null;
  path: number[];
}

export interface StepRow {
  intensity: PlanIntensity;
  length: PlanStep['length'];
  target: PlanStep['target'];
  remark?: string;
}

export type StepGroup =
  | { kind: 'step'; row: StepRow }
  | { kind: 'repeat'; times: number; rows: StepRow[] };

export interface WorkoutProfile {
  segments: ProfileSegment[];
  seconds: number;
  approx: boolean;
  hasHeartRate: boolean;
  groups: StepGroup[];
}

const segmentOf = (step: PlanStep, round: number | null, path: number[]): ProfileSegment => {
  const approx = step.length.type === 'distance';
  const seconds = step.length.type === 'time'
    ? step.length.seconds
    : Math.round((step.length.meters / 1000) * APPROX_SECONDS_PER_KM);
  const hr = step.target.type === 'heart_rate' ? step.target : null;
  return {
    intensity: step.intensity,
    seconds,
    approx,
    target: step.target.type,
    low: hr ? hr.low : null,
    high: hr ? hr.high : null,
    round,
    path,
  };
};

const rowOf = (step: PlanStep): StepRow => {
  const { note: remark } = step;
  return { intensity: step.intensity, length: step.length, target: step.target, remark };
};

export const workoutProfile = (workout: PlanWorkout): WorkoutProfile => {
  const segments: ProfileSegment[] = [];
  const groups: StepGroup[] = [];
  const walk = (node: PlanStepNode, index: number) => {
    if (node.type === 'step') {
      segments.push(segmentOf(node, null, [index]));
      groups.push({ kind: 'step', row: rowOf(node) });
      return;
    }
    for (let round = 0; round < node.times; round += 1) {
      node.steps.forEach((step, inner) => segments.push(segmentOf(step, round, [index, inner])));
    }
    groups.push({ kind: 'repeat', times: node.times, rows: node.steps.map(rowOf) });
  };
  workout.steps.forEach(walk);
  return {
    segments,
    seconds: segments.reduce((sum, segment) => sum + segment.seconds, 0),
    approx: segments.some((segment) => segment.approx),
    hasHeartRate: segments.some((segment) => segment.low !== null),
    groups,
  };
};

export const heartRateDomain = (profile: WorkoutProfile): { min: number; max: number } => {
  const lows = profile.segments.flatMap((segment) => (segment.low === null ? [] : [segment.low]));
  const highs = profile.segments.flatMap((segment) => (segment.high === null ? [] : [segment.high]));
  if (!lows.length || !highs.length) return { min: 90, max: 180 };
  const min = Math.floor((Math.min(...lows) - 8) / 10) * 10;
  const max = Math.ceil((Math.max(...highs) + 8) / 10) * 10;
  return { min: Math.max(40, min), max: Math.max(min + 40, max) };
};

export const profileMinutes = (profile: WorkoutProfile): number => Math.round(profile.seconds / 60);
