
export interface StageState {
  state: 'ok' | 'failed' | 'never' | string;
  at?: string | null;
  last_ok_at?: string | null;
  error_kind?: string | null;
  message?: string | null;
}

export interface SourceBreakdown {
  source: 'device' | 'user_fused' | 'unknown' | string;
  records: number;
}

export interface CoverageExplanation {
  kind: 'gaps' | 'observations' | string;
  window_days: number;
  observed_days: number;
  gap_dates: string[];
  gap_total: number;
  first_observed_at?: string | null;
  latest_observed_at?: string | null;
  note: string;
}

export interface StreamHealth {
  stream: string;
  label: string;
  cadence: 'continuous' | 'daily' | 'nightly' | 'per_event' | 'occasional' | string;
  fetch: StageState;
  parse: StageState;
  write: StageState;
  raw_records: number;
  canonical_records: number;
  last_written_records: number;
  sources: SourceBreakdown[];
  coverage: CoverageExplanation;
}

export interface IntegrityCheckResult {
  checked_at: string;
  ok: boolean;
  detail?: string | null;
}

export interface DatabaseHealth {
  schema_version: number;
  normalizer_revision: string;
  stored_normalizer_revision?: string | null;
  normalizer_replay_pending?: boolean;
  replay_in_progress: boolean;
  database_bytes: number;
  raw_records: number;
  canonical_records: number;
  pending_normalization: number;
  last_integrity_check?: IntegrityCheckResult | null;
}

export interface HealthTimings {
  last_cloud_sync_at?: string | null;
  last_cloud_sync_outcome?: string | null;
  last_local_replay_at?: string | null;
  last_manual_reprocess_at?: string | null;
  newest_sample_at?: string | null;
}

export interface HealthAction {
  id: string;
  code?: string;
  label: string;
  reason: string;
  destructive: boolean;
}

export interface DataHealth {
  generated_at: string;
  database: DatabaseHealth;
  timings: HealthTimings;
  streams: StreamHealth[];
  occasional_metrics: StreamHealth[];
  actions: HealthAction[];
}
