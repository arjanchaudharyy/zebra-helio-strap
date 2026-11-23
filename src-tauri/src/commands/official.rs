//! Zepp official sign-in: authorize in the system browser, poll the relay for the token, and save it in the system credential store.
//!
//! This is separate from the legacy web login (`login/`). That one signs in through an embedded WebView and reads
//! the `api-mifit` token. This one uses the system browser, so third-party logins such as Google / Xiaomi / Facebook / Apple,
//! which do not work in an embedded window, all work here. Each side stores its own token.

use crate::app_state::AppState;
use crate::ipc_error::AppError;
use crate::ipc_types::{mask_user_id, AccountBrief, AccountPair};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use zeppbridge_core::models::ZeppBridgeError;
use zeppbridge_core::official::{
    invalidate_pending_logins, AuthorizationRequest, ClaimOutcome, OfficialClient, OfficialStore,
    AUTHORIZATION_WINDOW_SECONDS,
};

mod recovery;
pub use recovery::recover_official_credentials;

pub const OFFICIAL_EVENT: &str = "official://status";

/// The interval between token claims. This is the longest delay between the user's consent in the browser and the desktop app seeing it.
const CLAIM_INTERVAL: Duration = Duration::from_secs(2);

/// After this many failed connections to the relay in a row, tell the user in the UI and stop the silent wait (about 2-60 seconds, depending on
/// whether the connection is refused at once or times out). Before, all network errors were retried silently. The user saw "Authorized" in the browser
/// and "Waiting for authorization" in the App, for up to 15 minutes, with no reason given.
const RELAY_UNREACHABLE_AFTER: u32 = 3;

/// An extra notice while waiting. The UI localizes it by code; the text here is only a fallback.
type Notice = (&'static str, &'static str);

const RELAY_UNREACHABLE: Notice = (
    "err.official.relay_unreachable",
    "The browser may already be authorized, but ZeppBridge cannot reach zeppbridge.com to get the result. If the browser uses a proxy and the App does not, turn on the system proxy or let the proxy app cover all apps, then click \"Check now\".",
);

/// The relay records this authorization only when the browser opens the authorization page. Before that, every claim gets 404. After this many
/// 404 replies in a row (about 10 seconds), assume the browser did not open the page, and suggest copying the link.
const BROWSER_NOT_OPENED_AFTER: u32 = 5;

const BROWSER_OPEN_FAILED: Notice = (
    "err.official.browser",
    "Could not open the system browser. Click \"Copy authorization link\", paste it into the browser address bar, and approve there. This app then connects automatically.",
);

const BROWSER_NOT_OPENED: Notice = (
    "err.official.browser_not_opened",
    "The authorization page did not seem to open in the browser. If no browser window appeared, click \"Copy authorization link\" and paste it into the browser address bar. This app then connects automatically.",
);

const STILL_PENDING: Notice = (
    "err.official.still_pending",
    "Just checked: the relay has not received this authorization yet. Make sure you clicked approve in the browser and saw the success page. If that was an old authorization in another browser window, start the authorization again.",
);

/// The authorization in progress: the cancel flag and the start URL of this authorization (for "Copy authorization link" in the UI).
/// Only one at a time.
struct Flow {
    cancel: Arc<AtomicBool>,
    start_url: String,
    notice: Option<Notice>,
    /// "I authorized on the web, check now": wakes the poll without waiting for the next interval.
    wake: Arc<tokio::sync::Notify>,
}

static FLOW: Mutex<Option<Flow>> = Mutex::new(None);
#[cfg(test)]
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize)]
pub struct OfficialStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_recovery: Option<recovery::RecoveryStatus>,
    /// `idle` / `waiting` / `connected` / `needs_reauth` / `failed`
    pub state: String,
    /// The error code (`err.official.*`) for `failed`. The UI gets its text from this code.
    pub message_code: Option<String>,
    /// Fallback text when the UI cannot resolve the code. The CLI and the logs use it.
    pub message: Option<String>,
    /// Shows only the last four characters.
    pub user_id_masked: Option<String>,
    /// The nickname from the official profile, so the user can see which account is linked.
    pub nickname: Option<String>,
    pub connected_at: Option<i64>,
    /// The start URL while waiting for authorization. It holds only the state and the hash of the claim secret, so it is safe to copy
    /// into a private window (to use another account).
    pub authorize_url: Option<String>,
    /// When the authorized account and the advanced data account are different, the identity of each one.
    pub conflict: Option<AccountPair>,
}

