
export interface StreamCoverage {
  stream: string;
  requested_chunks: number;
  persisted_chunks: number;
  empty_chunks: number;
  failed_chunks: number;
  pending_chunks: number;
  persisted_from: string | null;
  persisted_to: string | null;
  empty_months: string[];
  records: number;
}

export interface FailedChunk {
  stream: string;
  chunk_start: string;
  error: string | null;
  error_code?: string | null;
  attempts: number;
  exhausted: boolean;
}

export interface CoverageLedger {
  requested_from: string | null;
  requested_to: string | null;
  streams: StreamCoverage[];
  total_chunks: number;
  completed_chunks: number;
  complete: boolean;
  failed_chunks_detail: FailedChunk[];
  needs_manual_retry: boolean;
  months?: MonthSeam[];
}

export interface MonthSeam {
  month: string;
  state: 'gold' | 'crack' | 'local' | 'pending' | 'empty';
}

export type BackupKind = 'manual' | 'pre_migration' | 'pre_restore';

export interface BackupCoverage {
  earliest_sample_at: string | null;
  latest_sample_at: string | null;
  last_cloud_sync_at: string | null;
}

export interface BackupManifest {
  id: string;
  created_at: string;
  app_version: string;
  schema_version: number;
  normalizer_revision: string;
  kind: BackupKind;
  coverage: BackupCoverage;
  table_counts: Record<string, number>;
  bytes: number;
  sha256: string;
  integrity_ok: boolean;
  pinned: boolean;
}

export interface BackupVerification {
  id: string;
  file_present: boolean;
  bytes_match: boolean;
  sha256_match: boolean;
  integrity_ok: boolean;
  problem: string | null;
  problem_code?: string | null;
}

export type RestoreCompatibility =
  | 'same_schema'
  | 'older_schema_will_migrate'
  | 'future_schema_refused';

export interface RestorePreview {
  manifest: BackupManifest;
  verification: BackupVerification;
  compatibility: RestoreCompatibility;
  current_schema_version: number;
  current_table_counts: Record<string, number>;
  can_restore: boolean;
  blocker: string | null;
  blocker_code?: string | null;
}

export interface PendingRestore {
  backup_id: string;
  staged_at: string;
  rollback_backup_id: string;
}
