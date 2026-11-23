use super::*;

#[test]
fn extracted_login_debug_never_prints_the_token() {
    for token in ["secret-app-token", "unicode-\u{4ee4}\u{724c}\nwith-escapes"] {
        let extracted = ExtractedLogin {
            user_id: "user".into(),
            app_token: token.into(),
            region_hint: Some("cn".into()),
        };
        for debug in [format!("{extracted:?}"), format!("{extracted:#?}")] {
            assert!(!debug.contains(token));
            assert!(!debug.contains("unicode-"));
            assert!(debug.contains("<redacted>"));
        }
        assert_eq!(extracted.clone(), extracted);
        assert_eq!(extracted.app_token, token);
    }
    let empty = ExtractedLogin {
        user_id: "user".into(),
        app_token: String::new(),
        region_hint: None,
    };
    assert!(format!("{empty:?}").contains("<empty>"));
}

fn probe_auth(host: &str) -> AuthInfo {
    AuthInfo {
        app_token: "token".to_string(),
        user_id: "user".to_string(),
        region_host: host.to_string(),
    }
}

/// This is the bug itself: the wrong region answers first, the correct region answers later.
///
/// A region that does not know this user does not look up data, so its empty response often arrives
/// before the real devices from the correct region. The old code used the first answer, so it saved the
/// wrong region. Every later sync went there: the UI said connected, and the database had no records.
#[test]
fn a_fast_empty_region_does_not_beat_a_slower_one_that_knows_the_account() {
    let mut outcome = RegionBatchOutcome::default();

    assert!(!outcome.record(
        0,
        probe_auth("https://wrong.example"),
        RegionEvidence::Empty
    ));
    assert!(outcome.record(
        5,
        probe_auth("https://right.example"),
        RegionEvidence::Identified
    ));

    let winner = outcome.into_winner(false).expect("a winner");
    assert_eq!(winner.auth.region_host, "https://right.example");
    assert_eq!(winner.confidence, "identified");
}

/// When no host returns devices, pick by preference order, not by who answered first.
#[test]
fn only_empty_answers_fall_back_to_the_most_preferred_host() {
    let mut outcome = RegionBatchOutcome::default();

    outcome.record(
        3,
        probe_auth("https://third.example"),
        RegionEvidence::Empty,
    );
    outcome.record(
        1,
        probe_auth("https://first.example"),
        RegionEvidence::Empty,
    );
    outcome.record(
        2,
        probe_auth("https://second.example"),
        RegionEvidence::Empty,
    );

    let winner = outcome.into_winner(false).expect("a winner");
    assert_eq!(winner.auth.region_host, "https://first.example");
    // A guess must be marked as a guess: when a sync gives no records, this is the only clue.
    assert_eq!(winner.confidence, "unconfirmed");
}

/// A host that Zepp named but that returns no devices is not wrong: the account can have no watch.
/// It still counts, but it is marked `hinted`, not `identified`.
#[test]
fn an_authoritative_host_still_counts_when_the_account_has_no_devices() {
    let mut outcome = RegionBatchOutcome::default();
    outcome.record(
        0,
        probe_auth("https://hinted.example"),
        RegionEvidence::Empty,
    );

    let winner = outcome.into_winner(true).expect("a winner");
    assert_eq!(winner.auth.region_host, "https://hinted.example");
    assert_eq!(winner.confidence, "hinted");
}

/// If no host answers, there is no result. Do not pick one at random.
#[test]
fn no_answer_produces_no_winner() {
    assert!(RegionBatchOutcome::default().into_winner(true).is_none());
    assert!(RegionBatchOutcome::default().into_winner(false).is_none());
}

/// A clear credential rejection is still a final failure. Empty responses from other hosts do not override it.
#[test]
fn an_explicit_rejection_stays_fatal() {
    let mut failures = RegionProbeFailures::default();
    failures.record(RegionProbeFailure::Transient);
    failures.record(RegionProbeFailure::Rejected);
    failures.record(RegionProbeFailure::Other);

    let failure = failures.into_login_failure();
    assert!(!failure.retryable);
    assert_eq!(failure.error.code, "err.login.credentials_rejected");
}