impl OfficialStatus {
    fn failed(code: &str, message: &str) -> Self {
        Self {
            credential_recovery: None,
            state: "failed".into(),
            message_code: Some(code.into()),
            message: Some(message.into()),
            user_id_masked: None,
            nickname: None,
            connected_at: None,
            authorize_url: None,
            conflict: None,
        }
    }
}

/// The start URL and the notice of the authorization in progress. `None` when nothing is waiting.
fn waiting_flow() -> Option<(String, Option<Notice>)> {
    FLOW.lock().ok().and_then(|flow| {
        flow.as_ref()
            .map(|flow| (flow.start_url.clone(), flow.notice))
    })
}

/// Replaces the notice of this authorization. Returns true only if it changed (only then is a new status worth sending).
fn set_notice(mine: &Arc<AtomicBool>, notice: Option<Notice>) -> bool {
    let Ok(mut flow) = FLOW.lock() else {
        return false;
    };
    match flow.as_mut() {
        Some(current) if Arc::ptr_eq(&current.cancel, mine) && current.notice != notice => {
            current.notice = notice;
            true
        }
        _ => false,
    }
}

/// The next notice while waiting. After a manual check with no result, say "not received yet". When the network comes back, remove "cannot connect".
/// When the authorization page opens, remove "browser did not open". Otherwise keep it, so the notice does not flash between two automatic polls.
fn notice_after_pending(current: Option<Notice>, woken: bool) -> Option<Notice> {
    if woken {
        Some(STILL_PENDING)
    } else if current.is_some_and(|notice| {
        [RELAY_UNREACHABLE, BROWSER_OPEN_FAILED, BROWSER_NOT_OPENED].contains(&notice)
    }) {
        None
    } else {
        current
    }
}

/// What to do after the relay says "no such authorization" (404). `seen`: the relay replied "still waiting" once, so the authorization page
/// opened in the browser. `unseen_replies`: the number of 404 replies in a row so far.
///
/// The relay records the authorization only when the page opens, so a 404 **before it was seen** is not an expiry: the authorization page
/// never opened. Before, every 404 counted as expired, so on Linux, when no browser appeared, the user saw "This authorization has
/// expired". Now keep waiting, and after a while (or when the user clicks "Check now") suggest copying the link and opening it manually.
/// Only a 404 after it was seen is a real expiry (or the token was already claimed).
fn after_unknown_state(
    seen: bool,
    unseen_replies: u32,
    woken: bool,
    current: Option<Notice>,
) -> Unclaimed {
    if seen {
        return Unclaimed::Expired;
    }
    if current == Some(BROWSER_OPEN_FAILED) {
        return Unclaimed::KeepWaiting(current);
    }
    if woken || unseen_replies >= BROWSER_NOT_OPENED_AFTER {
        return Unclaimed::KeepWaiting(Some(BROWSER_NOT_OPENED));
    }
    // A reply came back, so the network works.
    Unclaimed::KeepWaiting(current.filter(|notice| *notice != RELAY_UNREACHABLE))
}

/// The result of `after_unknown_state`: really expired, or keep waiting with this notice.
#[derive(Debug, PartialEq)]
enum Unclaimed {
    Expired,
    KeepWaiting(Option<Notice>),
}

fn current_notice() -> Option<Notice> {
    waiting_flow().and_then(|(_, notice)| notice)
}

