
export interface DeviceProfile {
  name?: string;
  canonical_name?: string;
  display_name?: string;
  name_code?: string;
  catalog_id?: string;
  kind?: 'watch' | 'strap' | 'ring' | 'band' | 'scale' | 'unknown' | string;
  image_key?: string | null;
  match_status?: 'exact' | 'alias' | 'user_assigned' | 'unknown';
  has_local_data?: boolean;
  last_data_at?: string | null;
  hidden?: boolean;
  firmware?: string;
  serial?: string;
  device_id?: string;
  timezone?: string;
}

export interface DeviceCacheMetadata {
  status: 'fresh' | 'stale' | 'missing' | 'refresh_failed' | 'unavailable' | string;
  cached_at?: string | null;
  age_seconds?: number | null;
  refreshed: boolean;
  refresh_error?: string | null;
  refresh_error_code?: string | null;
}

export interface DeviceProfilesResult {
  profiles: DeviceProfile[];
  hidden_profiles?: DeviceProfile[];
  cache: DeviceCacheMetadata;
}

/**
 * One row of the capability overview.
 *
 * `status` is not a boolean on purpose: the Zepp events endpoint answers
 * "200 with no items" for names that cannot exist, so missing data never
 * proves a device lacks a sensor. Only `unsupported` — an outright rejection —
 * licenses saying so.
 */
export interface CapabilityItem {
  stream: string;
  status: 'available' | 'no_records' | 'unsupported' | 'unknown' | string;
  records: number;
  recordsUnit: string;
  recordsUnitCode?: string;
  windowDays?: number;
  latestDate?: string | null;
  note?: string | null;
  source: 'derived' | 'probed' | string;
  ingested?: boolean;
}

export interface CapabilityOverview {
  items: CapabilityItem[];
  probedAt?: string | null;
}

/**
 * The result of asking the server whether one candidate stream exists.
 *
 * Which Zepp event streams answer depends on the account, the devices and the
 * region, and the endpoint has no discovery call — so availability is probed,
 * not assumed. A probe reports status and field names only; no measured value
 * is read and nothing is stored.
 */
export interface CapabilityProbe {
  stream: string;
  /** Which surface answered — the same event name behaves differently on each. */
  surface: 'v2_events' | 'user_events' | 'user_events_day' | string;
  /** How often the stream is measured; decides how far back the probe looks. */
  cadence: 'continuous' | 'episodic' | string;
  windowDays: number;
  eventType: string;
  subType: string;
  status: 'available' | 'empty' | 'unavailable' | 'error' | 'unrecognized';
  records: number;
  /** Newest item's calendar date — the answer for episodic metrics. */
  latestDate?: string | null;
  fields: string[];
  failure?: string | null;
}
