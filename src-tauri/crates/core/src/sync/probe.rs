//! Before a sync, check whether the cloud has new data.
//!
//! The path is watch -> phone Zepp (Bluetooth) -> cloud -> ZeppBridge. The computer cannot wake the watch or the phone. When the user clicks
//! "Sync" on the computer, the phone often has not uploaded the new watch data yet. Before, the full set of seven or eight streams ran (10 to 60 seconds)
//! and ended with "no new data", with no hint to pull to refresh on the phone.
//!
//! Now one light request runs before the real sync: **does the cloud have heart rate after the newest local heart rate**. Heart rate is a per-minute
//! continuous stream, so it always moves forward with each phone sync. If it did not move, the other streams are not new either (one phone sync uploads them together).
//!
//! - Legacy channel: the `band_data` details from the newest local heart rate day to today, and the newest heart rate in them. It is the **same source** as local heart rate.
//!   Before, it read `/users/{id}/heartRate`, but for some accounts that endpoint is always an empty page, and an empty page counted as "not new":
//!   the phone had synced, but the computer kept saying "not uploaded to the cloud yet".
//! - Official only: official heart rate is only available per day. Fetch today (local time zone) and check the largest timestamp.
//!
//! "Not new" needs positive evidence: the cloud returned heart rate, and the newest one is not later than local. No heart rate at all (empty page, unknown payload)
//! counts as "unknown", like a timeout, network error or token problem, and the sync runs as usual. The probe can save work but must never block a sync.
//! The "Retry" button in the UI does not probe: the user just pulled to refresh on the phone and wants a real sync.
//! In these three cases there is no probe, and the sync runs directly:
//! - no local heart rate yet (first sync);
//! - the newest local heart rate is two days old: heart rate monitoring may be off, so "heart rate did not move" says nothing about the other streams;
//! - a full-window refresh is due (once a day): it fills late data, so the probe must not block it.

use super::*;
use crate::official::fetch::OfficialFetcher;
use crate::official::fresh_tokens;

/// The total probe time limit. After it, the result is unknown and the sync runs as usual.
pub const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// If the newest local heart rate is older than this, do not trust "heart rate did not move". Sync directly.
pub fn probe_trust_window() -> Duration {
    Duration::days(2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudProbe {
    /// The cloud has data newer than local: sync as usual.
    Newer,
    /// The newest cloud data is the moment local already has: skip the full sync.
    Stale { cloud_latest_at: DateTime<Utc> },
    /// The probe gave no result: sync as usual.
    Unknown,
}

/// Whether to probe, and from which moment. `None` means sync directly.
pub fn probe_since(
    local_newest_heart_rate: Option<&str>,
    now: DateTime<Utc>,
    full_refresh_due: bool,
) -> Option<DateTime<Utc>> {
    if full_refresh_due {
        return None;
    }
    let newest = DateTime::parse_from_rfc3339(local_newest_heart_rate?)
        .ok()?
        .with_timezone(&Utc);
    // The local timestamp is in the future (wrong clock or wrong time zone in a payload): do not use it.
    if newest > now || now - newest > probe_trust_window() {
        return None;
    }
    Some(newest)
}

/// Compare the newest cloud moment with local. When the cloud gives no heart rate moment (empty page), that is **not** "not new". It is unknown:
/// some accounts always get an empty page from one endpoint, and treating it as "not new" would block the sync forever.
pub fn conclude(since: DateTime<Utc>, cloud_newest: Option<DateTime<Utc>>) -> CloudProbe {
    match cloud_newest {
        Some(at) if at > since => CloudProbe::Newer,
        Some(at) => CloudProbe::Stale {
            cloud_latest_at: at,
        },
        None => CloudProbe::Unknown,
    }
}

/// Map one probe (it can fail or time out) to one of three results.
async fn bounded<F>(since: DateTime<Utc>, probe: F) -> CloudProbe
where
    F: std::future::Future<Output = Result<Option<DateTime<Utc>>>>,
{
    bounded_within(PROBE_TIMEOUT, since, probe).await
}

async fn bounded_within<F>(limit: std::time::Duration, since: DateTime<Utc>, probe: F) -> CloudProbe
where
    F: std::future::Future<Output = Result<Option<DateTime<Utc>>>>,
{
    match tokio::time::timeout(limit, probe).await {
        Ok(Ok(newest)) => conclude(since, newest),
        Ok(Err(error)) => {
            tracing::info!("Pre-sync cloud probe gave no result, syncing as usual: {error}");
            CloudProbe::Unknown
        }
        Err(_) => {
            tracing::info!("Pre-sync cloud probe took longer than {limit:?}, syncing as usual");
            CloudProbe::Unknown
        }
    }
}

impl SyncManager {
    /// Legacy channel: whether the newest heart rate in the cloud band data is newer than local. No write lock, no database writes.
    pub async fn probe_cloud(&self, since: DateTime<Utc>) -> CloudProbe {
        bounded(since, self.fetcher.newest_band_heart_rate(since)).await
    }
}

impl OfficialSync {
    /// Official only: the newest heart rate moment today (local time zone). No write lock, no database writes; tokens refresh as usual when needed.
    pub async fn probe_cloud(&self, since: DateTime<Utc>) -> CloudProbe {
        bounded(since, async {
            let Some(tokens) =
                fresh_tokens(&self.store, &self.client, Utc::now().timestamp()).await?
            else {
                return Err(ZeppBridgeError::NeedsReauth(
                    "Zepp official authorization is not connected yet".into(),
                ));
            };
            let fetcher = OfficialFetcher {
                client: &self.client,
                access_token: &tokens.access_token,
                time_zone: self.time_zone.clone(),
            };
            let today = crate::official::fetch::local_today(&self.time_zone);
            fetcher.newest_heart_rate(today).await
        })
        .await
    }
}

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