fn stored_status(data_dir: &std::path::Path) -> OfficialStatus {
    if let Some(status) = recovery::status(data_dir) {
        return status;
    }
    if let Some((url, notice)) = waiting_flow() {
        return OfficialStatus {
            credential_recovery: None,
            state: "waiting".into(),
            message_code: notice.map(|(code, _)| code.to_string()),
            message: notice.map(|(_, message)| message.to_string()),
            user_id_masked: None,
            nickname: None,
            connected_at: None,
            authorize_url: Some(url),
            conflict: None,
        };
    }
    stored_account_status(&OfficialStore::new(data_dir))
}

fn stored_account_status(store: &OfficialStore) -> OfficialStatus {
    let meta = store.meta().ok().flatten();
    match meta {
        Some(meta) => OfficialStatus {
            // If it expired and was not renewed (get_official_status tries one renewal first), do not say "Authorized".
            credential_recovery: None,
            state: if meta.needs_reauth
                || meta.is_expired(chrono::Utc::now().timestamp())
                || store.load().ok().flatten().is_none()
            {
                "needs_reauth"
            } else {
                "connected"
            }
            .into(),
            message_code: None,
            message: None,
            user_id_masked: Some(mask_user_id(&meta.user_id)),
            nickname: meta.nickname.clone(),
            connected_at: Some(meta.connected_at),
            authorize_url: None,
            conflict: None,
        },
        None => OfficialStatus {
            credential_recovery: None,
            state: "idle".into(),
            message_code: None,
            message: None,
            user_id_masked: None,
            nickname: None,
            connected_at: None,
            authorize_url: None,
            conflict: None,
        },
    }
}

fn publish(app: &AppHandle, status: &OfficialStatus) {
    let _ = app.emit(OFFICIAL_EVENT, status);
}

#[tauri::command]
pub async fn get_official_status(
    state: tauri::State<'_, AppState>,
) -> Result<OfficialStatus, AppError> {
    renew_if_expired(&state.data_dir).await;
    Ok(stored_status(&state.data_dir))
}

/// The token expired: renew it once through the relay. If the renewal works, it stays "Authorized". If Zepp clearly refuses,
/// `refresh_tokens` marks it as needing a new authorization. If the relay is not reachable now, it stays expired, and the UI says so.
async fn renew_if_expired(data_dir: &std::path::Path) {
    let store = OfficialStore::new(data_dir);
    let now = chrono::Utc::now().timestamp();
    let expired = store
        .meta()
        .ok()
        .flatten()
        .is_some_and(|meta| !meta.needs_reauth && meta.is_expired(now));
    if !expired {
        return;
    }
    if let Ok(client) = OfficialClient::new() {
        let _ = zeppbridge_core::official::fresh_tokens(&store, &client, now).await;
    }
}

#[tauri::command]
pub async fn start_official_login(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<OfficialStatus, AppError> {
    let request = AuthorizationRequest::new()
        .map_err(|_| AppError::new("err.official.failed", "Could not generate the random values for this authorization"))?;
    let Some((cancel, generation, wake)) = begin_flow(request.start_url())? else {
        return Ok(stored_status(&state.data_dir));
    };
    // In an AppImage, remove the injected library path first, or the browser does not start (see external_open.rs).
    // A failure to open is not a failure of the flow: keep waiting and show "Copy authorization link" first, so the user can paste it into a browser.
    // On Linux this waits for the opener's exit code (a few seconds at most), so do not block the async thread.
    let opened = {
        let app = app.clone();
        let url = request.start_url();
        tauri::async_runtime::spawn_blocking(move || crate::external_open::open_url(&app, &url))
            .await
    };
    if !matches!(opened, Ok(Ok(()))) {
        crate::diagnostics::log("Official login: could not open the system browser");
        set_notice(&cancel, Some(BROWSER_OPEN_FAILED));
    }
    let waiting = stored_status(&state.data_dir);
    publish(&app, &waiting);
    let data_dir = state.data_dir.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = poll_claim(&app, &data_dir, request, &cancel, &wake, generation).await;
        clear_flow_if(&cancel);
        if cancel.load(Ordering::SeqCst) {
            // The user cancelled (and maybe started again): do not publish the result of this attempt.
            return;
        }
        let status = match outcome {
            Some(failure) => failure,
            None => stored_status(&data_dir),
        };
        publish(&app, &status);
    });
    Ok(waiting)
}

