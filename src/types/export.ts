
export type ExportDataType =
  | 'blood_pressure'
  | 'life_events'
  | 'heart_rate'
  | 'sleep'
  | 'workouts'
  | 'steps'
  | 'spo2'
  | 'stress'
  | 'hrv'
  | 'hrv_rmssd'
  | 'respiratory_rate'
  | 'pai'
  | 'lactate_threshold'
  | 'training_load'
  | 'vo2max'
  | 'daily_activity'
  | 'recovery';

/**
 * How much of each stream an export carries.
 *
 * `summary` aggregates the two streams that dominate an export's size
 * (per-minute heart rate, per-second workout series) and keeps every
 * structured metric intact, so a month of data stays small enough to hand to a
 * model. `full` keeps the raw series and is what the FIT export uses.
 */
export type ExportDetail = 'summary' | 'full';

export type ExportScope =
  | { kind: 'dateRange'; start: string; end: string }
  | { kind: 'workout'; workoutId: string };

export interface ExportSelection {
  scope?: ExportScope;
  startDate?: string;
  endDate?: string;
  dataTypes: ExportDataType[];
  detail?: ExportDetail;
}

export interface ExportResult {
  path: string;
  record_count: number;
  bytes: number;
  generated_at: string;
  file_count?: number;
}

export type AiHandoffMode = 'inline' | 'attachment';

export interface AiHandoffMetadata {
  preciseRouteIncluded: boolean;
  authenticationFieldsRemoved: boolean;
  identityFieldsRemoved: boolean;
}

export interface AiHandoffResult {
  mode: AiHandoffMode;
  clipboardText: string;
  filePath?: string;
  bytes: number;
  records: number;
  redactions: string[];
  metadata: AiHandoffMetadata;
}

export interface ReprocessResult {
  total_records: number;
  streams: Record<string, number>;
  message: string;
}