/// Block all third-party logins: advanced data accepts only Zepp email / phone number + password. When we block,
/// we must also identify it as third-party, so the UI can explain why the click did nothing.
#[test]
fn third_party_sign_in_is_blocked_and_recognised() {
    for host in THIRD_PARTY_AUTH_HOSTS {
        let url = format!("https://{host}/oauth");
        assert!(!is_allowed_login_url(&url), "{url} must be blocked");
        assert!(
            is_third_party_auth_page(&url),
            "{url} should be third-party"
        );
    }
    // Zepp's own universal login page and verification code page are allowed as usual.
    assert!(is_allowed_login_url(
        "https://user.zepp.com/universalLogin/index.html#/login"
    ));
    assert!(is_allowed_login_url("https://watchface.zepp.com/"));
    assert!(!is_third_party_auth_page("https://watchface.zepp.com/"));
    assert!(!is_third_party_auth_page("not a url"));
}

/// An account mismatch exists only when both sides have an ID and the IDs differ. A side that is not configured does not count.
#[test]
fn only_two_known_different_accounts_count_as_a_mismatch() {
    assert!(crate::commands::accounts_differ("1234567890", "9876543210"));
    assert!(!crate::commands::accounts_differ(
        "1234567890",
        " 1234567890 "
    ));
    assert!(!crate::commands::accounts_differ("", "1234567890"));
    assert!(!crate::commands::accounts_differ("1234567890", ""));
}

#[test]
fn a_stale_login_epoch_must_not_close_a_newer_window() {
    assert!(!closer_epoch_owns_the_window(1, 2));
    assert!(closer_epoch_owns_the_window(7, 7));
    assert!(!closer_epoch_owns_the_window(8, 7));
}

/// While the old window still holds the label, never build a new window.
///
/// After `close()` the label is not released at once (see `close_login_window_and_wait` for why).
/// While it is held, the only correct action is to keep waiting. If this check lets it through, the user gets
/// "Cannot open the login window", and the working window that they had is already closed.
///
/// The premise "the label still exists after close()" has no test: to pin it we need the `tauri` mock
/// runtime. Adding `tauri = { features = ["test"] }` to dev-dependencies enables `tauri/test` in the
/// whole test build, and on Windows the test binary then fails to load (STATUS_ENTRYPOINT_NOT_FOUND, no
/// test runs). It is not worth breaking the whole Windows gate for one assertion about third-party
/// library behavior.
#[test]
fn a_new_login_window_waits_until_the_old_label_is_released() {
    // The label is still held: no matter how long we waited, it is not "ready to build".
    assert_eq!(
        close_wait_step(true, Duration::ZERO),
        CloseWait::KeepWaiting
    );
    assert_eq!(
        close_wait_step(true, LOGIN_WINDOW_CLOSE_TIMEOUT - LOGIN_WINDOW_CLOSE_POLL),
        CloseWait::KeepWaiting
    );

    // Allow only when the label is free. A first login has no old window and must not be slowed by this wait.
    assert_eq!(close_wait_step(false, Duration::ZERO), CloseWait::Released);

    // The wait must end. If the main thread is really stuck, give "wait and try again", but never let
    // the command hang forever.
    assert_eq!(
        close_wait_step(true, LOGIN_WINDOW_CLOSE_TIMEOUT),
        CloseWait::TimedOut
    );
    assert_eq!(
        close_wait_step(true, LOGIN_WINDOW_CLOSE_TIMEOUT + Duration::from_secs(1)),
        CloseWait::TimedOut
    );
    // If the label becomes free after the timeout, still allow it. Do not report an error.
    assert_eq!(
        close_wait_step(false, LOGIN_WINDOW_CLOSE_TIMEOUT + Duration::from_secs(1)),
        CloseWait::Released
    );
}

/// While the user types a password on the main login page, never navigate away from the page.
///
/// This matches user feedback: halfway through the email and password, or while copying a verification code
/// from email, the page changed to the privacy page (clear data / delete account), and the login had to restart.
#[test]
fn fallback_needs_the_full_wait_and_fires_at_most_once() {
    let long_enough = FALLBACK_AFTER + Duration::from_secs(5);

    assert!(fallback_is_due(long_enough, false));
    assert!(!fallback_is_due(Duration::from_secs(5), false));
    assert!(!fallback_is_due(long_enough, true));
}