/// Returns `None` if one attempt is already waiting (reuse it). Otherwise registers a new attempt and returns its cancel flag,
/// its generation, and the bell that wakes the poll.
///
/// Inside the lock, only decide "reuse or new". Compute the status after the lock is released: `stored_status` locks
/// FLOW again, and the std Mutex is not reentrant, so computing it inside the lock is a deadlock (code review R04).
#[allow(clippy::type_complexity)]
fn begin_flow(
    start_url: String,
) -> Result<Option<(Arc<AtomicBool>, u64, Arc<tokio::sync::Notify>)>, AppError> {
    let mut flow = FLOW
        .lock()
        .map_err(|_| AppError::new("err.official.failed", "Official sign-in state is invalid. Restart the app and try again"))?;
    if flow.is_some() {
        return Ok(None);
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let wake = Arc::new(tokio::sync::Notify::new());
    *flow = Some(Flow {
        cancel: cancel.clone(),
        start_url,
        notice: None,
        wake: wake.clone(),
    });
    // A new sign-in cancels all earlier receipts that are still in flight (R05).
    let generation = invalidate_pending_logins();
    recovery::clear();
    Ok(Some((cancel, generation, wake)))
}

/// Clears only this attempt: an old poll task must not erase a new authorization started right after a cancel.
fn clear_flow_if(mine: &Arc<AtomicBool>) {
    if let Ok(mut flow) = FLOW.lock() {
        if flow
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(&current.cancel, mine))
        {
            *flow = None;
        }
    }
}

/// Claims until there is a result, a timeout or a cancel. On success the token is saved and it returns `None`. Otherwise it returns the
/// failure status to show (a cancel also counts as "no failure" and goes back to idle).
async fn poll_claim(
    app: &AppHandle,
    data_dir: &std::path::Path,
    request: AuthorizationRequest,
    cancel: &Arc<AtomicBool>,
    wake: &tokio::sync::Notify,
    generation: u64,
) -> Option<OfficialStatus> {
    let client = match OfficialClient::new() {
        Ok(client) => client,
        Err(_) => {
            return Some(OfficialStatus::failed(
                "err.official.failed",
                "Could not create the network client",
            ))
        }
    };
    let deadline = Instant::now() + Duration::from_secs(AUTHORIZATION_WINDOW_SECONDS);
    let mut network_failures = 0u32;
    // Whether the relay replied "still waiting": only that shows the authorization page opened in the browser (see after_unknown_state).
    let mut seen = false;
    let mut unseen_replies = 0u32;
    loop {
        let woken = tokio::select! {
            _ = tokio::time::sleep(CLAIM_INTERVAL) => false,
            _ = wake.notified() => true,
        };
        if cancel.load(Ordering::SeqCst) {
            return None;
        }
        if Instant::now() >= deadline {
            return Some(OfficialStatus::failed(
                "err.official.timeout",
                "Authorization timed out. Start the authorization again",
            ));
        }
        let now = chrono::Utc::now().timestamp();
        match client
            .claim(&request.state, request.claim_secret(), now)
            .await
        {
            Ok(ClaimOutcome::Pending) => {
                network_failures = 0;
                seen = true;
                // A manual check always gets a reply, changed or not, so "Checking" in the UI can stop.
                if set_notice(cancel, notice_after_pending(current_notice(), woken)) || woken {
                    publish(app, &stored_status(data_dir));
                }
                continue;
            }
            // A short network problem is not a failure: claim again next round. If several tries in a row fail, or a manual check
            // also fails, show the reason, but keep claiming, so it connects as soon as the network comes back.
            Err(ZeppBridgeError::NetworkError(_)) => {
                network_failures += 1;
                if (woken || network_failures >= RELAY_UNREACHABLE_AFTER)
                    && (set_notice(cancel, Some(RELAY_UNREACHABLE)) || woken)
                {
                    publish(app, &stored_status(data_dir));
                }
                continue;
            }
            Ok(ClaimOutcome::Ready(tokens)) => {
                return save_verified(data_dir, &client, tokens, generation).await
            }
            Ok(ClaimOutcome::Denied) => {
                return Some(OfficialStatus::failed(
                    "err.official.denied",
                    "You did not approve the authorization. ZeppBridge received nothing",
                ))
            }
            Ok(ClaimOutcome::Failed) => {
                return Some(OfficialStatus::failed(
                    "err.official.rejected",
                    "Zepp did not accept this authorization. Try again",
                ))
            }
            Ok(ClaimOutcome::Expired) => {
                network_failures = 0;
                unseen_replies += 1;
                match after_unknown_state(seen, unseen_replies, woken, current_notice()) {
                    Unclaimed::KeepWaiting(notice) => {
                        if set_notice(cancel, notice) || woken {
                            publish(app, &stored_status(data_dir));
                        }
                        continue;
                    }
                    Unclaimed::Expired => {
                        return Some(OfficialStatus::failed(
                            "err.official.expired",
                            "This authorization has expired. Start the authorization again",
                        ))
                    }
                }
            }
            Err(ZeppBridgeError::Unavailable(_)) => {
                return Some(OfficialStatus::failed(
                    "err.official.not_enabled",
                    "The official sign-in service is not enabled yet",
                ))
            }
            Err(_) => {
                return Some(OfficialStatus::failed(
                    "err.official.failed",
                    "The authorization relay did not respond correctly. Try again later",
                ))
            }
        }
    }
}

