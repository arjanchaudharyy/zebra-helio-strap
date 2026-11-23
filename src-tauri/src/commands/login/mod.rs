use super::auth::{probe_region_evidence, RegionEvidence};

use crate::app_state::AppState;

use crate::connectors::zepp::validate_region_host;

use crate::ipc_error::AppError;

use crate::ipc_types::LoginStatus;

use crate::models::{AuthInfo, ZeppBridgeError};

use serde_json::Value;

use std::sync::atomic::{AtomicBool, Ordering};

use std::sync::Arc;

use std::time::Duration;

use tauri::{
    webview::NewWindowResponse, AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

mod copy;
mod extract;
mod poll;
pub(crate) mod recovery;
mod region_probe;
pub use recovery::recover_advanced_credentials;
#[cfg(test)]
mod tests;
mod url_policy;
mod window;

use copy::*;
pub(crate) use extract::*;
use poll::*;
pub(crate) use region_probe::*;
use url_policy::*;
use window::*;

const LOGIN_WINDOW_LABEL: &str = "zepp-login";

const LOGIN_EVENT: &str = "login://status";

const PRIMARY_LOGIN_URL: &str = "https://watchface.zepp.com/";

const FALLBACK_LOGIN_URL: &str = "https://user.huami.com/privacy2/index.html";

const POLL_INTERVAL: Duration = Duration::from_millis(750);

/// How long the page must be idle before we open the fallback page for the user.
///
/// This timer handles one case only: the main login page did not render on this machine, and the user
/// waits at a blank page. It is not "the time limit for login": typing an email and password, waiting for
/// a verification code by email, or third-party authorization all take much longer. So in addition to the
/// time, we also confirm that nobody touched the page. See `fallback_is_due` and `login_page_is_idle`.
const FALLBACK_AFTER: Duration = Duration::from_secs(90);

const SESSION_TIMEOUT: Duration = Duration::from_secs(15 * 60);

const COOKIE_EVAL_TIMEOUT: Duration = Duration::from_secs(2);

/// After we close the previous login window, the maximum wait for it to release the `zepp-login` label.
///
/// Usually this takes a few milliseconds: one turn of the main thread. We allow 3 seconds so that a busy
/// main thread does not cause a wrong result, but the user does not wait long at a button that does not respond. See
/// `close_login_window_and_wait`.
const LOGIN_WINDOW_CLOSE_TIMEOUT: Duration = Duration::from_secs(3);

const LOGIN_WINDOW_CLOSE_POLL: Duration = Duration::from_millis(25);

/// After a region probe fails because of the network, how long to wait before the next try.
const REGION_RETRY_BACKOFF: Duration = Duration::from_secs(5);

/// Set a "user touched this page" marker in the login window.
///
/// Record only that an input event occurred. Do not read keys or content. The login form is often in a
/// cross-origin iframe, and events do not bubble to the top document, so child frames send a fixed string up with postMessage.
/// The script is injected again on each navigation, so the marker applies to the current page only.
const LOGIN_ACTIVITY_SCRIPT: &str = r#"(function(){
  try {
    if (window.__zeppbridgeActivityHooked) { return; }
    window.__zeppbridgeActivityHooked = true;
    window.__zeppbridgeInteracted = false;
    var mark = function(){
      window.__zeppbridgeInteracted = true;
      try {
        if (window.top && window.top !== window) {
          window.top.postMessage('zeppbridge:login-activity', '*');
        }
      } catch (e) {}
    };
    ['keydown','pointerdown','mousedown','touchstart','paste','input','change'].forEach(function(name){
      window.addEventListener(name, mark, true);
    });
    window.addEventListener('message', function(event){
      if (event && event.data === 'zeppbridge:login-activity') {
        window.__zeppbridgeInteracted = true;
      }
    }, true);
  } catch (e) {}
})();"#;

/// Hide the row of third-party login buttons on the official universal login page (see `THIRD_PARTY_AUTH_HOSTS`).
///
/// The selectors were checked against the universal login page on user.zepp.com: `.hm-social-login`
/// wraps the row, and each button is `.social-item`. Change only the element's own style, do not insert `<style>`:
/// the login page CSP can block inline style sheets, but CSSOM style writes are not affected. If a Zepp
/// redesign breaks the selectors, the navigation block is the fallback.
const HIDE_THIRD_PARTY_SCRIPT: &str = r#"(function(){
  try {
    if (window.__zeppbridgeThirdPartyHidden) { return; }
    window.__zeppbridgeThirdPartyHidden = true;
    var SELECTOR = '.hm-social-login, .social-item, [class*="social-login"]';
    var hide = function(){
      try {
        var nodes = document.querySelectorAll(SELECTOR);
        for (var i = 0; i < nodes.length; i++) {
          if (nodes[i].style.display !== 'none') {
            nodes[i].style.setProperty('display', 'none', 'important');
          }
        }
      } catch (e) {}
    };
    new MutationObserver(hide).observe(document, { childList: true, subtree: true });
    document.addEventListener('DOMContentLoaded', hide);
  } catch (e) {}
})();"#;

