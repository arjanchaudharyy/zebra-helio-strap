export type MetricTrust = 'measured' | 'computed' | 'inferred' | 'modeled';

const TRUST: Record<string, MetricTrust> = {
  steps: 'measured',
  heart_rate_24h: 'measured',
  daily_max_hr: 'measured',
  resting_hr: 'measured',
  spo2: 'measured',
  hrv: 'measured',
  weight: 'measured',
  height: 'measured',
  blood_pressure_systolic: 'measured',
  blood_pressure_diastolic: 'measured',
  blood_pressure_pulse: 'measured',

  bmi: 'computed',
  bmr: 'computed',
  hrv_rmssd: 'computed',
  spo2_odi: 'computed',
  training_balance: 'computed',
  sleep_weekly: 'computed',
  intake_calories: 'computed',
  intake_protein_g: 'computed',
  intake_fat_g: 'computed',
  intake_carbs_g: 'computed',

  pai_total: 'inferred',
  stress: 'inferred',
  respiratory_rate: 'inferred',
  distance: 'inferred',
  active_calories: 'inferred',
  active_minutes: 'inferred',
  sleep_duration: 'inferred',
  sleep_score: 'inferred',
  sleep_stages: 'inferred',

  vo2max: 'modeled',
  lactate_threshold: 'modeled',
  training_load: 'modeled',
  readiness: 'modeled',
  body_fat_rate: 'modeled',
  muscle_mass: 'modeled',
  body_water_rate: 'modeled',
  bone_mass: 'modeled',
  visceral_fat: 'modeled',
};

export const metricTrust = (metric: string): MetricTrust | null => TRUST[metric] ?? null;