/// Checks with the official profile endpoint that the token works, then saves it.
///
/// 401/403 means Zepp clearly rejects this token: do not save it. Other profile endpoint failures (feature not enabled,
/// temporarily unreachable) do not mean the token is bad, because Zepp already accepted it in the token exchange. Save it.
///
/// Check the generation before saving (R05): if the user cancelled, disconnected or started again during claim / profile,
/// do not write this token, and do not count it as a failure.
async fn save_verified(
    data_dir: &std::path::Path,
    client: &OfficialClient,
    mut tokens: zeppbridge_core::official::OfficialTokens,
    generation: u64,
) -> Option<OfficialStatus> {
    let mut nickname = None;
    match client.profile(&tokens.access_token).await {
        Ok(profile) => {
            tokens.user_id = profile.user_id;
            nickname = profile.nickname;
        }
        Err(ZeppBridgeError::NeedsReauth(_)) => {
            return Some(OfficialStatus::failed(
                "err.official.rejected",
                "Zepp did not accept this authorization. Try again",
            ))
        }
        Err(_) => {}
    }
    // Already cancelled / replaced: do not claim the database and do not write (R05).
    if zeppbridge_core::official::current_generation() != generation {
        return None;
    }
    // Advanced data is linked to another Zepp account: say so clearly, do not silently show half the data.
    let legacy = zeppbridge_core::auth::AuthManager::new(data_dir.to_path_buf()).saved_user_id();
    if let Some(legacy) = legacy
        .as_deref()
        .filter(|legacy| crate::commands::accounts_differ(legacy, &tokens.user_id))
    {
        let mut status = OfficialStatus::failed(
            "err.official.other_account",
            "This is a different Zepp account: the official sign-in account and the advanced data account are not the same, so this authorization was not saved. If you first registered Zepp with Google, Apple or another third party, Zepp treats a login with the same email and a password as a new account.",
        );
        status.conflict = Some(AccountPair {
            official: AccountBrief {
                user_id_masked: mask_user_id(&tokens.user_id),
                nickname: nickname.clone(),
            },
            advanced: AccountBrief {
                user_id_masked: mask_user_id(legacy),
                nickname: None,
            },
        });
        return Some(status);
    }
    // If the database belongs to another account, do not save this token (R06).
    if let Err(error) = crate::commands::claim_library_for_login(data_dir, &tokens.user_id).await {
        return Some(OfficialStatus::failed(&error.code, &error.message));
    }
    let store = OfficialStore::new(data_dir);
    match store.save_login(
        &tokens,
        nickname.as_deref(),
        chrono::Utc::now().timestamp(),
        generation,
    ) {
        // Written, or not written because it was cancelled / replaced: neither is a failure.
        Ok(_) => None,
        Err(error) => {
            if matches!(error, ZeppBridgeError::CredentialStore(_)) {
                recovery::remember(data_dir, tokens, nickname, generation);
                recovery::status(data_dir)
            } else {
                let error = AppError::from(error);
                Some(OfficialStatus::failed(&error.code, &error.message))
            }
        }
    }
}

