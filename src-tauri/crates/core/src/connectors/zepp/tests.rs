use super::*;

/// `code = 1` is success and passes through as before.
///
/// Three streams (workouts / workout_detail / sleep) wrap their payloads in
/// `{ code, message, data }`. In one real database, all 1075 wrapped payloads had code
/// 1 and message "success", so this check must not reject any of them.
#[test]
fn a_successful_envelope_passes_through_untouched() {
    let payload = serde_json::json!({
        "code": 1,
        "message": "success",
        "data": { "next": -1, "summary": [] }
    });
    assert!(classify_business_code(&payload).is_none());
}

/// The other four streams have no top-level `code`. This function must ignore them fully.
#[test]
fn payloads_without_the_envelope_are_never_touched() {
    assert!(classify_business_code(&serde_json::json!({ "items": [1, 2, 3] })).is_none());
    assert!(classify_business_code(&serde_json::json!([1, 2, 3])).is_none());
    // A string `code` (not this envelope) must not count as a business code either.
    assert!(classify_business_code(&serde_json::json!({ "code": "ABC" })).is_none());
}

/// HTTP 200 with a non-success code must become its own error, not "data could not be parsed".
///
/// This is the shape of the "I signed in and the account is empty" reports: transport succeeds,
/// the business layer rejects, `data` is missing, and the normalizer throws an unclear parse error.
#[test]
fn a_rejected_envelope_becomes_its_own_error_with_the_code_visible() {
    let payload = serde_json::json!({ "code": -1, "message": "token invalid" });
    let error = classify_business_code(&payload).expect("a non-success code must be caught");
    assert!(matches!(
        error,
        ZeppBridgeError::CloudRejected { code: -1, .. }
    ));
    assert_eq!(error.code(), "err.core.cloud_rejected");
    // The cloud's own text must appear in the user-facing message. It is the only way
    // the next feedback report can identify the exact failure code.
    let message = error.user_message();
    assert!(message.contains("-1"), "{message}");
    assert!(message.contains("token invalid"), "{message}");
    // But it must not be forced to "needs re-auth". No failure code was observed, and sending
    // the user to scan the QR code again on a guess is worse than this bug.
    assert!(!error.needs_reauth());
}

/// With no message from the cloud, do not crash. Give a short explanation.
#[test]
fn a_rejected_envelope_without_a_message_still_reports_the_code() {
    let error =
        classify_business_code(&serde_json::json!({ "code": 401 })).expect("a non-success code must be caught");
    assert!(error.user_message().contains("401"));
}

#[test]
fn host_validation_accepts_known_regional_variants() {
    assert_eq!(
        validate_region_host("https://api-mifit-us3.zepp.com").unwrap(),
        "https://api-mifit-us3.zepp.com"
    );
    assert_eq!(
        validate_region_host("api-mifit.huami.com").unwrap(),
        "https://api-mifit.huami.com"
    );
}

#[test]
fn host_validation_rejects_unsafe_forms() {
    for host in [
        "http://api-mifit.huami.com",
        "https://user:pass@api-mifit.huami.com",
        "https://api-mifit.huami.com:443",
        "https://api-mifit.huami.com/path",
        "https://api-mifit.huami.com?q=1",
        "https://api-mifit.huami.com#fragment",
        "https://api-mifit.evil.example",
    ] {
        assert!(validate_region_host(host).is_err(), "accepted {host}");
    }
}

#[test]
fn status_classification_is_explicit() {
    assert!(matches!(
        classify_status(401),
        Some(ZeppBridgeError::NeedsReauth(_))
    ));
    assert!(matches!(
        classify_status(404),
        Some(ZeppBridgeError::Unavailable(_))
    ));
    assert!(matches!(
        classify_status(503),
        Some(ZeppBridgeError::RetryExhausted { .. })
    ));
    assert!(classify_status(204).is_none());
}

#[test]
fn detail_params_reject_injection() {
    assert!(validate_track_id("1700000000").is_ok());
    assert!(validate_track_id("../x").is_err());
    assert!(validate_detail_source("run.gps").is_ok());
    assert!(validate_detail_source("a/b").is_err());
}

#[test]
fn api_paths_reject_scheme_relative_and_query() {
    assert!(validate_api_path("/users/1/devices").is_ok());
    assert!(validate_api_path("//evil.example/users/1").is_err());
    assert!(validate_api_path("/users/1?x=1").is_err());
    assert!(validate_api_path("/users/1#frag").is_err());
    assert!(validate_api_path("users/1").is_err());
    assert!(validate_api_path("/users\\1").is_err());
}

#[test]
fn redirect_origin_compares_scheme_host_and_port() {
    let base = Url::parse("https://api-mifit.huami.com/").unwrap();
    assert!(same_region_origin(
        &Url::parse("https://api-mifit.huami.com/v1/next").unwrap(),
        &base
    ));
    assert!(!same_region_origin(
        &Url::parse("https://api-mifit.huami.com:8443/v1/next").unwrap(),
        &base
    ));
    assert!(!same_region_origin(
        &Url::parse("http://api-mifit.huami.com/v1/next").unwrap(),
        &base
    ));
    assert!(!same_region_origin(
        &Url::parse("https://evil.example/v1/next").unwrap(),
        &base
    ));
}

#[test]
fn member_id_is_the_account_holder_or_digits() {
    assert_eq!(validate_member_id("-1").unwrap(), "-1");
    assert_eq!(validate_member_id("42").unwrap(), "42");
    assert!(validate_member_id("../x").is_err());
    assert!(validate_member_id("").is_err());
    assert!(validate_member_id("1/2").is_err());
    assert!(validate_member_id("abc").is_err());
}
