use crate::{models, storage::StreamFreshness, sync};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A stream status shaped for the TypeScript IPC contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamStatusView {
    pub stream: String,
    pub status: String,
    pub records: Option<i64>,
    pub last_sync: Option<String>,
    pub last_cloud_sync_at: Option<String>,
    pub newest_sample_at: Option<String>,
    pub message: Option<String>,
    pub needs_reauth: Option<bool>,
}

/// A capability status shaped for the TypeScript IPC contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityStatusView {
    pub capability: String,
    pub available: bool,
    pub reason: Option<String>,
    /// Stable code for the `reason` text. Empty means the reason is a raw message passed through from the backend,
    /// and the UI can only show it as is.
    #[serde(default)]
    pub reason_code: Option<String>,
}

/// Overall application status exposed to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppStatus {
    /// Advanced-data credential is locally readable and passes format checks.
    pub configured: bool,
    /// Saved advanced-data metadata exists, even when its credential is inaccessible.
    pub auth_configuration_present: bool,
    /// Same local availability as `configured`; separate from remote verification.
    pub credential_available: bool,
    pub auth_state: String,
    pub connection_state: String,
    /// `legacy` / `official` / `both` / `none`: which channels are connected. With only the official one, the UI must say which metrics
    /// need the "advanced data" connection (official has no HRV, SpO2, stress, readiness, and so on).
    pub data_source: String,
    pub masked_user_id: Option<String>,
    pub region_host: Option<String>,
    pub last_sync: Option<String>,
    pub last_cloud_sync_at: Option<String>,
    pub last_cloud_sync_outcome: Option<String>,
    pub streams: Vec<StreamStatusView>,
    pub capabilities: Vec<CapabilityStatusView>,
    pub database_path: Option<String>,
    pub retention_days: i64,
    pub history_sync_days: i64,
    /// How many days back one incremental sync fetches.
    ///
    /// The N in the UI text "Syncing the last N days" comes from here. **Do not hard-code it in the frontend.** When it
    /// changed from 7 to 30, only the backend changed, and the UI said 7 for one full release.
    pub incremental_sync_days: i64,
    /// How many days back this silent scheduled sync fetches: when a full-window refresh is due, it is
    /// `incremental_sync_days`, otherwise `QUICK_SYNC_DAYS`. Do not hard-code this in the frontend either.
    pub auto_sync_days: i64,
    pub storage: Option<crate::models::StorageEstimate>,
    /// The date range that has data on this device. Every "last N days" picker in the UI reads the local
    /// database, so each one needs this. Otherwise it draws "not in the database" as "you did not move in those months".
    pub coverage: crate::storage::LocalCoverage,
    /// Why the current `region_host` is taken to belong to this account: `identified` / `hinted` /
    /// `unconfirmed` / `unknown`. The UI writes its own sentence from this code. The backend does not write
    /// locale text, so all four outputs say the same thing.
    ///
    /// `unconfirmed` is the only level that needs a warning to the user: the region was guessed from the fallback list, and
    /// nothing proves that it belongs to this account. If a sync returns no records, this is the only
    /// clue the user can see.
    pub region_confidence: String,
    /// Whether the background is compacting historical payloads.
    ///
    /// One `compaction://started` event is not enough: Rust sends it in setup(),
    /// but the frontend starts to listen only at onMounted. Nobody listens when the event is sent,
    /// so the UI shows only a "sync deferred" hint and nothing explains why.
    /// The state can be read at any time. The event cannot.
    pub compacting: bool,
    /// Whether the background is replaying derived data from local raw payloads. Same as `compacting`: the frontend usually misses the startup event,
    /// but the state can be read at any time. During replay, cloud sync defers (`deferred`).
    pub replaying: bool,
}

/// The Zepp account that one connection links to: only the last four digits of the ID show. Only the official authorization side has a nickname.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountBrief {
    pub user_id_masked: String,
    pub nickname: Option<String>,
}

/// When the official authorization and advanced data are rejected as "not the same account", the account on each side.
///
/// For error reports only: "not the same account" alone does not tell the user which side is wrong. The official authorization step
/// uses the Zepp account already signed in to the browser, and the page does not show who it is.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountPair {
    pub official: AccountBrief,
    pub advanced: AccountBrief,
}

/// Only the last four digits of the ID show. Side by side, that is enough to tell if they are the same account.
pub fn mask_user_id(user_id: &str) -> String {
    let tail: String = user_id
        .trim()
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••{tail}")
}

