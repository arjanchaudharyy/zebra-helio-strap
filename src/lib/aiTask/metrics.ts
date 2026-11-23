import { defineMessages, messagesOf } from '../../i18n';
import type { AiTaskCategory } from '../bridge/types';

const metricMessages = defineMessages(
  {
    resting_hr: 'Resting HR', readiness: 'Readiness', physical_readiness: 'Physical readiness', mental_readiness: 'Mental readiness',
    hybrid_charge: 'Hybrid Charge', physical_charge: 'Physical Charge', mental_charge: 'Mental Charge', stress: 'Stress',
    respiratory_rate: 'Respiratory rate', sleep_hrv: 'Sleep HRV', sleep_rhr: 'Sleep resting HR', hrv_baseline: 'HRV baseline',
    rhr_baseline: 'Resting HR baseline', ahi_baseline: 'AHI baseline', spo2_odi: 'SpO₂ ODI', spo2_night_score: 'Night SpO₂ score',
    spo2_measured_minutes: 'SpO₂ measured time', hrv: 'HRV', hrv_rmssd: 'HRV (RMSSD)', spo2: 'SpO₂',
    heart_rate: 'All-day heart rate',
    blood_pressure_systolic: 'Systolic pressure (experimental)', blood_pressure_diastolic: 'Diastolic pressure (experimental)', blood_pressure_pulse: 'Pulse at BP measurement',
    training_load: 'Training load', vo2max: 'VO₂max', lactate_threshold_hr: 'Lactate threshold HR', lactate_threshold_pace: 'Lactate threshold pace',
    pai_daily: 'Daily PAI', pai_total: 'Total PAI', steps: 'Steps', active_calories: 'Active calories', active_minutes: 'Active minutes',
    weight: 'Weight', bmi: 'BMI', height: 'Height', body_fat_rate: 'Body fat', body_water_rate: 'Body water', muscle_mass: 'Muscle mass',
    bone_mass: 'Bone mass', protein_rate: 'Protein', visceral_fat: 'Visceral fat', bmr: 'BMR', body_balance_score: 'Body balance score',
    distance_meters: 'Distance', moving_seconds: 'Moving time', calories: 'Calories', avg_hr: 'Avg HR', max_hr: 'Max HR',
    min_hr: 'Min HR', total_steps: 'Steps', elevation_gain_m: 'Elevation gain', elevation_loss_m: 'Elevation loss',
    duration_minutes: 'Sleep duration', score: 'Sleep score', deep_minutes: 'Deep', light_minutes: 'Light', rem_minutes: 'REM',
    awake_minutes: 'Awake', wake_count: 'Wake-ups',
    unit_min: 'min', unit_s: 's', unit_score: 'pts', unit_count: 'times', unit_kcal: 'kcal', unit_m: 'm',
    unit_load: 'load', unit_steps: 'steps', unit_per_min: '/min', unit_s_per_km: 's/km',
  },
);

const lookup = (table: unknown, key: string): string | undefined => {
  const value = (table as Record<string, unknown>)[key];
  return typeof value === 'string' && value ? value : undefined;
};

export const metricLabel = (metric: string): string => lookup(messagesOf(metricMessages), metric) ?? metric;

const UNIT_KEYS: Record<string, string> = {
  min: 'unit_min', s: 'unit_s', score: 'unit_score', count: 'unit_count', kcal: 'unit_kcal',
  // The backend sends some units in Chinese (steps, minutes, meters, kcal, per minute, seconds per km).
  m: 'unit_m', load: 'unit_load', '\u6b65': 'unit_steps',
  '\u5206\u949f': 'unit_min', '\u7c73': 'unit_m', '\u5343\u5361': 'unit_kcal', '\u6b21/\u5206': 'unit_per_min', '\u79d2/\u516c\u91cc': 'unit_s_per_km',
};

export const unitLabel = (unit: string): string => {
  const key = UNIT_KEYS[unit];
  return (key && lookup(messagesOf(metricMessages), key)) || unit;
};

export const CATEGORY_METRICS: Readonly<Partial<Record<AiTaskCategory, readonly string[]>>> = {
  workout: ['distance_meters', 'moving_seconds', 'calories', 'avg_hr', 'max_hr', 'min_hr', 'training_load', 'vo2max', 'total_steps', 'elevation_gain_m', 'elevation_loss_m'],
  sleep: ['duration_minutes', 'score', 'deep_minutes', 'light_minutes', 'rem_minutes', 'awake_minutes', 'wake_count'],
  recovery: ['resting_hr', 'readiness', 'stress', 'respiratory_rate', 'sleep_hrv', 'hrv', 'hrv_rmssd', 'spo2'],
  resting_hr: ['resting_hr'],
  heart_rate: ['heart_rate'],
  training: ['training_load', 'vo2max', 'lactate_threshold_hr', 'lactate_threshold_pace', 'pai_daily', 'steps', 'active_calories', 'active_minutes', 'bmr'],
  body: ['blood_pressure_systolic', 'blood_pressure_diastolic', 'blood_pressure_pulse', 'weight', 'bmi', 'body_fat_rate', 'body_water_rate', 'muscle_mass', 'bone_mass', 'visceral_fat', 'bmr'],
};
