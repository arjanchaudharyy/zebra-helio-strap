//! Pushes to the user's own PA site (the ZeppBridge-Cloud Worker). Ported from
//! `ZeppBridge-Cloud/uploader/upload.mjs`: read-only fetch -> strip tracks -> split -> gzip -> encrypt -> PUT.
//!
//! The object layout matches the component: `days/<day>.json`, `workouts/<id>.json` (15-second samples) and the optional
//! `.full.json`, `summary.json`, `contract.json`, `index.json`, `status.json` (**sent last**, so
//! a reader who sees a new status already has the other objects). Workouts that leave the retention window, or are deleted or hidden locally,
//! are deleted in the cloud too.
//!
//! One difference from the component: the manifest records a content hash per object (without timestamps such as `generated_at`).
//! **Unchanged content is not sent again**; if no object changed, no request is sent.
//!
//! Not ported yet: the profile / context / plan /
//! baselines / data-health archive objects, the live token handoff, UI snapshots.

pub mod crypto;
pub mod summary;

use super::client::{self, SiteUrl};
use crate::models::error::{Result, ZeppBridgeError};
use crate::models::ExportDetail;
use crate::records::{self, RecordQuery};
use crate::storage::{Database, EXPORT_DATA_TYPES};
use chrono::{DateTime, Duration as ChronoDuration, Local, NaiveDate, Utc};
use flate2::{write::GzEncoder, Compression};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;

const SAMPLE_INTERVAL_MS: i64 = 15_000;

const SUMMARY_TYPES: [&str; 13] = [
    "daily_activity",
    "sleep",
    "workouts",
    "heart_rate",
    "hrv",
    "pai",
    "recovery",
    "steps",
    "stress",
    "training_load",
    "vo2max",
    "respiratory_rate",
    "spo2",
];

/// The user's options (no keys).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaOptions {
    pub retention_days: u32,
    pub summary_days: u32,
    pub full_resolution_workouts: bool,
}

/// The three values stored in the credential store (JSON). Used only in memory; never written to disk or logs.
#[derive(Clone, Deserialize, Serialize)]
pub struct PaSecrets {
    pub tenant: Option<String>,
    pub dk: String,
    #[serde(rename = "writeKey")]
    pub write_key: String,
}

impl std::fmt::Debug for PaSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PaSecrets(<redacted>)")
    }
}

/// The decoded and checked keys.
pub struct PaKeys {
    pub tenant: String,
    pub dk: Vec<u8>,
    pub write_key: String,
}