/// Web-login progress exposed to the frontend and the `login://status` event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoginStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_recovery: Option<crate::commands::credential_recovery::RecoveryStatus>,
    pub state: String,
    pub message: String,
    pub page_url: String,
    /// Stable code for the `message` text. The UI uses it to get text in its own language, and falls back to
    /// the original message only if that fails. Login is the step where users get stuck most, so the text must not be in one language only.
    #[serde(default)]
    pub code: String,
    /// When it is not the same account as the official authorization, the account on each side.
    #[serde(default)]
    pub conflict: Option<AccountPair>,
}

impl LoginStatus {
    pub fn idle() -> Self {
        Self {
            credential_recovery: None,
            state: "idle".to_string(),
            message: String::new(),
            page_url: String::new(),
            code: String::new(),
            conflict: None,
        }
    }

    pub fn new(
        state: &str,
        code: &str,
        message: impl Into<String>,
        page_url: impl Into<String>,
    ) -> Self {
        Self {
            credential_recovery: None,
            state: state.to_string(),
            message: message.into(),
            page_url: page_url.into(),
            code: code.to_string(),
            conflict: None,
        }
    }
}

/// A stream result shaped for the TypeScript sync-report contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiSyncStreamResult {
    pub stream: String,
    pub status: String,
    pub records_written: i64,
    pub message: Option<String>,
    pub needs_reauth: Option<bool>,
    pub last_cloud_sync_at: Option<String>,
    pub newest_sample_at: Option<String>,
}

/// A sync report shaped for the TypeScript IPC contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UiSyncReport {
    pub success: bool,
    pub outcome: String,
    pub started_at: String,
    pub finished_at: String,
    pub last_cloud_sync_at: String,
    pub total_records: i64,
    pub streams: Vec<UiSyncStreamResult>,
    pub message: Option<String>,
    /// Stable code for the `message` text. The UI uses it to get text in its own language, and falls back to
    /// the original `message` text only if that fails. The backend does not write text per UI language.
    #[serde(default)]
    pub message_code: Option<String>,
    /// A warning next to the result (`ui.sync.*`). There is only one now: sync succeeded, but old data cleanup
    /// failed. Before, it was only appended to `message`, and the UI reads `message` only when deferred, so the user never
    /// saw it.
    #[serde(default)]
    pub notice_code: Option<String>,
    /// When the result is `cloud_stale` (cloud probe before sync): the time of the newest cloud record. The UI shows "Cloud latest 08:12".
    #[serde(default)]
    pub cloud_latest_at: Option<String>,
    /// Whether this round really changed health data in the database (whether `storage::content_epoch` moved).
    /// `total_records` cannot tell: a payload fetched again unchanged still reports the count written last time.
    /// The UI uses this to decide whether to invalidate page read caches. When absent (cancelled, deferred, no new cloud data), the UI falls back to the count.
    #[serde(default)]
    pub content_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CleanupResult {
    pub days: i64,
    pub message: Option<String>,
}

/// Convert an internal sync report to the frontend report shape.
pub fn ui_sync_report(
    report: sync::SyncReport,
    started_at: String,
    finished_at: String,
    outcome: String,
    freshness: &BTreeMap<String, StreamFreshness>,
) -> UiSyncReport {
    let streams = report
        .streams
        .into_iter()
        .map(|stream| {
            let stream_freshness = freshness.get(&stream.stream).cloned().unwrap_or_default();
            UiSyncStreamResult {
                stream: stream.stream,
                status: stream_status_name(stream.status).to_string(),
                records_written: stream.records_written,
                message: stream.message,
                needs_reauth: Some(stream.needs_reauth),
                last_cloud_sync_at: stream_freshness.last_cloud_sync_at,
                newest_sample_at: stream_freshness.newest_sample_at,
            }
        })
        .collect::<Vec<_>>();

    let total_records = streams.iter().map(|stream| stream.records_written).sum();
    let notice_code = report
        .cleanup_failed
        .then(|| "ui.sync.cleanup_failed".to_string());

    // The report-level "last cloud sync at" must reflect the newest cloud
    // timestamp the streams actually carried, not the local completion time
    // (which is always >= the cloud data time).  Fall back to the finished
    // time only when no stream carries cloud freshness.
    let last_cloud_sync_at = freshness
        .values()
        .filter_map(|stream| stream.last_cloud_sync_at.clone())
        .max()
        .unwrap_or_else(|| finished_at.clone());

    UiSyncReport {
        success: report.success,
        outcome,
        started_at,
        last_cloud_sync_at,
        finished_at,
        total_records,
        streams,
        message: report.message,
        message_code: None,
        notice_code,
        cloud_latest_at: None,
        content_changed: None,
    }
}

