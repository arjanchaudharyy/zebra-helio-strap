//! Login window: create it, probe activity, close it by epoch.

use super::*;

pub(super) fn build_login_window(
    app: &AppHandle,
    page_url: &str,
    locale: &str,
    third_party_blocked: Arc<AtomicBool>,
) -> std::result::Result<WebviewWindow, AppError> {
    let url = page_url
        .parse()
        .map_err(|_| AppError::new("err.login.bad_url", "Invalid login URL"))?;
    let app_for_new_window = app.clone();
    let blocked_on_navigation = third_party_blocked.clone();
    let banner_script = LOGIN_BANNER_SCRIPT.replace(
        "__ZEPPBRIDGE_BANNER_TEXT__",
        &serde_json::to_string(login_banner_text(locale)).unwrap_or_else(|_| "\"\"".into()),
    );
    // The zepp-login capability is only window-close + event-listen, and it
    // has no `remote` URLs. A page loaded from watchface.zepp.com therefore
    // cannot invoke app commands: Tauri 2 requires an explicit remote
    // capability for that, and we do not add one. Do not "fix" this isolation
    // by granting remote IPC.
    WebviewWindowBuilder::new(app, LOGIN_WINDOW_LABEL, WebviewUrl::External(url))
        .title(login_window_title(locale))
        // A login attempt must not inherit a previous account's cookies or
        // localStorage. OAuth popups still stay in this one WebView session,
        // but closing it discards the session instead of silently reusing it
        // for the next account.
        .incognito(true)
        // The login form can be in a child frame, so attach the activity marker in both.
        .initialization_script_for_all_frames(LOGIN_ACTIVITY_SCRIPT)
        // Hide the third-party login buttons on the page first. If a Zepp redesign breaks the selector, the
        // navigation block below is the fallback and tells the user why the click did nothing.
        .initialization_script_for_all_frames(HIDE_THIRD_PARTY_SCRIPT)
        .initialization_script(&banner_script)
        .inner_size(920.0, 760.0)
        .min_inner_size(420.0, 520.0)
        .resizable(true)
        .on_navigation(move |url| {
            let allowed = is_allowed_login_url(url.as_str());
            if !allowed {
                log_blocked_login_url("navigation", url);
                if is_third_party_auth_page(url.as_str()) {
                    blocked_on_navigation.store(true, Ordering::SeqCst);
                }
            }
            allowed
        })
        // A sign-in page that switches to `target=_blank` must not escape to
        // the system browser because the resulting Zepp cookies would live in
        // a different browser profile.
        .on_new_window(move |url, _features| {
            if !is_allowed_login_url(url.as_str()) {
                log_blocked_login_url("new-window", &url);
                if is_third_party_auth_page(url.as_str()) {
                    third_party_blocked.store(true, Ordering::SeqCst);
                }
                return NewWindowResponse::Deny;
            }
            if let Some(window) = app_for_new_window.get_webview_window(LOGIN_WINDOW_LABEL) {
                if let Err(error) = window.navigate(url) {
                    eprintln!("Zepp login OAuth navigation failed: {error}");
                }
            }
            NewWindowResponse::Deny
        })
        .build()
        .map_err(|error| {
            AppError::new(
                "err.login.window_failed",
                format!("Cannot open the login window: {error}"),
            )
        })
}

/// Is someone using this page: `Some(true)` idle, `Some(false)` in use, `None` no answer.
///
/// The three states are intentional. With no answer, we cannot treat the page as in use: then a user
/// whose main login page never rendered never gets the fallback page. We also cannot treat it as idle: then
/// one timeout can send away a user who waits for a verification code. So with no answer, do nothing and ask again next round.
pub(super) async fn login_page_activity(window: &WebviewWindow) -> Option<bool> {
    const SCRIPT: &str = r#"(function(){
  try {
    var typed = false;
    var fields = document.querySelectorAll('input, textarea');
    for (var i = 0; i < fields.length; i++) {
      var field = fields[i];
      if (field.type === 'hidden') { continue; }
      if (field.value && String(field.value).length > 0) { typed = true; break; }
    }
    return JSON.stringify({ idle: !window.__zeppbridgeInteracted && !typed });
  } catch (e) { return JSON.stringify({ idle: false }); }
})()"#;

    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let sent = std::sync::Mutex::new(Some(tx));
    window
        .eval_with_callback(SCRIPT, move |raw| {
            if let Some(tx) = sent.lock().ok().and_then(|mut guard| guard.take()) {
                let _ = tx.send(decode_eval_string(&raw));
            }
        })
        .ok()?;
    let raw = tokio::time::timeout(COOKIE_EVAL_TIMEOUT, rx)
        .await
        .ok()
        .and_then(Result::ok)?;
    serde_json::from_str::<Value>(&raw)
        .ok()?
        .get("idle")
        .and_then(Value::as_bool)
}

pub(super) fn current_page_url(window: &WebviewWindow) -> String {
    window
        .url()
        .map(|url| url.to_string())
        .unwrap_or_else(|_| PRIMARY_LOGIN_URL.to_string())
}

pub(super) fn close_login_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LOGIN_WINDOW_LABEL) {
        let _ = window.close();
    }
}

pub(super) fn closer_epoch_owns_the_window(closer: u64, active: u64) -> bool {
    closer == active
}

pub(super) fn close_login_window_for_epoch(app: &AppHandle, epoch: u64) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if closer_epoch_owns_the_window(epoch, state.login.epoch.load(Ordering::SeqCst)) {
        close_login_window(app);
    }
}

/// What to do this round: the label is free, wait more, or the wait failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CloseWait {
    /// No window holds the `zepp-login` label now, so we can build a new window.
    Released,
    /// The label is still held, but the wait is not over. Check again next round.
    KeepWaiting,
    /// The label was not released before the timeout.
    TimedOut,
}

pub(super) fn close_wait_step(window_still_registered: bool, elapsed: Duration) -> CloseWait {
    if !window_still_registered {
        return CloseWait::Released;
    }
    if elapsed >= LOGIN_WINDOW_CLOSE_TIMEOUT {
        return CloseWait::TimedOut;
    }
    CloseWait::KeepWaiting
}

/// Close the previous login window and wait until its label is really released.
///
/// `WebviewWindow::close()` only posts a close message to the main thread event loop and returns.
/// The window and its webview leave the manager table only after the main thread handles it and emits
/// `Destroyed`. The login window uses the fixed label `zepp-login`, so a build with the same label right
/// after that fails with "a webview with label `zepp-login` already exists".
///
/// This is what happened when the user clicked re-authenticate while a login window was open: the old
/// window closed, the new one was not built, and the UI only said "Cannot open the login window". The user had no window left.
///
/// The alternative is to reuse the old window and navigate it to the login page, with no wait. But the
/// login window is an isolated `.incognito(true)` session. Reuse keeps the previous account's session,
/// and an earlier change removed exactly that. So we close the window and wait.
pub(super) async fn close_login_window_and_wait(app: &AppHandle) -> bool {
    close_login_window(app);
    let started = std::time::Instant::now();
    loop {
        match close_wait_step(
            app.get_webview_window(LOGIN_WINDOW_LABEL).is_some(),
            started.elapsed(),
        ) {
            CloseWait::Released => return true,
            CloseWait::TimedOut => return false,
            CloseWait::KeepWaiting => tokio::time::sleep(LOGIN_WINDOW_CLOSE_POLL).await,
        }
    }
}