/// Once the user enters a third-party login flow, never navigate away from the page.
///
/// This matches user feedback: while copying a Xiaomi verification code from email, the page changed to the
/// privacy page (clear data / delete account), and the login had to restart. A URL that is not the page we
/// opened is proof that the user is signing in, and the timer has no say.
#[test]
fn only_the_page_we_opened_may_be_navigated_away() {
    assert!(is_primary_login_page("https://watchface.zepp.com"));
    assert!(is_primary_login_page("https://watchface.zepp.com/"));
    assert!(is_primary_login_page(
        "https://watchface.zepp.com/login?from=app"
    ));

    assert!(!is_primary_login_page(
        "https://account.xiaomi.com/oauth2/authorize"
    ));
    assert!(!is_primary_login_page(
        "https://accounts.google.com/o/oauth2/auth"
    ));
    assert!(!is_primary_login_page(
        "https://www.facebook.com/dialog/oauth"
    ));
    assert!(!is_primary_login_page(
        "https://open.weixin.qq.com/connect/qrconnect"
    ));
    assert!(!is_primary_login_page(
        "https://user.huami.com/privacy2/index.html"
    ));
}

/// Credentials in localStorage must work the same as in cookies.
///
/// The watch face site is a frontend app, and it is normal for it to write login data to localStorage. Then
/// `document.cookie` and the webview cookie jar cannot see it, and the user must open developer tools
/// to copy the App Token. People on Reddit really did this.
#[test]
fn credentials_from_web_storage_parse_like_cookies() {
    let raw = r#"{"token_info":{"user_id":"55","app_token":"from-local-storage"}}"#;
    let entries = vec![("hm-user-login-info".to_string(), raw.to_string())];
    let got = parse_login_cookies(&entries).expect("storage credentials");
    assert_eq!(got.user_id, "55");
    assert_eq!(got.app_token, "from-local-storage");
}

/// "Not signed in yet" and "signed in but we did not read the credentials" must be distinct.
///
/// If not, the second case waits silently until the 15-minute timeout and then says "Login timed out.
/// Try again." No number of retries fixes it: the correct step is to enter the Token manually.
#[test]
fn a_signed_in_page_is_told_apart_from_the_login_page() {
    let none: Vec<(String, String)> = Vec::new();

    // Still on the login page, and no cookie name that appears only after login.
    assert!(!page_looks_signed_in(
        "https://watchface.zepp.com/login",
        &none
    ));
    assert!(!page_looks_signed_in(
        "https://account.xiaomi.com/oauth2/authorize",
        &none
    ));

    // Left the login page.
    assert!(page_looks_signed_in(
        "https://watchface.zepp.com/dashboard",
        &none
    ));

    // Or a cookie name that appears only after login is present, even before we parse credentials.
    assert!(page_looks_signed_in(
        "https://user.huami.com/privacy2/index.html",
        &[("apptoken".to_string(), "whatever".to_string())]
    ));

    // Generic cookie names on a third-party OAuth page do not mean Zepp signed in.
    assert!(!page_looks_signed_in(
        "https://accounts.google.com/o/oauth2/auth",
        &[
            ("session".to_string(), "google-session".to_string()),
            ("token".to_string(), "google-token".to_string()),
            ("SID".to_string(), "x".to_string()),
        ]
    ));
    assert!(!page_looks_signed_in(
        "https://www.facebook.com/dialog/oauth",
        &[
            ("xs".to_string(), "fb".to_string()),
            ("c_user".to_string(), "1".to_string())
        ]
    ));
}

#[test]
fn parses_hm_user_login_info_token_info() {
    let raw = r#"{"token_info":{"user_id":"12345","app_token":"tok_abc"}}"#;
    let cookies = vec![("hm-user-login-info".into(), raw.into())];
    let got = parse_login_cookies(&cookies).expect("login info");
    assert_eq!(got.user_id, "12345");
    assert_eq!(got.app_token, "tok_abc");
}

#[test]
fn parses_url_encoded_login_info() {
    let encoded = "%7B%22token_info%22%3A%7B%22user_id%22%3A%22111%22%2C%22app_token%22%3A%22secret-token%22%7D%7D";
    let cookies = vec![("hm-user-login-info".into(), encoded.into())];
    let got = parse_login_cookies(&cookies).expect("encoded login info");
    assert_eq!(got.user_id, "111");
    assert_eq!(got.app_token, "secret-token");
}