impl PaSecrets {
    pub fn validate(&self) -> std::result::Result<PaKeys, &'static str> {
        let (kind, tenant) =
            crypto::parse_key(&self.write_key).ok_or("err.push.pa_write_key_malformed")?;
        if kind != crypto::KeyKind::Write {
            return Err("err.push.pa_write_key_malformed");
        }
        if self.tenant.as_deref().is_some_and(|given| given != tenant) {
            return Err("err.push.pa_tenant_mismatch");
        }
        let dk = crypto::from_b64url(&self.dk).ok_or("err.push.pa_data_key_malformed")?;
        if dk.len() != 32 {
            return Err("err.push.pa_data_key_malformed");
        }
        Ok(PaKeys {
            tenant,
            dk,
            write_key: self.write_key.clone(),
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkoutEntry {
    pub date: String,
    pub workout_type: Option<String>,
    pub start_time: Option<String>,
    pub has_full: bool,
}

/// The push manifest: `data/push/<target>.manifest.json`. Fields follow the component's manifest, plus `objects`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default = "manifest_version")]
    pub version: u32,
    #[serde(default)]
    pub destination: Option<String>,
    /// Day -> time of the first upload.
    #[serde(default)]
    pub days: BTreeMap<String, String>,
    #[serde(default)]
    pub workouts: BTreeMap<String, WorkoutEntry>,
    #[serde(default)]
    pub full_resolution: bool,
    /// Relative path -> content hash of the last upload.
    #[serde(default)]
    pub objects: BTreeMap<String, String>,
    #[serde(default)]
    pub last_seen_cloud_sync_at: Option<String>,
}

fn manifest_version() -> u32 {
    1
}

/// What to send in this round.
#[derive(Debug, Default)]
pub struct Plan {
    /// The objects to PUT: relative path and plaintext JSON. `status.json` is always last.
    pub outgoing: Vec<(String, String)>,
    pub deletes: BTreeSet<String>,
    /// The number of objects skipped in this round because their content did not change.
    pub unchanged: usize,
    /// The manifest to write back after success.
    pub manifest: Manifest,
    pub days_considered: usize,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.outgoing.is_empty() && self.deletes.is_empty()
    }
}

/// The local status fields that `status.json` uses (same source as `zeppbridge-cli status --json`).
#[derive(Debug, Clone, Default)]
pub struct LocalStatus {
    pub last_cloud_sync_at: Option<String>,
    pub latest_day: Option<String>,
    pub latest_workout_at: Option<String>,
}

impl LocalStatus {
    pub fn read(db: &Database) -> Self {
        let health = db.data_health(30, 0).ok();
        let coverage = db
            .local_coverage(Local::now().date_naive())
            .unwrap_or_default();
        let latest = db.get_recent_workouts(1).unwrap_or_default();
        Self {
            last_cloud_sync_at: health.and_then(|health| health.timings.last_cloud_sync_at),
            latest_day: coverage.latest_day,
            latest_workout_at: latest
                .first()
                .map(|workout| workout.start_time.to_rfc3339()),
        }
    }
}

/// `n` days back from today (including today), ascending.
pub fn days_back(today: NaiveDate, n: u32) -> Vec<String> {
    (0..i64::from(n))
        .rev()
        .map(|offset| {
            (today - ChronoDuration::days(offset))
                .format("%Y-%m-%d")
                .to_string()
        })
        .collect()
}

/// Keeps one real sample every 15 seconds (no averaging, no interpolation).
pub fn every_15s(samples: &[Value]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut next = i64::MIN;
    for sample in samples {
        let Some(moment) = sample
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
        else {
            continue;
        };
        let at = moment.timestamp_millis();
        if at < next {
            continue;
        }
        out.push(sample.clone());
        next = at + SAMPLE_INTERVAL_MS;
    }
    out
}

fn workout_object(
    envelope: &Value,
    workout: &Value,
    samples: Vec<Value>,
    resolution: &str,
) -> Value {
    let mut rest = workout.clone();
    if let Some(object) = rest.as_object_mut() {
        object.remove("route");
        object.insert("samples".into(), Value::Array(samples));
    }
    json!({
        "schema_version": envelope.get("schema_version"),
        "records_version": envelope.get("records_version"),
        "generated_at": envelope.get("generated_at"),
        "provenance": envelope.get("provenance"),
        "devices": envelope.get("devices"),
        "resolution": resolution,
        "samples_note": if resolution == "15s" {
            "One real sample kept every 15 s (not averaged, not interpolated). sample_count is the original per-second count."
        } else {
            "Every stored sample at original resolution."
        },
        "route_note": "GPS routes are never uploaded.",
        "workout": rest,
    })
}

/// The content hash: drop the timestamp fields that change every time; the rest is the "content".
pub fn content_hash(value: &Value) -> String {
    let mut stripped = value.clone();
    if let Some(object) = stripped.as_object_mut() {
        for key in ["generated_at", "last_upload_at"] {
            object.remove(key);
        }
    }
    crypto::sha256_hex(&serde_json::to_string(&stripped).unwrap_or_default())
}

fn day_query(types: &[&str], from: &str, to: &str, detail: ExportDetail) -> RecordQuery {
    let mut query = RecordQuery::range(types, from, to, None);
    query.detail = detail;
    // PA never receives tracks; it stays off here, and a hard coordinate check runs before sending.
    query.include_gps = false;
    query
}

/// Reads the local database and computes what to send in this round. Read-only: no write lock, no database change.
pub fn build_plan(
    db: &Database,
    options: &PaOptions,
    previous: &Manifest,
    today: NaiveDate,
    status: &LocalStatus,
    now: DateTime<Utc>,
    time_zone: &str,
) -> Result<Plan> {
    let mut manifest = previous.clone();
    manifest.version = 1;
    let window = days_back(today, options.retention_days);
    // One snapshot covers the whole retention window, including historical imports
    // and local edits. Hash each object immediately and retain only changed payloads.
    let snapshot = db.conn.unchecked_transaction()?;
    let mut plan = Plan {
        days_considered: window.len(),
        ..Default::default()
    };
    let mut objects = std::mem::take(&mut manifest.objects);
    let mut queue = |rel: String, value: Value| -> Result<()> {
        let hash = content_hash(&value);
        if objects.get(&rel) == Some(&hash) {
            plan.unchanged += 1;
        } else {
            let text = serde_json::to_string(&value)
                .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?;
            objects.insert(rel.clone(), hash);
            plan.outgoing.push((rel, text));
        }
        Ok(())
    };
    let mut deletes: BTreeSet<String> = BTreeSet::new();
    let now_text = now.to_rfc3339();

    let all_types: Vec<&str> = EXPORT_DATA_TYPES.to_vec();
    let mut seen = BTreeSet::new();
    let mut workout_days = Vec::new();
    for day in &window {
        let set =
            records::query_records(db, &day_query(&all_types, day, day, ExportDetail::Summary))?;
        if set.value["data"]["workouts"]
            .as_array()
            .is_some_and(|records| !records.is_empty())
        {
            workout_days.push(day);
        }
        queue(format!("days/{day}.json"), set.value)?;
    }

    for day in workout_days {
        let envelope =
            records::query_records(db, &day_query(&["workouts"], day, day, ExportDetail::Full))?
                .value;
        let empty = Vec::new();
        let workouts = envelope
            .get("data")
            .and_then(|data| data.get("workouts"))
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        for workout in workouts {
            let id = match workout.get("workout_id") {
                Some(Value::String(text)) => text.clone(),
                Some(other) => other.to_string(),
                None => continue,
            };
            seen.insert(id.clone());
            let samples = workout
                .get("samples")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            queue(
                format!("workouts/{id}.json"),
                workout_object(&envelope, workout, every_15s(&samples), "15s"),
            )?;
            let has_full = options.full_resolution_workouts && !samples.is_empty();
            if has_full {
                queue(
                    format!("workouts/{id}.full.json"),
                    workout_object(&envelope, workout, samples, "full"),
                )?;
            } else if manifest
                .workouts
                .get(&id)
                .is_some_and(|entry| entry.has_full)
            {
                // Check the old manifest before overwriting: otherwise, after per-second is turned off, recently re-imported workouts would keep their per-second copies.
                deletes.insert(format!("workouts/{id}.full.json"));
            }
            manifest.workouts.insert(
                id,
                WorkoutEntry {
                    date: day.clone(),
                    workout_type: workout
                        .get("workout_type")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    start_time: workout
                        .get("start_time")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    has_full,
                },
            );
        }
    }

    // Per-second option off: the per-second copies already in the cloud must be deleted, not only no longer uploaded.
    if !options.full_resolution_workouts {
        for (id, entry) in manifest.workouts.iter_mut() {
            if entry.has_full {
                deletes.insert(format!("workouts/{id}.full.json"));
                entry.has_full = false;
            }
        }
    }

    // Delete what left the retention window.
    let stale_days: Vec<String> = manifest
        .days
        .keys()
        .filter(|day| window.binary_search(day).is_err())
        .cloned()
        .collect();
    for day in stale_days {
        deletes.insert(format!("days/{day}.json"));
        manifest.days.remove(&day);
    }
    let stale_workouts: Vec<String> = manifest
        .workouts
        .iter()
        .filter(|(id, _)| !seen.contains(*id))
        .map(|(id, _)| id.clone())
        .collect();
    for id in stale_workouts {
        deletes.insert(format!("workouts/{id}.json"));
        deletes.insert(format!("workouts/{id}.full.json"));
        manifest.workouts.remove(&id);
    }

    // The read-only API for the assistant: daily summaries for the last N days + the reading contract.
    let summary_count = options.summary_days.min(options.retention_days) as usize;
    let summary_days: Vec<String> = window[window.len().saturating_sub(summary_count)..].to_vec();
    if let (Some(first), Some(last)) = (summary_days.first(), summary_days.last()) {
        let envelope = records::query_records(
            db,
            &day_query(&SUMMARY_TYPES, first, last, ExportDetail::Summary),
        )?
        .value;
        queue(
            "summary.json".into(),
            summary::build_summary(&envelope, &summary_days, time_zone, &now_text),
        )?;
    }
    queue("contract.json".into(), crate::contract::contract_json())?;

    for day in &window {
        manifest
            .days
            .entry(day.clone())
            .or_insert_with(|| now_text.clone());
    }
    manifest.full_resolution = options.full_resolution_workouts;
    manifest.last_seen_cloud_sync_at = status.last_cloud_sync_at.clone();

    let mut index: Vec<Value> = manifest
        .workouts
        .iter()
        .map(|(id, entry)| {
            json!({
                "workout_id": id,
                "date": entry.date,
                "workout_type": entry.workout_type,
                "start_time": entry.start_time,
                "has_full": entry.has_full,
            })
        })
        .collect();
    index.sort_by(|a, b| {
        let key = |value: &Value| value["start_time"].as_str().unwrap_or_default().to_string();
        key(b).cmp(&key(a))
    });
    queue(
        "index.json".into(),
        json!({
            "retention_days": options.retention_days,
            "days": manifest.days.keys().collect::<Vec<_>>(),
            "workouts": index,
        }),
    )?;
    snapshot.commit()?;
    let status_value = json!({
        "schema": "zeppbridge-cloud/1",
        "last_upload_at": now_text,
        "last_cloud_sync_at": status.last_cloud_sync_at,
        "latest_day": status.latest_day,
        "latest_workout_at": status.latest_workout_at,
        "sync_error": Value::Null,
        "sync": { "skipped": true, "note": "pushed by ZeppBridge itself after its own sync" },
        "full_resolution_workouts": options.full_resolution_workouts,
        "retention_days": options.retention_days,
        "summary_days": summary_count,
        "time_zone": time_zone,
        "app_version": env!("CARGO_PKG_VERSION"),
        "uploader": "zeppbridge",
        "time_note": "last_upload_at = when this computer uploaded; last_cloud_sync_at = when it last pulled from the Zepp cloud; each record carries its own sample time.",
    });

    for rel in &deletes {
        objects.remove(rel);
    }
    manifest.objects = objects;
    // Add status.json, at the end, only when content changed (or something must be deleted); with no change, send no request.
    if !plan.outgoing.is_empty() || !deletes.is_empty() {
        let text = serde_json::to_string(&status_value)
            .map_err(|error| ZeppBridgeError::ParseError(error.to_string()))?;
        manifest
            .objects
            .insert("status.json".into(), content_hash(&status_value));
        plan.outgoing.push(("status.json".into(), text));
    }
    plan.deletes = deletes;
    plan.manifest = manifest;
    Ok(plan)
}

/// A hard check before sending: any coordinate field aborts the whole run, and no byte is sent.
pub fn check_no_coordinates(plan: &Plan) -> std::result::Result<(), String> {
    for (rel, text) in &plan.outgoing {
        if let Some(field) = crypto::find_coordinate_field(text) {
            return Err(format!(
                "coordinate field \"{field}\" found in {rel}; aborted, nothing uploaded"
            ));
        }
    }
    Ok(())
}

pub fn gzip(text: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    // Writing into a Vec cannot fail.
    let _ = encoder.write_all(text.as_bytes());
    encoder.finish().unwrap_or_default()
}

/// The result of one upload round (only counts and bytes).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UploadStats {
    pub objects: usize,
    pub deleted: usize,
    pub unchanged: usize,
    pub bytes_plain: usize,
    pub bytes_sent: usize,
}

