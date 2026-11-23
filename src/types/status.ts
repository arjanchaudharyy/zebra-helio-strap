
export type SourceScope = 'user_fused' | 'device' | 'unknown' | string;

export interface StreamStatus {
  stream: string;
  status: string;
  records?: number;
  last_sync?: string;
  last_cloud_sync_at?: string;
  newest_sample_at?: string;
  message?: string;
  needs_reauth?: boolean;
}

export interface CapabilityStatus {
  capability: string;
  available: boolean;
  reason?: string;
  reason_code?: string | null;
}

export interface AppStatus {
  /** Advanced-data credentials are locally readable; remote verification is `auth_state`. */
  configured: boolean;
  /** Saved metadata survives missing/denied credentials, keeping recovery controls visible. */
  auth_configuration_present?: boolean;
  /** Local credential availability; falls back to `configured` for older backends. */
  credential_available?: boolean;
  auth_state: string;
  connection_state: 'unconfigured' | 'configured' | 'connected' | 'needs_reauth' | string;
  data_source?: 'legacy' | 'official' | 'both' | 'none';
  masked_user_id?: string;
  region_host?: string;
  last_sync?: string;
  last_cloud_sync_at?: string;
  last_cloud_sync_outcome?: SyncOutcome;
  streams: StreamStatus[];
  capabilities: CapabilityStatus[];
  database_path?: string;
  retention_days: number;
  history_sync_days?: number;
  incremental_sync_days?: number;
  auto_sync_days?: number;
  storage?: StorageEstimate;
  coverage?: LocalCoverage;
  region_confidence?: 'identified' | 'hinted' | 'unconfirmed' | 'unknown' | string;
  compacting?: boolean;
  replaying?: boolean;
}

export interface LocalCoverage {
  earliest_day: string | null;
  latest_day: string | null;
  covered_days: number;
}

export interface StreamStorageEstimate {
  stream: string;
  observed_days: number;
  observed_bytes: number;
  bytes_per_day: number;
  measured: boolean;
  estimated_add_bytes: number;
}

export interface RawPayloadCompaction {
  compacted: number;
  skipped: number;
  bytesBefore: number;
  bytesAfter: number;
}

export interface StorageEstimate {
  free_bytes: number;
  estimated_add_bytes: number;
  database_bytes: number;
  allow_long_history: boolean;
  warn_tight_space: boolean;
  message: string;
  message_code?: string;
  requested_days: number;
  streams: StreamStorageEstimate[];
  measured: boolean;
  stop_reason: string | null;
  stop_reason_code?: string | null;
  needed_bytes?: number;
}

export interface UserPrefs {
  demo_mode?: boolean;
  ai_profile_note?: string;
  retention_days: number;
  history_sync_days: number;
  archive_enabled: boolean;
}

export interface SyncProgress {
  completed?: boolean;
  stream: string;
  current: number;
  total: number;
  message: string;
  code?: string;
  detail?: string | null;
  changed?: boolean;
}

export type LoginState = 'idle' | 'waiting' | 'extracting' | 'verifying' | 'connected' | 'failed';

export interface AccountBrief {
  user_id_masked: string;
  nickname: string | null;
}

export interface AccountPair {
  official: AccountBrief;
  advanced: AccountBrief;
}

export interface LoginStatus {
  credential_recovery?: { platform: string; can_save_file: boolean } | null;
  state: LoginState | string;
  message: string;
  page_url: string;
  code?: string;
  conflict?: AccountPair | null;
}

export interface OfficialStatus {
  /** Only public recovery options; authorization tokens remain in backend memory. */
  credential_recovery?: { platform: string; can_save_file: boolean } | null;
  state: 'idle' | 'waiting' | 'connected' | 'needs_reauth' | 'failed' | string;
  message_code: string | null;
  message: string | null;
  user_id_masked: string | null;
  nickname?: string | null;
  authorize_url?: string | null;
  connected_at: number | null;
  conflict?: AccountPair | null;
}

export interface SyncStreamResult {
  stream: string;
  status: string;
  records_written: number;
  message?: string;
  needs_reauth?: boolean;
  last_cloud_sync_at?: string;
  newest_sample_at?: string;
}

/**
 * `deferred` is not a failure: the library is replaying its stored raw
 * payloads after a normalizer upgrade and the sync stood aside rather than
 * fight it for the write lock. Nothing was lost and the caller retries.
 */
export type SyncOutcome =
  | 'updated'
  | 'no_new_data'
  | 'partial'
  | 'failed'
  | 'cancelled'
  | 'deferred'
  | 'cloud_stale';

export interface SyncReport {
  success: boolean;
  outcome: SyncOutcome;
  started_at: string;
  finished_at: string;
  last_cloud_sync_at: string;
  total_records: number;
  streams: SyncStreamResult[];
  message?: string;
  message_code?: string | null;
  notice_code?: string | null;
  cloud_latest_at?: string | null;
  content_changed?: boolean | null;
}

export interface LocalApiStatus {
  enabled: boolean;
  running: boolean;
  base_url: string;
  address: string;
  workout_series_path: string;
  token_present: boolean;
  error?: string | null;
  error_code?: string | null;
}
