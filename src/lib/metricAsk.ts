import type { AiTaskCategory, AiTaskDetailLevel } from './bridge/types';

const BY_PREFIX: Array<[RegExp, AiTaskCategory]> = [
  [/^sleep|^deep_|^rem_|^light_|^awake_|^nap/, 'sleep'],
  [/^(blood_pressure|weight|bmi|body_|muscle|bone|visceral|bmr|protein|fat_|calorie_intake|food)/, 'body'],
  [/^(training_load|vo2max|lactate|pai|steps|distance|active_|floors)/, 'training'],
  [/^(heart_rate|hr_|max_hr|min_hr)/, 'heart_rate'],
  [/^(resting_hr|readiness|physical_readiness|stress|respiratory|hrv|sleep_hrv|spo2|charge|skin_temp|body_temp)/, 'recovery'],
];

export const askCategoryOf = (metric: string): AiTaskCategory => {
  if (metric === 'resting_hr') return 'recovery';
  for (const [pattern, category] of BY_PREFIX) if (pattern.test(metric)) return category;
  return 'recovery';
};

export type AskDirection = 'above' | 'below' | 'usual' | null;

export interface AskPreset {
  key: string;
  categories: AiTaskCategory[];
  days: number;
  daysBy?: Partial<Record<AiTaskCategory, number>>;
  detail?: AiTaskDetailLevel;
}

export const askPresets = (metric: string): AskPreset[] => {
  const category = askCategoryOf(metric);
  const first: AskPreset = { key: 'first', categories: [category], days: 28 };
  if (category === 'sleep') {
    return [first, { key: 'sleepWithTraining', categories: ['sleep', 'training'], days: 28 }];
  }
  if (category === 'body') {
    return [first, { key: 'bodyFood', categories: ['body', 'training'], days: 56 }, { key: 'withSleep', categories: ['body', 'sleep'], days: 28 }];
  }
  if (category === 'training') {
    return [first, { key: 'trainingNext', categories: ['training', 'recovery'], days: 28 }, { key: 'withSleep', categories: ['training', 'sleep'], days: 28 }];
  }
  return [first, { key: 'withTraining', categories: [category, 'training'], days: 28 }, { key: 'withSleep', categories: [category, 'sleep'], days: 28 }];
};

const THIS_WORKOUT = { workout: 1 } as const;
export const workoutAskPresets = (): AskPreset[] => [
  { key: 'workoutReview', categories: ['workout', 'training', 'recovery', 'sleep', 'resting_hr'], days: 7, daysBy: THIS_WORKOUT, detail: 'detailed' },
  { key: 'workoutRecovery', categories: ['workout', 'sleep', 'recovery', 'resting_hr'], days: 30, daysBy: THIS_WORKOUT },
  { key: 'workoutNext', categories: ['workout', 'training', 'recovery', 'sleep'], days: 14, daysBy: { ...THIS_WORKOUT, training: 28 } },
];

export const sleepAskPresets = (): AskPreset[] => [
  { key: 'sleepNight', categories: ['sleep', 'recovery', 'resting_hr'], days: 14 },
  { key: 'sleepWithTraining', categories: ['sleep', 'recovery', 'resting_hr', 'training'], days: 14, daysBy: { training: 28 } },
  { key: 'sleepRecovery', categories: ['sleep', 'recovery', 'resting_hr'], days: 14 },
];

export const daysEndingOn = (end: string, days: number): string[] => {
  const last = Date.parse(`${end}T00:00:00Z`);
  if (Number.isNaN(last)) return [];
  return Array.from({ length: Math.max(1, days) }, (_, i) => new Date(last - (days - 1 - i) * 86_400_000).toISOString().slice(0, 10));
};