/// Compress -> encrypt -> send. Deletes first, then uploads; `status.json` last.
pub async fn upload(
    site: &SiteUrl,
    keys: &PaKeys,
    plan: &Plan,
) -> std::result::Result<UploadStats, String> {
    let client = client::build_client()?;
    let mut stats = UploadStats {
        unchanged: plan.unchanged,
        ..Default::default()
    };
    for rel in &plan.deletes {
        client::send(
            &client,
            reqwest::Method::DELETE,
            &format!("{}/objects/{rel}", site.api),
            &keys.write_key,
            "application/octet-stream",
            None,
            &format!("DELETE {rel}"),
        )
        .await?;
        stats.deleted += 1;
    }
    for (rel, text) in &plan.outgoing {
        let gz = gzip(text);
        let sealed = crypto::encrypt_object(&keys.dk, &crypto::object_key(&keys.tenant, rel), &gz)
            .map_err(|_| "encryption failed".to_string())?;
        stats.bytes_plain += text.len();
        stats.bytes_sent += sealed.len();
        client::send(
            &client,
            reqwest::Method::PUT,
            &format!("{}/objects/{rel}", site.api),
            &keys.write_key,
            "application/octet-stream",
            Some(sealed),
            &format!("PUT {rel}"),
        )
        .await?;
        stats.objects += 1;
    }
    Ok(stats)
}

#[cfg(test)]
mod tests;
