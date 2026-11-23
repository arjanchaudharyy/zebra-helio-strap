//! Fallback auto sync while the window is not visible (macOS feedback).
//!
//! Auto sync used to be only the frontend timer that ticks every minute (`useSyncController.ts`). After the window closes to
//! the tray, or the screen locks or sleeps, the system throttles or stops WebView timers, so nothing syncs all night.
//! This puts an alarm in the backend: it checks every 60 seconds and takes over **only while the window is not visible**. When it is on, connected,
//! not syncing and due, it takes the same path as the "Sync" button (`commands::sync::background_quick_sync`,
//! the same progress events and run lock). While the window is visible the frontend is in charge. Both sides share the run lock as a guard.
//!
//! The switch and interval live in frontend localStorage. The frontend copies them here with `set_auto_sync_prefs` at startup and when settings change.
//! Until they are copied, the alarm does nothing (it does not guess whether the user turned auto sync on).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use tauri::{AppHandle, Emitter, Manager};

use crate::app_state::AppState;

const TICK: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Inner {
    /// Settings copied from the frontend. `None` means not copied yet.
    prefs: Option<(bool, u32)>,
    /// When the alarm last started a sync, and the count of consecutive failures (only syncs the alarm started).
    last_attempt: Option<Instant>,
    failures: u32,
}

#[derive(Default)]
pub(crate) struct AutoSyncAlarm(Mutex<Inner>);

/// How long to wait after a failure: 1 / 2 / 4 / 8 minutes, capped at the interval. With no failure it is the full interval.
/// Same rule as `autoSyncRetryMinutes` in the frontend `src/lib/autoSync.ts`.
pub(crate) fn retry_minutes(failures: u32, interval_minutes: u32) -> u32 {
    if failures == 0 {
        return interval_minutes;
    }
    (1u32 << (failures - 1).min(3)).min(interval_minutes)
}

/// Whether it is due: a full interval since the last successful sync, and a full backoff since the alarm's last attempt.
pub(crate) fn due(
    last_cloud_sync_at: Option<&str>,
    since_last_attempt: Option<Duration>,
    failures: u32,
    interval_minutes: u32,
    now: DateTime<Utc>,
) -> bool {
    let interval = i64::from(interval_minutes) * 60;
    let synced_long_ago =
        match last_cloud_sync_at.and_then(|v| DateTime::parse_from_rfc3339(v).ok()) {
            // A bad timestamp or a clock that jumped back: better to run one extra time (same as the frontend launchSyncIsDue).
            Some(at) => {
                let elapsed = (now - at.with_timezone(&Utc)).num_seconds();
                elapsed < 0 || elapsed >= interval
            }
            None => true,
        };
    let backed_off = since_last_attempt.is_none_or(|elapsed| {
        elapsed >= Duration::from_secs(u64::from(retry_minutes(failures, interval_minutes)) * 60)
    });
    synced_long_ago && backed_off
}

/// The frontend copies the auto sync switch and interval to the backend. Only intervals the settings page can give are accepted; others become 15 minutes.
#[tauri::command]
pub(crate) fn set_auto_sync_prefs(
    alarm: tauri::State<'_, AutoSyncAlarm>,
    enabled: bool,
    interval_minutes: u32,
) {
    let interval = if [15, 30, 60].contains(&interval_minutes) {
        interval_minutes
    } else {
        15
    };
    if let Ok(mut inner) = alarm.0.lock() {
        inner.prefs = Some((enabled, interval));
    }
}

fn window_visible(app: &AppHandle) -> bool {
    app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false)
    })
}

pub(crate) fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;
            if crate::EXIT_GATE.is_exiting() {
                break;
            }
            tick(&app).await;
        }
    });
}

async fn tick(app: &AppHandle) {
    let Some(alarm) = app.try_state::<AutoSyncAlarm>() else {
        return;
    };
    let (interval, since_last_attempt, failures) = {
        let Ok(inner) = alarm.0.lock() else {
            return;
        };
        match inner.prefs {
            Some((true, interval)) => (
                interval,
                inner.last_attempt.map(|at| at.elapsed()),
                inner.failures,
            ),
            _ => return,
        }
    };
    if window_visible(app) {
        return;
    }
    // AppState is attached only in the init thread. If it is not ready, check on the next tick.
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    // If a sync is running (started by the frontend, or a backfill), do not queue: a queued one would sync again right after.
    if state.sync_command_lock.try_lock().is_err() {
        return;
    }
    let last = {
        let data_dir = state.data_dir.clone();
        crate::commands::spawn_independent_read(data_dir, |db| db.cloud_sync_metadata())
            .await
            .ok()
            .and_then(|(at, _)| at)
    };
    if !due(
        last.as_deref(),
        since_last_attempt,
        failures,
        interval,
        Utc::now(),
    ) {
        return;
    }
    if let Ok(mut inner) = alarm.0.lock() {
        inner.last_attempt = Some(Instant::now());
    }
    crate::diagnostics::log("Auto sync: window hidden, the backend alarm starts a quick sync");
    let result = crate::commands::background_quick_sync(app, &state).await;
    let failed = match &result {
        Ok(report) => report.outcome == "failed",
        // Not connected or not verified is not a failure: there is nothing to sync, so the backoff must not grow.
        Err(error) => !matches!(
            error.code.as_str(),
            "err.sync.not_connected"
                | "err.sync.not_verified"
                | "err.sync.not_verified_probe"
                | "err.sync.not_verified_backfill"
        ),
    };
    if let Ok(mut inner) = alarm.0.lock() {
        inner.failures = if failed {
            inner.failures.saturating_add(1)
        } else {
            0
        };
    }
    // When the window comes back, the frontend uses this to reload state and refresh pages (it did not start this run, so it does not reload by itself).
    if let Ok(report) = result {
        let _ = app.emit("sync://background-finished", report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_back_off_one_two_four_eight_minutes_capped_at_the_interval() {
        assert_eq!(retry_minutes(0, 15), 15);
        assert_eq!(
            (1..=6).map(|n| retry_minutes(n, 15)).collect::<Vec<_>>(),
            [1, 2, 4, 8, 8, 8]
        );
        // If the interval is shorter than the backoff, cap at the interval (the settings page minimum is 15 minutes; this is only a guard).
        assert_eq!(retry_minutes(4, 5), 5);
    }

    #[test]
    fn the_alarm_waits_for_the_interval_and_for_the_backoff() {
        let now = DateTime::parse_from_rfc3339("2026-11-03T08:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        // Never synced: due.
        assert!(due(None, None, 0, 15, now));
        // Synced ten minutes ago: not due.
        assert!(!due(Some("2026-11-03T07:50:00Z"), None, 0, 15, now));
        // Synced an hour ago, and the alarm failed once two minutes ago: after the second failure the wait is 2 minutes, so it is due.
        let hour_ago = Some("2026-11-03T07:00:00Z");
        assert!(due(hour_ago, Some(Duration::from_secs(120)), 2, 15, now));
        // After the third failure the wait is 4 minutes. Two minutes is not enough.
        assert!(!due(hour_ago, Some(Duration::from_secs(120)), 3, 15, now));
        // The last one succeeded (failure count reset) but cloud_stale has no time: wait the full interval.
        assert!(!due(hour_ago, Some(Duration::from_secs(600)), 0, 15, now));
        assert!(due(hour_ago, Some(Duration::from_secs(900)), 0, 15, now));
        // Bad timestamp: better to run one extra time.
        assert!(due(Some("garbage"), None, 0, 15, now));
    }
}
