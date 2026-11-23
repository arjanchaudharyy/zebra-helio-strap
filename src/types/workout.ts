import type { SourceScope } from './status';


export interface HeartRateZoneBucket {
  index: number;
  upper_bound_bpm: number;
  seconds: number;
}

export interface Workout {
  workout_id: string;
  /** Backwards-compatible normalized type. */
  workout_type: string;
  normalized_type: string;
  type_source: 'numeric_mapped' | 'unknown_code' | 'string_field' | 'missing' | string;
  user_override?: string | null;
  effective_type: string;
  custom_label?: string | null;
  zepp_type?: number | null;
  zepp_source?: string | null;
  start_time: string;
  end_time: string;
  distance_meters?: number;
  /** Cloud run_time in seconds; excludes pauses. */
  moving_seconds?: number | null;
  calories?: number;
  avg_hr?: number;
  max_hr?: number;
  min_hr?: number | null;
  elevation_gain_m?: number | null;
  elevation_loss_m?: number | null;
  max_altitude_m?: number | null;
  min_altitude_m?: number | null;
  training_load?: number;
  vo2max?: number;
  training_effect?: number | null;
  anaerobic_training_effect?: number | null;
  rpe?: number | null;
  avg_cadence_spm?: number | null;
  max_cadence_spm?: number | null;
  avg_stride_cm?: number | null;
  gear_id?: string | null;
  total_steps?: number | null;
  gps_available?: boolean;
  sample_count?: number;
  source_scope: SourceScope;
  device_id?: string;
  synced_at?: string | null;
  hr_zones?: HeartRateZoneBucket[];
}

export interface SportOption {
  key: string;
  label: string;
}

export interface WorkoutCodeLabel {
  zeppType: number;
  label: string;
  records: number;
  updatedAt: string;
}

export interface WorkoutRoutePoint {
  segment_start?: boolean;
  timestamp: string;
  latitude: number;
  longitude: number;
  altitude_m?: number | null;
}

export interface WorkoutSeriesSample {
  timestamp: string;
  heart_rate?: number | null;
  speed?: number | null;
  pace?: number | null;
  cadence?: number | null;
  stride_cm?: number | null;
  altitude_m?: number | null;
  /** Running power in watts, verified against the summary's average/max. */
  power_watts?: number | null;
  /** Ground contact time in milliseconds. */
  ground_contact_ms?: number | null;
  /** Vertical oscillation in millimetres. */
  vertical_oscillation_mm?: number | null;
  /** Vertical stride ratio in percent. */
  vertical_ratio_pct?: number | null;
  /** Grade-adjusted equivalent pace in seconds per kilometre. */
  equivalent_pace_s_per_km?: number | null;
}

export interface WorkoutPause {
  start_time: string;
  end_time: string;
  kind: string;
}

export type StrengthWeightUnit = 'kg' | 'lb';

export interface StrengthSetRow {
  set_index: number;
  action_code: number;
  exercise_key?: string | null;
  muscle_group?: string | null;
  reps: number;
  weight_value?: number | null;
  weight_unit: StrengthWeightUnit;
  weight_kg?: number | null;
  duration_ms?: number | null;
  avg_hr?: number | null;
  calories_kcal?: number | null;
  exercise_name?: string | null;
  secondary_muscle_groups?: string[];
}

export interface StrengthExerciseGroup {
  action_code: number;
  exercise_key?: string | null;
  muscle_group?: string | null;
  sets: number;
  total_reps: number;
  exercise_name?: string | null;
  secondary_muscle_groups?: string[];
}

export interface StrengthCatalogAction {
  action_code: number;
  name: string;
  primary_groups: string[];
  secondary_groups: string[];
  main_positions: number[];
  sub_positions: number[];
  self_weight?: boolean | null;
}

export interface StrengthCatalogResult {
  lang: string;
  fetched_at: string | null;
  status: 'fresh' | 'stale' | 'unavailable';
  actions: StrengthCatalogAction[];
}

export interface StrengthWorkoutDetail {
  total_sets: number;
  total_reps: number;
  total_volume_kg?: number | null;
  rest_seconds?: number | null;
  sets: StrengthSetRow[];
  exercise_groups: StrengthExerciseGroup[];
}

export interface WorkoutSeries {
  workout_id: string;
  samples: WorkoutSeriesSample[];
  route: WorkoutRoutePoint[];
  pauses: WorkoutPause[];
  splits: WorkoutSplitRow[];
  laps: WorkoutLapRow[];
  summary: WorkoutSeriesSummary;
  climbs?: WorkoutClimbs | null;
  strength?: StrengthWorkoutDetail | null;
}

