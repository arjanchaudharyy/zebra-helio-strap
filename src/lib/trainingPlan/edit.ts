import type {
  PlanDocument, PlanStepInput, PlanStepLength, PlanStepNode, PlanTarget, PlanWorkout, PlanWorkoutInput,
} from '../../types/trainingPlan';

const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;
const pad = (value: number) => String(value).padStart(2, '0');

export const lengthText = (length: PlanStepLength): string => {
  if (length.type === 'time') return length.seconds % 60 === 0 ? `${length.seconds / 60}min` : `${length.seconds}s`;
  return length.meters % 1000 === 0 ? `${length.meters / 1000}km` : `${length.meters}m`;
};

const paceText = (seconds: number) => `${Math.floor(seconds / 60)}:${pad(seconds % 60)}`;

export const targetText = (target: PlanTarget): string | undefined => {
  switch (target.type) {
    case 'heart_rate': return `hr ${target.low}-${target.high}`;
    case 'pace': return `pace ${paceText(target.fast)}-${paceText(target.slow)}`;
    case 'power': return `power ${target.low}-${target.high}`;
    default: return undefined;
  }
};

export const workoutToInput = (workout: PlanWorkout): PlanWorkoutInput => {
  const single = (step: { intensity: string; length: PlanStepLength; target: PlanTarget; note?: string }): PlanStepInput => {
    const out: PlanStepInput = { kind: step.intensity, duration: lengthText(step.length) };
    const target = targetText(step.target);
    if (target) out.target = target;
    if (step.note) out.note = step.note;
    return out;
  };
  const node = (n: PlanStepNode): PlanStepInput => (n.type === 'repeat'
    ? { repeat: n.times, steps: n.steps.map(single) }
    : single(n));
  const input: PlanWorkoutInput = { date: workout.date, sport: workout.sport, name: workout.name, steps: workout.steps.map(node) };
  if (workout.variant) input.variant = workout.variant;
  if (workout.focus) input.focus = workout.focus;
  if (workout.description) input.description = workout.description;
  return input;
};

export const documentIndexOf = (document: PlanDocument, date: string, k: number, blocked: Set<number>): number | null => {
  let seen = 0;
  for (let index = 0; index < document.workouts.length; index += 1) {
    const workout = document.workouts[index]!;
    if (workout.date !== date || blocked.has(index)) continue;
    if (seen === k) return index;
    seen += 1;
  }
  return null;
};

type Single = Extract<PlanStepInput, { kind: string }>;

const withStep = (document: PlanDocument, workout: number, path: number[], change: (step: Single) => void): PlanDocument => {
  const next = clone(document);
  const steps = next.workouts[workout]?.steps;
  if (!steps) return next;
  const node = steps[path[0]!];
  if (!node) return next;
  if ('repeat' in node) {
    const inner = path.length > 1 ? node.steps[path[1]!] : undefined;
    if (inner && 'kind' in inner) change(inner);
    return next;
  }
  if (path.length === 1) change(node);
  return next;
};

export const setStepLength = (document: PlanDocument, workout: number, path: number[], length: PlanStepLength): PlanDocument =>
  withStep(document, workout, path, (step) => { step.duration = lengthText(length); });

export const setStepTarget = (document: PlanDocument, workout: number, path: number[], target: PlanTarget): PlanDocument =>
  withStep(document, workout, path, (step) => {
    const text = targetText(target);
    if (text) step.target = text;
    else delete step.target;
  });

export const setRepeat = (document: PlanDocument, workout: number, index: number, times: number): PlanDocument => {
  const next = clone(document);
  const node = next.workouts[workout]?.steps[index];
  if (node && 'repeat' in node) node.repeat = Math.min(50, Math.max(1, Math.round(times)));
  return next;
};

export const setText = (document: PlanDocument, workout: number, field: 'focus' | 'description' | 'name', value: string): PlanDocument => {
  const next = clone(document);
  const target = next.workouts[workout];
  if (!target) return next;
  const trimmed = value.trim();
  if (field === 'name') target.name = trimmed;
  else if (trimmed) target[field] = trimmed;
  else delete target[field];
  return next;
};


export type AddKind = 'warmup' | 'step' | 'repeat' | 'cooldown';

export const addTemplate = (kind: AddKind): PlanStepInput => {
  switch (kind) {
    case 'warmup': return { kind: 'warmup', duration: '10min' };
    case 'cooldown': return { kind: 'cooldown', duration: '10min' };
    case 'repeat': return { repeat: 4, steps: [{ kind: 'active', duration: '3min' }, { kind: 'recovery', duration: '2min' }] };
    default: return { kind: 'active', duration: '10min' };
  }
};

const isKind = (node: PlanStepInput | undefined, kind: string) => !!node && 'kind' in node && node.kind === kind;

export const addStep = (document: PlanDocument, workout: number, kind: AddKind): { document: PlanDocument; index: number } => {
  const next = clone(document);
  const steps = next.workouts[workout]?.steps;
  if (!steps) return { document: next, index: -1 };
  let at = steps.length;
  if (kind === 'warmup') at = 0;
  else if (kind !== 'cooldown') while (at > 0 && isKind(steps[at - 1], 'cooldown')) at -= 1;
  steps.splice(at, 0, addTemplate(kind));
  return { document: next, index: at };
};

export const removeStep = (document: PlanDocument, workout: number, path: number[]): PlanDocument => {
  const next = clone(document);
  const steps = next.workouts[workout]?.steps;
  const node = steps?.[path[0]!];
  if (!steps || !node) return next;
  if (path.length === 1) {
    steps.splice(path[0]!, 1);
    return next;
  }
  if ('repeat' in node && node.steps[path[1]!]) {
    node.steps.splice(path[1]!, 1);
    if (!node.steps.length) steps.splice(path[0]!, 1);
  }
  return next;
};
