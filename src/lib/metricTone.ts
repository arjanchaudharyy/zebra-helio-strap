import { resolvedTheme } from '../composables/useTheme';
import { chartPalettes, type ZeppSemanticColors } from './echartsTheme';

export type MetricTone = Exclude<keyof ZeppSemanticColors, 'sleep'> | 'sleepDeep' | 'sleepLight' | 'sleepRem';

const TONES: Record<string, MetricTone> = {
  resting_hr: 'heart',
  blood_pressure_systolic: 'heart',
  blood_pressure_diastolic: 'sleepLight',
  blood_pressure_pulse: 'stride',
  hrv: 'stride',
  hrv_rmssd: 'sleepLight',
  sleep_score: 'sleepRem',
  readiness: 'readiness',
  stress: 'calories',
  spo2: 'pace',
  spo2_odi: 'altitude',
  respiratory_rate: 'sleepRem',
  heart_rate: 'heart',
  sleep_duration: 'sleepLight',
  sleep_start_regularity: 'sleepRem',
  workout_count: 'stride',
  steps: 'brand',
  distance: 'distance',
  active_calories: 'calories',
  active_minutes: 'altitude',
  training_load: 'training',
  vo2max: 'vo2',
  lactate_threshold_hr: 'heart',
  lactate_threshold_pace: 'pace',
  pai_total: 'calories',
  weight: 'distance',
  bmi: 'stride',
  body_fat_rate: 'calories',
  muscle_mass: 'stride',
  body_water_rate: 'pace',
  bone_mass: 'altitude',
  visceral_fat: 'calories',
  bmr: 'calories',
  height: 'altitude',
  intake_calories: 'calories',
  intake_protein_g: 'stride',
  intake_fat_g: 'altitude',
  intake_carbs_g: 'pace',
};

export const metricTone = (metric: string): MetricTone => TONES[metric] ?? 'brand';

export const toneColor = (tone: MetricTone, colors: ZeppSemanticColors): string => {
  if (tone === 'sleepDeep') return colors.sleep.deep;
  if (tone === 'sleepLight') return colors.sleep.light;
  if (tone === 'sleepRem') return colors.sleep.rem;
  return colors[tone];
};

export const metricColor = (metric: string): string =>
  toneColor(metricTone(metric), chartPalettes[resolvedTheme.value].series);