/// Stops the authorization in progress and cancels its receipt that is not written yet (R05).
fn cancel_flow() {
    if let Ok(mut flow) = FLOW.lock() {
        if let Some(current) = flow.take() {
            current.cancel.store(true, Ordering::SeqCst);
        }
    }
    invalidate_pending_logins();
    recovery::clear();
}

/// "I authorized on the web, check now": wakes the waiting poll so it claims at once.
///
/// Do not claim directly here: the relay deletes the token when it hands it over, so with two claims at once the slower one sees "expired".
/// The result goes back to the UI through `official://status` as usual.
#[tauri::command]
pub async fn check_official_login_now(
    state: tauri::State<'_, AppState>,
) -> Result<OfficialStatus, AppError> {
    if let Ok(flow) = FLOW.lock() {
        if let Some(current) = flow.as_ref() {
            current.wake.notify_one();
        }
    }
    Ok(stored_status(&state.data_dir))
}

#[tauri::command]
pub async fn cancel_official_login(
    state: tauri::State<'_, AppState>,
) -> Result<OfficialStatus, AppError> {
    cancel_flow();
    Ok(stored_status(&state.data_dir))
}

/// Disconnects official sign-in: first asks Zepp to revoke (best effort), then deletes the local token. **No health data is deleted.**
#[tauri::command]
pub async fn disconnect_official(
    state: tauri::State<'_, AppState>,
) -> Result<OfficialStatus, AppError> {
    let data_dir: PathBuf = state.data_dir.clone();
    // Also cancel any authorization in flight, so it cannot write a token back after the disconnect.
    cancel_flow();
    let store = OfficialStore::new(&data_dir);
    if let Ok(Some(tokens)) = store.load() {
        if let Ok(client) = OfficialClient::new() {
            // When the revoke works, Zepp also deletes the synced training plans (an empty array cannot be sent after that),
            // so the local ledger resets too. If the revoke fails, keep the ledger: the plans may still be on the watch.
            if client.revoke(&tokens.access_token).await.is_ok() {
                super::training_plan::forget_after_revoke(&state).await;
            }
        }
    }
    let left_behind = store
        .disconnect()
        .map_err(|_| AppError::new("err.official.store", "Could not delete the official token from the system credential store"))?;
    if let Some(left_behind) = left_behind {
        crate::diagnostics::log(&format!(
            "Disconnected official sign-in: local data cleared, but the old token could not be deleted from the system credential store ({left_behind})"
        ));
    }
    Ok(stored_status(&data_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_metadata_without_readable_credentials_offers_reauthorization() {
        struct Credentials(std::result::Result<Option<String>, String>);
        impl zeppbridge_core::auth::CredentialBackend for Credentials {
            fn set(&self, _: &str, _: &str) -> std::result::Result<(), String> {
                Ok(())
            }
            fn get(&self, _: &str) -> std::result::Result<Option<String>, String> {
                self.0.clone()
            }
            fn delete(&self, _: &str) -> std::result::Result<(), String> {
                Ok(())
            }
        }
        let dir = std::env::temp_dir().join(format!(
            "zeppbridge-official-status-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join("official.json"),
            r#"{"user_id":"user-a","connected_at":1,"expires_at":null,"needs_reauth":false}"#,
        )
        .unwrap();
        for (credentials, expected) in [
            (Err("access denied".into()), "needs_reauth"),
            (Ok(None), "needs_reauth"),
            (Ok(Some("token".into())), "connected"),
        ] {
            let store = OfficialStore::with_backend(&dir, Arc::new(Credentials(credentials)));
            let status = stored_account_status(&store);
            assert_eq!(status.state, expected);
            assert!(
                status.user_id_masked.is_some(),
                "retain identity for the disconnect control"
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// When the browser did not open (or failed to open), the relay never saw this authorization, and every claim gets 404.
    /// That is not an expiry: keep waiting and suggest copying the link. Only a 404 after it was seen is an expiry.
    #[test]
    fn a_browser_that_never_opened_is_not_an_expired_authorization() {
        // Open failed: suggest copying the link from the start. More 404 replies do not become an expiry, and the notice stays.
        for replies in 1..=20 {
            assert_eq!(
                after_unknown_state(false, replies, false, Some(BROWSER_OPEN_FAILED)),
                Unclaimed::KeepWaiting(Some(BROWSER_OPEN_FAILED))
            );
        }
        // "Open" worked but the page did not appear: wait quietly for a few rounds, then show the notice. A manual check shows it at once.
        assert_eq!(
            after_unknown_state(false, 1, false, None),
            Unclaimed::KeepWaiting(None)
        );
        assert_eq!(
            after_unknown_state(false, 1, false, Some(RELAY_UNREACHABLE)),
            Unclaimed::KeepWaiting(None),
            "A 404 reply shows that the relay is reachable"
        );
        assert_eq!(
            after_unknown_state(false, BROWSER_NOT_OPENED_AFTER, false, None),
            Unclaimed::KeepWaiting(Some(BROWSER_NOT_OPENED))
        );
        assert_eq!(
            after_unknown_state(false, 1, true, None),
            Unclaimed::KeepWaiting(Some(BROWSER_NOT_OPENED))
        );
        // Only a 404 after the page opened (after a "still waiting" reply) is a real expiry.
        assert_eq!(
            after_unknown_state(true, 1, false, None),
            Unclaimed::Expired
        );
        // After the user opens the pasted link, remove the "browser did not open" notice.
        assert_eq!(notice_after_pending(Some(BROWSER_OPEN_FAILED), false), None);
        assert_eq!(notice_after_pending(Some(BROWSER_NOT_OPENED), false), None);
    }

    /// R04: clicking "Authorize" again while one attempt waits must return the same waiting state in limited time,
    /// and cancel and query must work after that. Before, this path took the same lock again inside the lock and never returned.
    #[test]
    fn a_second_start_reuses_the_waiting_flow_without_deadlocking() {
        let _test_guard = TEST_LOCK.lock().unwrap();
        let dir =
            std::env::temp_dir().join(format!("zeppbridge-official-flow-{}", std::process::id()));
        let (done, finished) = std::sync::mpsc::channel();
        let worker_dir = dir.clone();
        std::thread::spawn(move || {
            let first = begin_flow("https://example.invalid/start?state=a".into()).unwrap();
            assert!(first.is_some());
            let second = begin_flow("https://example.invalid/start?state=b".into()).unwrap();
            assert!(second.is_none(), "Reuse the attempt that is already waiting");
            let waiting = stored_status(&worker_dir);
            cancel_flow();
            let after = stored_status(&worker_dir);
            let _ = done.send((waiting, after, first.unwrap().0));
        });
        let (waiting, after, cancel) = finished
            .recv_timeout(Duration::from_secs(5))
            .expect("The second authorization start hung");
        assert_eq!(waiting.state, "waiting");
        assert_eq!(
            waiting.authorize_url.as_deref(),
            Some("https://example.invalid/start?state=a")
        );
        assert_eq!(after.state, "idle");
        assert!(cancel.load(Ordering::SeqCst), "Cancel must notify the poll in flight");
        let _ = std::fs::remove_dir_all(dir);
    }
}
