
export interface InsightComparison {
  baseline_value: number;
  delta: number;
  delta_percent: number;
  direction: 'higher' | 'lower' | 'same' | string;
}

export interface BaselineWindow {
  kind: 'comparable_runs' | 'previous_days' | string;
  days: number;
  min_samples: number;
  max_samples: number;
  distance_tolerance_percent?: number | null;
}

export type InsightConfidence = 'high' | 'medium' | 'low' | 'insufficient';

export interface InsightFact {
  fact_id: string;
  metric: string;
  value: number | null;
  unit: string;
  comparison: InsightComparison | null;
  baseline_window: BaselineWindow | null;
  evidence_count: number;
  source: string;
  confidence: InsightConfidence;
  reason: string | null;
  reason_code?: string | null;
  baseline_count?: number;
  evidence_refs: string[];
}

export interface BaselineEntry {
  workout_id: string;
  start_time: string;
  distance_meters: number;
}

export interface BaselineExclusion {
  workout_id: string;
  reason: string;
}

export interface HeartRateDrift {
  first_half_metres_per_beat: number;
  second_half_metres_per_beat: number;
  drift_percent: number;
  first_half_avg_hr: number;
  second_half_avg_hr: number;
  first_half_avg_speed_mps: number;
  second_half_avg_speed_mps: number;
  first_half_samples: number;
  second_half_samples: number;
  speed_cv: number;
}

export interface WorkoutInsight {
  workout_id: string;
  workout_type: string;
  supported: boolean;
  unsupported_reason: string | null;
  unsupported_code?: string | null;
  facts: InsightFact[];
  baseline_included: BaselineEntry[];
  baseline_excluded: BaselineExclusion[];
  heart_rate_drift?: HeartRateDrift | null;
  heart_rate_drift_unavailable?: string | null;
}

export interface WeeklyReport {
  generated_at: string;
  recent_start: string;
  recent_end: string;
  baseline_start: string;
  baseline_end: string;
  facts: InsightFact[];
}