#[test]
fn parses_nested_string_token_info_and_numeric_user() {
    let raw =
        r#"{"token_info":"{\"user_id\":987654,\"app_token\":\"nested-tok\",\"region\":\"us\"}"}"#;
    let cookies = vec![("hm-user-login-info".into(), raw.into())];
    let got = parse_login_cookies(&cookies).expect("nested token_info");
    assert_eq!(got.user_id, "987654");
    assert_eq!(got.app_token, "nested-tok");
    assert_eq!(got.region_hint.as_deref(), Some("us"));
}

#[test]
fn parses_userid_and_apptoken_cookies() {
    let cookies = vec![
        ("foo".into(), "bar".into()),
        ("userid".into(), "user_99".into()),
        ("apptoken".into(), "app-token-value".into()),
    ];
    let got = parse_login_cookies(&cookies).expect("pair cookies");
    assert_eq!(got.user_id, "user_99");
    assert_eq!(got.app_token, "app-token-value");
}

#[test]
fn current_pair_overrides_stale_bundled_login_info() {
    let cookies = vec![
        (
            "hm-user-login-info".into(),
            r#"{"token_info":{"user_id":"old","app_token":"old-token"}}"#.into(),
        ),
        ("userid".into(), "new-user".into()),
        ("apptoken".into(), "new+token".into()),
        (
            "wf_baseUrl".into(),
            "https://api-mifit-sg2.huami.com".into(),
        ),
    ];
    let got = parse_login_cookies(&cookies).expect("current pair");
    assert_eq!(got.user_id, "new-user");
    assert_eq!(got.app_token, "new+token");
    assert_eq!(
        got.region_hint.as_deref(),
        Some("https://api-mifit-sg2.huami.com")
    );
}

#[test]
fn region_host_can_be_read_from_domains_json() {
    let cookies = vec![
        ("userid".into(), "42".into()),
        ("apptoken".into(), "token".into()),
        (
            "domains".into(),
            r#"[{"cnames":["api-mifit-de2.huami.com"]}]"#.into(),
        ),
    ];
    let got = parse_login_cookies(&cookies).expect("domains candidate");
    assert_eq!(
        got.region_hint.as_deref(),
        Some("https://api-mifit-de2.huami.com")
    );
}

#[test]
fn fresher_page_values_are_not_overwritten_by_cookie_store_values() {
    let mut pairs = vec![
        ("userid".into(), "current".into()),
        ("apptoken".into(), "current-token".into()),
    ];
    append_missing_pairs(
        &mut pairs,
        vec![
            ("userid".into(), "stale".into()),
            ("apptoken".into(), "stale-token".into()),
            ("cname".into(), "api-mifit-us2.huami.com".into()),
        ],
    );
    let got = parse_login_cookies(&pairs).expect("page candidate");
    assert_eq!(got.user_id, "current");
    assert_eq!(got.app_token, "current-token");
}

#[test]
fn parses_document_cookie_header() {
    let header = "foo=bar; userid=42; apptoken=tkn";
    let got = parse_login_cookies(&parse_cookie_header(header)).expect("header");
    assert_eq!(got.user_id, "42");
    assert_eq!(got.app_token, "tkn");
}

#[test]
fn rejects_incomplete_or_unsafe_cookies() {
    assert!(parse_login_cookies(&[("userid".into(), "42".into())]).is_none());
    assert!(parse_login_cookies(&[
        ("userid".into(), "bad/id".into()),
        ("apptoken".into(), "tok".into()),
    ])
    .is_none());
    assert!(parse_login_cookies(&[(
        "hm-user-login-info".into(),
        r#"{"token_info":{"login_token":"nope"}}"#.into()
    ),])
    .is_none());
}

