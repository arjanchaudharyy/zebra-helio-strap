//! Login polling: wait for credentials, save them, publish status.

use super::*;

pub(super) fn spawn_login_poll(
    app: AppHandle,
    epoch: u64,
    window: WebviewWindow,
    third_party_blocked: Arc<AtomicBool>,
) {
    tauri::async_runtime::spawn(async move {
        let started = std::time::Instant::now();
        let mut fallback_used = false;
        // The session reached a page that looked signed in, but we never read credentials. For the user
        // these two timeouts are different: in one, the login is not complete; in the other, the login is
        // complete but we got nothing. In the second case, show the manual Token fallback at once, not a
        // "Login timed out" message after a 15-minute wait.
        let mut looked_signed_in = false;
        // Did the user touch the login window in this session? Count only positive evidence: the page says
        // someone is typing, or the URL went to a third-party login page. Once set, it stays set: navigation
        // inside the page clears the injected activity marker, but that does not mean the user stopped the login.
        let mut user_active = false;
        // We read the credentials, and the region confirmation after that is stuck. This is different from
        // "not signed in" and "signed in but no credentials". At timeout we must name the correct case.
        let mut credentials_extracted = false;

        loop {
            if !epoch_active(&app, epoch) {
                return;
            }
            if started.elapsed() >= SESSION_TIMEOUT {
                let (code, message) = if credentials_extracted {
                    (
                        "err.login.region_unreachable",
                        "Read the credentials, but could not reach the Zepp region service to confirm the account. Check your network and try again.",
                    )
                } else if looked_signed_in {
                    (
                        "err.login.credentials_unreadable",
                        "Signed in, but could not read credentials from the login window. You can enter the App Token manually.",
                    )
                } else {
                    ("err.login.timeout", "Login timed out. Try again.")
                };
                finish_failed(&app, epoch, code, message, current_page_url(&window)).await;
                close_login_window_for_epoch(&app, epoch);
                return;
            }
            if app.get_webview_window(LOGIN_WINDOW_LABEL).is_none() {
                finish_idle_if_active(&app, epoch).await;
                return;
            }

            let page_url = current_page_url(&window);
            // The user clicked a third-party login and the navigation block stopped it. Nothing happens on the page,
            // so explain why in the main window. Keep the window open: the user can continue with email + password.
            if third_party_blocked.swap(false, Ordering::SeqCst) && !credentials_extracted {
                emit_progress(
                    &app,
                    epoch,
                    "waiting",
                    "err.login.third_party_blocked",
                    "Advanced data supports only Zepp email / phone number + password login. Third-party login (Google, Apple, WeChat, Xiaomi and others) does not work here. For a third-party login account, use official authorization.",
                    &page_url,
                )
                .await;
            }
            // Ask the page only while a redirect is still possible. If the answer is "in use", stop for good.
            let mut page_is_idle = false;
            if !user_active {
                if is_primary_login_page(&page_url) {
                    match login_page_activity(&window).await {
                        Some(true) => page_is_idle = true,
                        Some(false) => user_active = true,
                        // With no answer, do nothing: do not treat it as in use (then a page that
                        // never rendered never gets the fallback page), and do not treat it as idle.
                        None => {}
                    }
                } else {
                    // The URL is no longer the page that we opened: it is the Zepp universal login page or
                    // the verification code page. The user is in the login flow.
                    user_active = true;
                }
            }
            if page_is_idle && fallback_is_due(started.elapsed(), fallback_used) {
                fallback_used = true;
                let _ = window.navigate(
                    FALLBACK_LOGIN_URL
                        .parse()
                        .expect("fallback login url is static"),
                );
                emit_progress(
                    &app,
                    epoch,
                    "waiting",
                    "err.login.fallback_page",
                    "Opening the fallback login page",
                    FALLBACK_LOGIN_URL,
                )
                .await;
            }

            let cookies = collect_cookies(&window, &page_url).await;
            // Log only cookie names, never values. Names are enough to tell "this cookie is not there",
            // and the values are the credentials.
            if !looked_signed_in && page_looks_signed_in(&page_url, &cookies) {
                looked_signed_in = true;
                log_credential_probe(&page_url, &cookies);
            }
            if let Some(extracted) = parse_login_cookies(&cookies) {
                credentials_extracted = true;
                emit_progress(
                    &app,
                    epoch,
                    "extracting",
                    "err.login.extracting",
                    "Login credentials read. Confirming the region.",
                    &page_url,
                )
                .await;
                emit_progress(
                    &app,
                    epoch,
                    "verifying",
                    "err.login.verifying",
                    "Verifying the account",
                    &page_url,
                )
                .await;

                match persist_extracted_login(&app, epoch, &extracted).await {
                    Ok(()) => {
                        if !epoch_active(&app, epoch) {
                            return;
                        }
                        emit_progress(
                            &app,
                            epoch,
                            "connected",
                            "err.login.connected",
                            "Zepp account connected",
                            &page_url,
                        )
                        .await;
                        close_login_window_for_epoch(&app, epoch);
                        // Fetch the device list after "connected": the region check is already slow,
                        // do not make the user watch "verifying" for one more device request.
                        if let Some(state) = app.try_state::<AppState>() {
                            super::super::data::refresh_device_profile(&state).await;
                        }
                        return;
                    }
                    // The network is down now, but the credentials are good. If we close the login window, the
                    // isolated session goes with it, and the user must redo the verification code and QR scan.
                    // So keep the window and retry every few seconds until the network returns or the session times out.
                    Err(failure) if failure.retryable => {
                        emit_progress(
                            &app,
                            epoch,
                            "waiting",
                            "err.login.region_retrying",
                            "Cannot reach the Zepp region service now. Retrying. The login window stays open.",
                            &page_url,
                        )
                        .await;
                        tokio::time::sleep(REGION_RETRY_BACKOFF).await;
                        continue;
                    }
                    Err(failure) => {
                        finish_login_failure(&app, epoch, failure, page_url).await;
                        close_login_window_for_epoch(&app, epoch);
                        return;
                    }
                }
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    });
}

/// Is a login failure worth one more try in place?
///
/// "The network is down" and "Zepp rejected the credentials" are different: the first one recovers soon,
/// and for the second one a wait does not help. Before, both closed the login window. That window is an
/// isolated session, so closing it made the user redo the verification code, QR scan and third-party authorization.
pub(super) struct LoginFailure {
    pub(super) error: AppError,
    pub(super) retryable: bool,
    /// When the account is not the same as the official authorization account, the account on each side (the UI shows them side by side).
    pub(super) conflict: Option<Box<crate::ipc_types::AccountPair>>,
}

impl LoginFailure {
    pub(super) fn fatal(error: AppError) -> Self {
        Self {
            error,
            retryable: false,
            conflict: None,
        }
    }

    pub(super) fn retryable(error: AppError) -> Self {
        Self {
            error,
            retryable: true,
            conflict: None,
        }
    }

    pub(super) fn cancelled() -> Self {
        Self::fatal(AppError::new("err.login.cancelled", "Login cancelled"))
    }
}

pub(super) async fn persist_extracted_login(
    app: &AppHandle,
    epoch: u64,
    extracted: &ExtractedLogin,
) -> std::result::Result<(), LoginFailure> {
    let Some(state) = app.try_state::<AppState>() else {
        return Err(LoginFailure::fatal(AppError::new(
            "err.login.state_unavailable",
            "App state is not available",
        )));
    };
    let (preferred, authoritative_count) =
        preferred_region_hosts(&state, &extracted.user_id, extracted.region_hint.as_deref()).await;
    let winner = probe_region_hosts(
        &extracted.user_id,
        &extracted.app_token,
        &preferred,
        authoritative_count,
    )
    .await?;
    let RegionWinner { auth, confidence } = winner;
    if !epoch_active(app, epoch) {
        return Err(LoginFailure::cancelled());
    }

    let _command_guard = state.lock_sync_commands().await;
    if !epoch_active(app, epoch) {
        return Err(LoginFailure::cancelled());
    }

    // The official authorization is for a different Zepp account: say so, do not silently show only half the data.
    // The most common cause: the account was registered with Google, and here the user signed in with the same email + password, so Zepp made a new account.
    if let Some(failure) = other_account_than_official(&state.data_dir, &auth.user_id) {
        return Err(failure);
    }
    // If the database belongs to a different account, do not save this token.
    if let Err(error) =
        crate::commands::claim_library_for_login(&state.data_dir, &auth.user_id).await
    {
        return Err(LoginFailure::fatal(error));
    }
    if let Err(error) = state.auth.save_auth(&auth) {
        if recovery::is_store_error(&error) {
            recovery::remember(&state, auth, epoch, Some(confidence));
        }
        // A save failure usually comes from the system credential manager (blocked by policy, token too long), so pass
        // the underlying cause as is. The UI gets localized text by code. This message is for the CLI, logs and reports.
        return Err(LoginFailure::fatal(AppError::from(error)));
    }

    // Disk is the source of truth from here. If the epoch dies after save,
    // still install the in-memory manager so it matches, then return cancelled
    // so the poller does not emit `connected`.
    apply_persisted_login(&state, auth, confidence).await?;
    if !epoch_active(app, epoch) {
        return Err(LoginFailure::cancelled());
    }
    Ok(())
}

/// Give the error to show when the advanced data login account is not the same as the saved official authorization account.
///
/// The user_id on both sides uses the same numbering (checked with a real database: one account has equal values on both sides),
/// so we can compare them directly.
pub(super) fn other_account_than_official(
    data_dir: &std::path::Path,
    user_id: &str,
) -> Option<LoginFailure> {
    use crate::ipc_types::{mask_user_id, AccountBrief, AccountPair};
    let official = zeppbridge_core::official::OfficialStore::new(data_dir)
        .meta()
        .ok()
        .flatten()?;
    crate::commands::accounts_differ(&official.user_id, user_id).then(|| {
        let mut failure = LoginFailure::fatal(AppError::new(
            "err.login.other_account",
            "This is a different Zepp account: the account you signed in with is not the official authorization account. No data was written. If you registered with Zepp through Google, Apple or another third party, a login with the same email and a password makes Zepp treat it as a new account.",
        ));
        failure.conflict = Some(Box::new(AccountPair {
            official: AccountBrief {
                user_id_masked: mask_user_id(&official.user_id),
                nickname: official.nickname.clone(),
            },
            advanced: AccountBrief {
                user_id_masked: mask_user_id(user_id),
                nickname: None,
            },
        }));
        failure
    })
}

pub(super) async fn apply_persisted_login(
    state: &AppState,
    auth: AuthInfo,
    confidence: &'static str,
) -> std::result::Result<(), LoginFailure> {
    let manager = match AppState::build_sync_manager(auth, &state.data_dir) {
        Ok(manager) => manager,
        Err(error) => {
            let message = error.user_message();
            let _ = state.auth.clear_auth();
            state.replace_sync_manager(None).await;
            {
                let mut auth_state = state.auth_state.write().await;
                *auth_state = "unconfigured".to_string();
            }
            {
                let mut warning = state.auth_warning.write().await;
                *warning = Some(format!("Cannot start sync. Check the auth region and try again: {message}"));
            }
            return Err(LoginFailure::fatal(AppError::new(
                "err.login.sync_init_failed",
                message,
            )));
        }
    };

    state.replace_sync_manager(Some(manager)).await;
    {
        let mut auth_state = state.auth_state.write().await;
        *auth_state = "verified".to_string();
    }
    {
        let mut warning = state.startup_warning.write().await;
        *warning = None;
    }
    {
        let mut warning = state.auth_warning.write().await;
        *warning = None;
    }
    {
        let mut region = state.region_confidence.write().await;
        *region = confidence.to_string();
    }
    Ok(())
}

pub(super) async fn publish_failed(
    app: &AppHandle,
    state: &AppState,
    error: &AppError,
    page_url: &str,
) {
    publish_status(
        app,
        state,
        LoginStatus::new(
            "failed",
            &error.code,
            error.message.as_str(),
            safe_login_page_url(page_url),
        ),
    )
    .await;
}

pub(super) fn epoch_active(app: &AppHandle, epoch: u64) -> bool {
    app.try_state::<AppState>()
        .is_some_and(|state| state.login.epoch.load(Ordering::SeqCst) == epoch)
}

pub(super) async fn publish_status(app: &AppHandle, state: &AppState, status: LoginStatus) {
    {
        let mut current = state.login.status.write().await;
        *current = status.clone();
    }
    let _ = app.emit(LOGIN_EVENT, status);
}

pub(super) async fn emit_progress(
    app: &AppHandle,
    epoch: u64,
    state_name: &str,
    code: &str,
    message: &str,
    page_url: &str,
) {
    if !epoch_active(app, epoch) {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let status = if state_name == "failed" {
        recovery::status(&state).unwrap_or_else(|| {
            LoginStatus::new(state_name, code, message, safe_login_page_url(page_url))
        })
    } else {
        LoginStatus::new(state_name, code, message, safe_login_page_url(page_url))
    };
    publish_status(app, &state, status).await;
}

pub(super) fn safe_login_page_url(raw: &str) -> String {
    let Ok(mut url) = reqwest::Url::parse(raw) else {
        return String::new();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

pub(super) async fn finish_failed(
    app: &AppHandle,
    epoch: u64,
    code: &str,
    message: &str,
    page_url: String,
) {
    emit_progress(app, epoch, "failed", code, message, &page_url).await;
}

/// Finish a failed login: the same as `finish_failed`, but also send the account comparison to the UI.
async fn finish_login_failure(
    app: &AppHandle,
    epoch: u64,
    failure: LoginFailure,
    page_url: String,
) {
    if !epoch_active(app, epoch) {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let mut status = LoginStatus::new(
        "failed",
        &failure.error.code,
        failure.error.message,
        safe_login_page_url(&page_url),
    );
    status.conflict = failure.conflict.map(|pair| *pair);
    publish_status(app, &state, status).await;
}

pub(super) async fn finish_idle_if_active(app: &AppHandle, epoch: u64) {
    if !epoch_active(app, epoch) {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    publish_status(app, &state, LoginStatus::idle()).await;
}
