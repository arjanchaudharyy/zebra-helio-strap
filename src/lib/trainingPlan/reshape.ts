import type { PlanDocument, PlanSport, PlanVariant, PlanWorkoutInput } from '../../types/trainingPlan';
const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

export const absorbPlanned = (document: PlanDocument, planned: PlanWorkoutInput[], dates: string[], today: string): PlanDocument => {
  const wanted = dates.filter((date) => date >= today);
  if (!wanted.length) return document;
  const written = document.workouts.map((w) => w.date).sort();
  const oldFrom = document.from ?? written[0];
  const oldTo = document.to ?? written[written.length - 1];
  const bounds = [oldFrom, oldTo, ...wanted].filter((date): date is string => !!date).sort();
  const from = bounds[0]!;
  const to = bounds[bounds.length - 1]!;
  if (from === oldFrom && to === oldTo) return document;
  const outside = (date: string) => !oldFrom || !oldTo || date < oldFrom || date > oldTo;
  const moved = planned.filter((w) => w.date >= today && w.date >= from && w.date <= to && outside(w.date));
  return { ...clone(document), from, to, workouts: [...clone(document.workouts), ...clone(moved)] };
};

/** Swap whole days, including local rest notes. Never overwrite a destination. */
export const moveDay = (document: PlanDocument, from: string, to: string): PlanDocument => {
  if (from === to) return clone(document);
  if (document.from && to < document.from || document.to && to > document.to) return clone(document);
  const swap = (date: string) => date === from ? to : date === to ? from : date;
  return { ...clone(document), workouts: document.workouts.map(w => ({ ...clone(w), date: swap(w.date) })),
    rest: document.rest?.map(r => ({ ...r, date: swap(r.date) })) };
};
const shiftDate = (date: string, days: number): string => {
  const [y, m, d] = date.split('-').map(Number);
  return new Date(Date.UTC(y!, m! - 1, d! + days)).toISOString().slice(0, 10);
};

export const insertDay = (document: PlanDocument, from: string, to: string): PlanDocument => {
  if (from === to) return clone(document);
  if (document.from && to < document.from || document.to && to > document.to) return clone(document);
  const forward = to > from;
  const move = (date: string) => {
    if (date === from) return to;
    if (forward && date > from && date <= to) return shiftDate(date, -1);
    if (!forward && date >= to && date < from) return shiftDate(date, 1);
    return date;
  };
  return { ...clone(document), workouts: document.workouts.map(w => ({ ...clone(w), date: move(w.date) })),
    rest: document.rest?.map(r => ({ ...r, date: move(r.date) })) };
};

export const SPORT_VARIANTS: Record<PlanSport, PlanVariant[]> = {
  running: ['outdoor', 'treadmill', 'track'],
  cycling: ['outdoor', 'indoor'],
  pool_swim: [],
  open_water_swim: [],
};

export const retypeWorkout = (document: PlanDocument, index: number, sport: PlanSport, variant?: PlanVariant): PlanDocument => {
  const next = clone(document);
  const target = next.workouts[index];
  if (!target) return next;
  target.sport = sport;
  const keep = variant ?? (target.variant as PlanVariant | undefined);
  if (keep && SPORT_VARIANTS[sport].includes(keep)) target.variant = keep;
  else delete target.variant;
  return next;
};

export const deleteWorkout = (document: PlanDocument, index: number): PlanDocument => {
  const next = clone(document);
  next.workouts.splice(index, 1);
  return next;
};

export const deleteDay = (document: PlanDocument, date: string): PlanDocument => ({ ...clone(document),
  workouts: document.workouts.filter(w => w.date !== date).map(w => clone(w)), rest: document.rest?.filter(r => r.date !== date).map(r => ({ ...r })) });

export const addWorkout = (document: PlanDocument, workout: Omit<PlanWorkoutInput, 'date'>, date: string): PlanDocument => {
  const next = clone(document);
  next.workouts.push({ ...clone(workout), date });
  if (next.from && date < next.from) next.from = date;
  if (next.to && date > next.to) next.to = date;
  return next;
};