#[test]
fn region_hint_stays_on_allow_list() {
    assert_eq!(
        hosts_from_region_hint("https://api-mifit-cn3.zepp.com"),
        vec!["https://api-mifit-cn3.zepp.com".to_string()]
    );
    let us = hosts_from_region_hint("us");
    assert!(us.iter().all(|host| host.contains("-us")));
    assert!(hosts_from_region_hint("https://evil.example").is_empty());
    assert_eq!(
        hosts_from_region_hint("https://api-mifit-eu2.zepp.com"),
        vec!["https://api-mifit-eu2.zepp.com".to_string()]
    );
    assert!(REGION_HOST_ALLOWLIST.contains(&"https://api-mifit-sg2.huami.com"));
    assert!(REGION_HOST_ALLOWLIST.contains(&"https://api-mifit-de2.huami.com"));
}

#[test]
fn native_login_title_follows_the_interface_locale() {
    assert_eq!(login_window_title("en"), "Sign in to Zepp");
    assert_eq!(login_window_title("en-US"), "Sign in to Zepp");
    // Chinese is not supported. It falls back to English.
    for tag in ["zh", "zh-CN", "zh-TW", "zh-Hant-TW", "zh-SG"] {
        assert_eq!(login_window_title(tag), "Sign in to Zepp", "{tag}");
    }
    // Region variants map to the base language. Unknown tags fall back to English.
    assert_eq!(login_window_title("es"), "Iniciar sesión en Zepp");
    assert_eq!(login_window_title("pt-BR"), "Entrar no Zepp");
    assert_eq!(login_window_title("pt"), "Iniciar sessão no Zepp");
    assert_eq!(login_window_title("de-AT"), "Bei Zepp anmelden");
    assert_eq!(login_window_title("ru"), "Войти в Zepp");
    assert_eq!(login_window_title("hi-IN"), "Zepp में साइन इन करें");
    assert_eq!(login_window_title("fr"), "Se connecter à Zepp");
    assert_eq!(login_window_title("nl"), "Inloggen bij Zepp");
    assert_eq!(login_window_title("pl-PL"), "Zaloguj się do Zepp");
    assert_eq!(login_window_title("ja-JP"), "Sign in to Zepp");
}

#[test]
fn login_status_url_drops_oauth_secrets() {
    let safe = safe_login_page_url(
        "https://account-us.zepp.com/callback?code=secret&state=private#access_token",
    );
    assert_eq!(safe, "https://account-us.zepp.com/callback");
    assert!(!safe.contains("secret"));
    assert!(!safe.contains("private"));
    assert!(!safe.contains("access_token"));
}

#[test]
fn region_probe_error_classification_distinguishes_rejection() {
    assert_eq!(
        classify_region_probe_error(&ZeppBridgeError::NeedsReauth("HTTP 401".into())),
        RegionProbeFailure::Rejected
    );
    assert_eq!(
        classify_region_probe_error(&ZeppBridgeError::Unavailable("HTTP 404".into())),
        RegionProbeFailure::Other
    );
    let rejected = RegionProbeFailures {
        rejected: 1,
        transient: 2,
        other: 3,
    }
    .into_login_failure();
    assert_eq!(rejected.error.code, "err.login.credentials_rejected");
    // The credentials were rejected. A hundred more tries give the same result, so do not keep the window idle.
    assert!(!rejected.retryable);

    // Only the network case is worth a retry in place, and it must keep the login window: it is an isolated
    // session, and closing it means the user redoes the verification code and QR scan.
    let unreachable = RegionProbeFailures {
        transient: 2,
        ..Default::default()
    }
    .into_login_failure();
    assert_eq!(unreachable.error.code, "err.login.region_unreachable");
    assert!(unreachable.retryable);

    let other = RegionProbeFailures {
        other: 1,
        ..Default::default()
    }
    .into_login_failure();
    assert_eq!(other.error.code, "err.login.region_probe_failed");
    assert!(!other.retryable);
}

/// A value larger than the system credential manager capacity cannot be an App Token.
///
/// Windows Credential Manager stores only 1280 UTF-16 code units. Before, this allowed up to 16 KB, so a
/// whole JSON block from page storage was taken as a token and failed only at the save step. The error
/// ("Cannot write to Windows Credential Manager") did not mention length.
#[test]
fn an_oversized_candidate_is_not_mistaken_for_an_app_token() {
    let real_token = "a".repeat(96);
    assert_eq!(
        sanitize_app_token(&real_token).as_deref(),
        Some(real_token.as_str())
    );

    let blob = "x".repeat(crate::auth::CREDENTIAL_MAX_UTF16_UNITS + 1);
    assert_eq!(sanitize_app_token(&blob), None);

    // After we reject the long candidate, the real token in hm-user-login-info gets its turn.
    let cookies = vec![
        ("apptoken".to_string(), blob),
        (
            "hm-user-login-info".to_string(),
            r#"{"token_info":{"user_id":"77","app_token":"the-real-token"}}"#.to_string(),
        ),
    ];
    let got = parse_login_cookies(&cookies).expect("falls back to the bundled token");
    assert_eq!(got.user_id, "77");
    assert_eq!(got.app_token, "the-real-token");
}