export interface ClimbMethod {
  smoothing_window_s: number;
  reversal_m: number;
  min_change_m: number;
  min_grade_pct: number;
  moving_speed_m_s: number;
}

export interface ClimbSegment {
  kind: 'climb' | 'descent';
  start_time: string;
  end_time: string;
  start_distance_m: number;
  end_distance_m: number;
  elevation_change_m: number;
  average_grade_pct: number;
  moving_seconds: number;
  vertical_speed_m_per_h: number | null;
  average_hr: number | null;
  average_pace_min_per_km: number | null;
}

export interface WorkoutClimbs {
  method: ClimbMethod;
  segments: ClimbSegment[];
}

/** Laps recorded by the watch, separate from computed kilometre splits. */
export interface WorkoutLapRow {
  index: number;
  start_time: string;
  end_time: string;
  distance_m: number;
  duration_seconds: number;
  avg_hr: number | null;
  max_hr: number | null;
  /** Mean of per-point cadence inside the lap (steps/min); null without samples. */
  avg_cadence_spm?: number | null;
}

/** One kilometre of a workout, cut from the server's cumulative distance. */
export interface WorkoutSplitRow {
  index: number;
  start_time: string;
  end_time: string;
  distance_m: number;
  duration_seconds: number;
  pace_min_per_km?: number | null;
  avg_hr?: number | null;
  max_hr?: number | null;
  elevation_gain_m?: number | null;
  elevation_loss_m?: number | null;
  /** A trailing partial kilometre, never to be read as a slow full one. */
  partial: boolean;
  /** Mean of per-point cadence inside the split (steps/min); null without samples. */
  avg_cadence_spm?: number | null;
}

export interface WorkoutSeriesSummary {
  average_pace?: number | null;
  average_cadence?: number | null;
  max_cadence?: number | null;
  average_stride_cm?: number | null;
  elevation_gain_m?: number | null;
  elevation_loss_m?: number | null;
  average_power_watts?: number | null;
  max_power_watts?: number | null;
  average_ground_contact_ms?: number | null;
  average_vertical_oscillation_mm?: number | null;
  average_vertical_ratio_pct?: number | null;
  /** The fastest equivalent pace in the series, in seconds per kilometre. */
  best_equivalent_pace_s_per_km?: number | null;
}

/**
 * One measured number a zone model can stand on.
 *
 * Every entry names where it came from and when it was measured. There is
 * deliberately no 220−age estimate: this list is measurements only.
 */
export interface HeartRateBasis {
  id: string;
  kind: 'max_hr' | 'resting_hr' | 'threshold_hr' | string;
  label: string;
  value: number;
  unit: string;
  source: string;
  measuredAt?: string | null;
  note?: string | null;
  noteCount?: number | null;
}

export interface HeartRateZoneBand {
  zone: number;
  label: string;
  lowPercent: number;
  highPercent: number;
}

export interface HeartRateZoneModel {
  id: 'max_hr' | 'hr_reserve' | 'lactate_threshold' | string;
  label: string;
  formula: string;
  requires: string[];
  bands: HeartRateZoneBand[];
  /** False when the library holds no basis of a required kind. */
  available: boolean;
}

export interface HeartRateZoneRow {
  zone: number;
  label: string;
  minBpm: number;
  maxBpm: number;
  seconds: number;
}

/** Every field starts empty: no model is chosen on the user's behalf. */
export interface HeartRateZonePreference {
  model?: string | null;
  maxBasis?: string | null;
  restingBasis?: string | null;
  thresholdBasis?: string | null;
}

export interface HeartRateZoneReport {
  model: string;
  modelLabel: string;
  formula: string;
  bases: HeartRateBasis[];
  zones: HeartRateZoneRow[];
  belowZone1Seconds: number;
  aboveZone5Seconds: number;
  totalSeconds: number;
  windowDays: number;
  source: string;
}

export interface HeartRateZoneOptions {
  bases: HeartRateBasis[];
  models: HeartRateZoneModel[];
  preference: HeartRateZonePreference;
  /** Present only once the preference names a model and its bases. */
  report?: HeartRateZoneReport | null;
  windowDays: number;
}

export interface WorkoutFitOverride {
  workoutId: string;
  fileName: string | null;
  sha256: string;
  byteLen: number;
  importedAt: string;
  distanceM: number | null;
  movingSeconds: number | null;
  originalDistanceM: number | null;
  originalMovingSeconds: number | null;
}