/// Convert persisted stream state into the frontend stream view.
pub fn stream_views(
    statuses: &[models::DataStatus],
    freshness: &BTreeMap<String, StreamFreshness>,
) -> Vec<StreamStatusView> {
    statuses
        .iter()
        .map(|status| {
            let stream_freshness = freshness.get(&status.stream).cloned().unwrap_or_default();
            StreamStatusView {
                stream: status.stream.clone(),
                status: status.status.clone(),
                records: Some(status.records_written),
                last_sync: status.last_sync.map(|timestamp| timestamp.to_rfc3339()),
                last_cloud_sync_at: stream_freshness.last_cloud_sync_at,
                newest_sample_at: stream_freshness.newest_sample_at,
                message: status.message.clone(),
                needs_reauth: Some(status.needs_reauth),
            }
        })
        .collect()
}

/// Convert persisted stream state into a stable capability list.
///
/// Capability availability is evidence-based: only an explicitly `verified`
/// capability with a non-failing stream status is marked available.  In
/// particular, `unverified` is never promoted to available.  The `sleep`
/// stream is the persisted representation of the band-data endpoint, so it is
/// accepted as an alias for the `band_data` capability.
pub fn capability_views(statuses: &[models::DataStatus]) -> Vec<CapabilityStatusView> {
    [
        "heart_rate",
        "daily_summary",
        "workouts",
        "band_data",
        "watch_statistics",
    ]
    .into_iter()
    .map(|capability| {
        let status = find_capability_status(statuses, capability);
        let (available, reason, reason_code) = match status {
            Some(status) => {
                let capability_state = status.capability.trim().to_ascii_lowercase();
                let stream_state = status.status.trim().to_ascii_lowercase();
                let available = capability_state == "verified"
                    && !status.needs_reauth
                    && !matches!(
                        stream_state.as_str(),
                        "failed" | "error" | "unavailable" | "unverified"
                    );
                let (reason, reason_code) = if available {
                    (status.message.clone(), None)
                } else {
                    let (code, message) = capability_reason(status, &capability_state);
                    (Some(message), code)
                };
                (available, reason, reason_code)
            }
            None => (
                false,
                Some("Not synced yet".to_string()),
                Some("err.capability.not_synced".to_string()),
            ),
        };

        CapabilityStatusView {
            capability: capability.to_string(),
            available,
            reason_code,
            reason,
        }
    })
    .collect()
}

fn find_capability_status<'a>(
    statuses: &'a [models::DataStatus],
    capability: &str,
) -> Option<&'a models::DataStatus> {
    // Prefer an exact stream name when both an alias and the canonical name
    // are present in a status snapshot.
    statuses
        .iter()
        .find(|status| status.stream.eq_ignore_ascii_case(capability))
        .or_else(|| {
            if capability == "band_data" {
                statuses
                    .iter()
                    .find(|status| status.stream.eq_ignore_ascii_case("sleep"))
            } else {
                None
            }
        })
}

/// Why this capability is not available. Returns (stable code, original text).
///
/// The branch with code `None` is a raw message passed through from the backend. It comes from the cloud, cannot be translated, and the UI can only
/// show it as is. All other branches are our own text and must change with the UI language.
fn capability_reason(
    status: &models::DataStatus,
    capability_state: &str,
) -> (Option<String>, String) {
    if status.needs_reauth {
        return match status.message.clone() {
            Some(message) => (None, message),
            None => (
                Some("err.capability.needs_reauth".to_string()),
                "Needs re-authentication".to_string(),
            ),
        };
    }
    if let Some(message) = status.message.clone() {
        return (None, message);
    }
    match capability_state {
        "unverified" => (
            Some("err.capability.unverified".to_string()),
            "Capability not verified yet".to_string(),
        ),
        "unavailable" => (
            Some("err.capability.unavailable".to_string()),
            "Capability not available".to_string(),
        ),
        "" => (
            Some("err.capability.unknown".to_string()),
            "Capability state unknown".to_string(),
        ),
        other => (
            Some("err.capability.other".to_string()),
            format!("Capability state: {other}"),
        ),
    }
}

fn stream_status_name(status: sync::StreamStatus) -> &'static str {
    match status {
        sync::StreamStatus::Success => "success",
        sync::StreamStatus::Failed => "failed",
        sync::StreamStatus::Unavailable => "unavailable",
        sync::StreamStatus::Unverified => "unverified",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_status_serializes_required_fields() {
        let status = LoginStatus::new(
            "waiting",
            "err.login.waiting",
            "Complete the login in the pop-up window",
            "https://watchface.zepp.com/",
        );
        let value = serde_json::to_value(&status).unwrap();
        assert_eq!(value["state"], "waiting");
        assert_eq!(value["message"], "Complete the login in the pop-up window");
        assert_eq!(value["code"], "err.login.waiting");
        assert_eq!(value["page_url"], "https://watchface.zepp.com/");
    }
}