#[test]
fn login_navigation_allow_list() {
    assert!(is_allowed_login_url("https://watchface.zepp.com/"));
    assert!(is_allowed_login_url(
        "https://user.huami.com/privacy2/index.html"
    ));
    // Advanced data accepts only email + password: all third-party login pages are kept out of the window.
    assert!(!is_allowed_login_url(
        "https://account.xiaomi.com/oauth2/authorize"
    ));
    assert!(!is_allowed_login_url(
        "https://open.weixin.qq.com/connect/qrconnect"
    ));
    assert!(!is_allowed_login_url(
        "https://accounts.google.com/o/oauth2/auth"
    ));
    assert!(!is_allowed_login_url(
        "https://www.facebook.com/dialog/oauth"
    ));
    assert!(!is_allowed_login_url(
        "https://account-us.amazfit.com/v1/accounts/connect/facebook/callback"
    ));
    assert!(!is_allowed_login_url("about:blank"));
    assert!(!is_allowed_login_url(
        "data:text/html,<script>alert(1)</script>"
    ));
    assert!(!is_allowed_login_url("https://example.com/"));
    assert!(!is_allowed_login_url("http://watchface.zepp.com/"));
    assert!(!is_allowed_login_url(
        "https://evil.xiaomi.com/oauth2/authorize"
    ));
    assert!(!is_allowed_login_url("https://facebook.com/dialog/oauth"));
}

#[test]
fn blocked_login_log_omits_query_and_fragment() {
    let url = reqwest::Url::parse(
        "https://example.com/oauth/callback?code=secret&state=private#access_token",
    )
    .unwrap();
    let fields = login_url_log_fields(&url);
    assert_eq!(fields, "host=example.com path=/oauth/callback");
    assert!(!fields.contains("secret"));
    assert!(!fields.contains("private"));
    assert!(!fields.contains("access_token"));
}

#[test]
fn login_window_has_no_opener_permission() {
    let main: serde_json::Value =
        serde_json::from_str(include_str!("../../../capabilities/default.json")).unwrap();
    assert_eq!(main["windows"], serde_json::json!(["main"]));

    let login: serde_json::Value =
        serde_json::from_str(include_str!("../../../capabilities/zepp-login.json")).unwrap();
    assert_eq!(login["windows"], serde_json::json!(["zepp-login"]));
    assert_eq!(
        login["permissions"],
        serde_json::json!(["core:window:allow-close", "core:event:allow-listen"])
    );
    assert!(login.get("remote").is_none());
    assert!(login["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .all(|permission| permission.as_str() != Some("core:default")
            && permission.as_str() != Some("opener:default")
            && permission["identifier"] != "opener:allow-open-url"));
}

/// On an account mismatch, the UI shows the two accounts side by side with only the last four digits. The full ID does not go into the event payload.
#[test]
fn an_account_conflict_shows_only_the_last_four_digits() {
    use crate::ipc_types::{mask_user_id, AccountBrief, AccountPair};
    assert_eq!(mask_user_id(" 1234567890 "), "••••7890");
    let mut status = LoginStatus::new("failed", "err.login.other_account", "", "");
    status.conflict = Some(AccountPair {
        official: AccountBrief {
            user_id_masked: mask_user_id("1234567890"),
            nickname: Some("Runner".into()),
        },
        advanced: AccountBrief {
            user_id_masked: mask_user_id("9876543210"),
            nickname: None,
        },
    });
    let json = serde_json::to_string(&status).unwrap();
    assert!(!json.contains("1234567890") && !json.contains("9876543210"));
    assert!(json.contains("••••7890") && json.contains("••••3210"));
}