/// Banner at the top of the login window, on the top-level page only. `login_banner_text` gives the text
/// by UI language and replaces `__ZEPPBRIDGE_BANNER_TEXT__` (it is already a JSON string literal).
/// Write the text with `textContent`, not as HTML.
const LOGIN_BANNER_SCRIPT: &str = r#"(function(){
  try {
    if (window.top !== window) { return; }
    var TEXT = __ZEPPBRIDGE_BANNER_TEXT__;
    var ID = 'zeppbridge-login-banner';
    var place = function(){
      if (!document.body || document.getElementById(ID)) { return; }
      var bar = document.createElement('div');
      bar.id = ID;
      bar.setAttribute('role', 'note');
      bar.textContent = TEXT;
      var css = {
        'box-sizing': 'border-box', 'position': 'relative', 'z-index': '2147483647',
        'margin': '0', 'padding': '10px 18px', 'background': '#fff4d6', 'color': '#3d2c00',
        'border-bottom': '1px solid #e8c766', 'font': '13px/1.55 system-ui, -apple-system, "Segoe UI", sans-serif',
        'text-align': 'center'
      };
      for (var key in css) { bar.style.setProperty(key, css[key], 'important'); }
      document.body.insertBefore(bar, document.body.firstChild);
    };
    new MutationObserver(place).observe(document, { childList: true, subtree: true });
    document.addEventListener('DOMContentLoaded', place);
  } catch (e) {}
})();"#;

const REGION_HOST_ALLOWLIST: &[&str] = &[
    "https://api-mifit-cn.huami.com",
    "https://api-mifit-cn2.huami.com",
    "https://api-mifit-cn.zepp.com",
    "https://api-mifit-cn2.zepp.com",
    "https://api-mifit-cn3.zepp.com",
    "https://api-mifit.huami.com",
    "https://api-mifit.zepp.com",
    "https://api-mifit-us.huami.com",
    "https://api-mifit-us2.huami.com",
    "https://api-mifit-us3.zepp.com",
    "https://api-mifit-de.huami.com",
    "https://api-mifit-de2.huami.com",
    "https://api-mifit-de.zepp.com",
    "https://api-mifit-sg.huami.com",
    "https://api-mifit-sg2.huami.com",
    "https://api-mifit-in.huami.com",
    "https://api-mifit-ru.huami.com",
];

/// Credentials parsed from the login webview.  Never logged in full.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ExtractedLogin {
    pub user_id: String,
    pub app_token: String,
    pub region_hint: Option<String>,
}

impl std::fmt::Debug for ExtractedLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtractedLogin")
            .field("user_id", &self.user_id)
            .field(
                "app_token",
                &if self.app_token.is_empty() {
                    "<empty>"
                } else {
                    "<redacted>"
                },
            )
            .field("region_hint", &self.region_hint)
            .finish()
    }
}

#[tauri::command]
pub async fn start_web_login(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    locale: String,
) -> std::result::Result<LoginStatus, AppError> {
    let epoch = state.login.epoch.fetch_add(1, Ordering::SeqCst) + 1;
    recovery::clear(&state);
    let page_url = PRIMARY_LOGIN_URL.to_string();

    // We must wait until the previous window is really gone, not only send a close request. See
    // `close_login_window_and_wait`.
    if !close_login_window_and_wait(&app).await {
        if state.login.epoch.load(Ordering::SeqCst) != epoch {
            return Err(AppError::new("err.login.cancelled", "Login cancelled"));
        }
        let error = AppError::new(
            "err.login.window_busy",
            "The previous login window is still closing. Wait a moment and try again.",
        );
        publish_failed(&app, &state, &error, &page_url).await;
        return Err(error);
    }
    if state.login.epoch.load(Ordering::SeqCst) != epoch {
        return Err(AppError::new("err.login.cancelled", "Login cancelled"));
    }

    let status = LoginStatus::new(
        "waiting",
        "err.login.waiting",
        "Complete the Zepp login in the pop-up window",
        page_url.clone(),
    );
    publish_status(&app, &state, status.clone()).await;

    let third_party_blocked = Arc::new(AtomicBool::new(false));
    let window = match build_login_window(&app, &page_url, &locale, third_party_blocked.clone()) {
        Ok(window) => window,
        // The status already says "complete the login in the pop-up window", but the pop-up did not open.
        // If we do not change it back, the UI reads this waiting status next time and points the user
        // to a window that does not exist.
        Err(error) => {
            publish_failed(&app, &state, &error, &page_url).await;
            return Err(error);
        }
    };
    spawn_login_poll(app, epoch, window, third_party_blocked);
    Ok(status)
}

#[tauri::command]
pub async fn cancel_web_login(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> std::result::Result<LoginStatus, AppError> {
    state.login.epoch.fetch_add(1, Ordering::SeqCst);
    recovery::clear(&state);
    close_login_window(&app);
    let status = LoginStatus::idle();
    publish_status(&app, &state, status.clone()).await;
    Ok(status)
}

#[tauri::command]
pub async fn get_login_status(
    state: tauri::State<'_, AppState>,
) -> std::result::Result<LoginStatus, AppError> {
    if let Some(status) = recovery::status(&state) {
        return Ok(status);
    }
    let mut status = state.login.status.read().await.clone();
    if status.credential_recovery.is_some() {
        status = recovery::expired();
    }
    Ok(status)
}
