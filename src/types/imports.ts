
export type ImportSourceKey = 'gpx' | 'fit' | 'strava' | 'apple_health' | 'unstrap';

export interface UnstrapExportResult {
  path: string;
  workouts: number;
  heart_rate_points: number;
}

export interface ImportReport {
  batch_id: number | null;
  added: number;
  duplicates: number;
  unreadable: number;
  health_added: number;
}

export interface ImportBatch {
  id: number;
  source: ImportSourceKey;
  file_name: string | null;
  imported_at: string;
  added: number;
  duplicates: number;
  unreadable: number;
  health_added: number;
  undone_at: string | null;
  first_start: string | null;
  last_start: string | null;
}
