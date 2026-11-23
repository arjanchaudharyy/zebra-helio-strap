//! URL policy for the login window: allowed hosts, blocked third-party logins, and logs with redacted fields only.

use super::*;

pub(super) fn is_allowed_login_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    // Only HTTPS navigation to the Zepp/Huami account domains is allowed.
    // Third-party sign-in (Google, Apple, WeChat, Xiaomi, Facebook) is
    // rejected on purpose, see `THIRD_PARTY_AUTH_HOSTS`.
    // `data:`, `blob:` and `about:` URLs are deliberately rejected: page
    // scripts must never be able to steer the credential-collecting webview
    // onto attacker-controlled inline content.
    if parsed.scheme() != "https" {
        return false;
    }
    parsed.host_str().is_some_and(|host| {
        let host = host.to_ascii_lowercase();
        host == "zepp.com"
            || host.ends_with(".zepp.com")
            || host == "huami.com"
            || host.ends_with(".huami.com")
    })
}

/// Domains that the row of third-party login buttons on the official universal login page goes to. **We block all of them.**
///
/// Before, they were allowed, and two groups of users got stuck. Google passkey in an embedded WebView
/// stopped at "verifying it's you". Worse, when a user with a Google-registered Zepp account signed in with the
/// same email + password, Zepp treated it as a different account (a user's screenshot said "created a new account"),
/// so advanced data connected to an empty account. Advanced data accepts only Zepp email / phone number + password.
/// Users of third-party login use official authorization (third-party login works in the system browser).
///
/// This table only decides "do we tell the user why we blocked it". External hosts not in the table
/// are blocked too, but silently.
pub(super) const THIRD_PARTY_AUTH_HOSTS: &[&str] = &[
    "account.xiaomi.com",
    "open.weixin.qq.com",
    "accounts.google.com",
    "www.facebook.com",
    "m.facebook.com",
    "appleid.apple.com",
    "account-us.amazfit.com",
];

/// Is this URL a third-party account authorization page (and not Zepp's own login page)?
pub(super) fn is_third_party_auth_page(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| THIRD_PARTY_AUTH_HOSTS.contains(&host.as_str()))
}

pub(super) fn login_url_log_fields(url: &reqwest::Url) -> String {
    let host = url.host_str().unwrap_or("<none>");
    format!("host={host} path={}", url.path())
}

pub(super) fn log_blocked_login_url(kind: &str, url: &reqwest::Url) {
    // Query and fragment can contain OAuth state/code values.  Never log them.
    eprintln!("blocked Zepp login {kind}: {}", login_url_log_fields(url));
}

/// Does this page look signed in?
///
/// The check uses two public facts only: has the page left the login page, and does any **Zepp/Huami**
/// cookie have a name that appears only after login. `session` / `token` on third-party OAuth pages (Google /
/// Facebook / WeChat / Xiaomi) do not count: they only show that the authorization page has its own session,
/// not that Zepp signed in. It is fine that we cannot see the credentials: we must tell apart "the user has not
/// signed in" and "the user signed in but we did not read it". In the first case we wait. In the second case
/// we stop and give the user the fallback path.
pub(super) fn page_looks_signed_in(page_url: &str, cookies: &[(String, String)]) -> bool {
    const SIGNED_IN_HINTS: &[&str] = &[
        "hm-user-login-info",
        "hm_user_login_info",
        "userid",
        "user_id",
        "apptoken",
        "app_token",
    ];
    if page_host_is_zepp_or_huami(page_url)
        && cookies.iter().any(|(name, _)| {
            let lowered = name.to_ascii_lowercase();
            SIGNED_IN_HINTS
                .iter()
                .any(|hint| lowered == *hint || lowered.contains(hint))
        })
    {
        return true;
    }
    // After a successful login, the watch face site leaves the /login level.
    page_url.starts_with("https://watchface.zepp.com/")
        && !page_url.contains("/login")
        && !page_url.contains("account.xiaomi.com")
}

pub(super) fn page_host_is_zepp_or_huami(page_url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(page_url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    parsed.host_str().is_some_and(|host| {
        let host = host.to_ascii_lowercase();
        host == "zepp.com"
            || host.ends_with(".zepp.com")
            || host == "huami.com"
            || host.ends_with(".huami.com")
    })
}

/// In debug, log "this page looks signed in" and how many cookie names we saw.
///
/// Release builds do not log this. Even with debug on, log only the host and the count, not the names:
/// a name can be an account identifier. Never log values, query or fragment.
#[cfg(debug_assertions)]
pub(super) fn log_credential_probe(page_url: &str, cookies: &[(String, String)]) {
    let host = reqwest::Url::parse(page_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string());
    eprintln!(
        "Zepp login: page looks signed in (host={host}); {} cookie name(s) seen",
        cookies.len()
    );
}

#[cfg(not(debug_assertions))]
pub(super) fn log_credential_probe(_page_url: &str, _cookies: &[(String, String)]) {}

/// The time condition for the fallback page.
///
/// `login_page_activity` answers "is someone still using this page" separately. The two checks are separate:
/// this one only checks if the wait is long enough and if we already redirected once.
///
/// Before, this checked only the time, so every login that needed a password or a verification code was
/// interrupted when the timer ran out. The page changed to the `user.huami.com` privacy page (it only has
/// options such as clear data and delete account).
pub(super) fn fallback_is_due(elapsed: Duration, fallback_used: bool) -> bool {
    !fallback_used && elapsed >= FALLBACK_AFTER
}

/// Is the window still on the page that we opened?
///
/// When the URL changes, the user is in a third-party login flow, and any automatic redirect interrupts it.
pub(super) fn is_primary_login_page(page_url: &str) -> bool {
    page_url.starts_with("https://watchface.zepp.com")
}
