use super::client::TokenBody;
use super::*;
use crate::auth::CredentialBackend;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct MemoryBackend {
    entries: Mutex<HashMap<String, String>>,
    named: bool,
}

impl MemoryBackend {
    fn named() -> Arc<Self> {
        Arc::new(Self {
            named: true,
            ..Self::default()
        })
    }
}

impl CredentialBackend for MemoryBackend {
    fn set(&self, user_id: &str, token: &str) -> std::result::Result<(), String> {
        self.entries
            .lock()
            .unwrap()
            .insert(user_id.into(), token.into());
        Ok(())
    }
    fn get(&self, user_id: &str) -> std::result::Result<Option<String>, String> {
        Ok(self.entries.lock().unwrap().get(user_id).cloned())
    }
    fn delete(&self, user_id: &str) -> std::result::Result<(), String> {
        self.entries.lock().unwrap().remove(user_id);
        Ok(())
    }
    fn supports_named_entries(&self) -> bool {
        self.named
    }
}

fn temp_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zeppbridge-official-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn tokens() -> OfficialTokens {
    OfficialTokens {
        access_token: "access-fixture".into(),
        refresh_token: Some("refresh-fixture".into()),
        expires_at: Some(10_000),
        user_id: "3000000002".into(),
    }
}

/// The address bar may only show state and the hash of claim_secret, never claim_secret itself.
/// Otherwise anyone who can see the browser history could claim the token for the user.
#[test]
fn the_start_url_carries_the_claim_hash_never_the_claim_secret() {
    let request = AuthorizationRequest::new().unwrap();
    assert_eq!(request.state.len(), 43);
    assert_eq!(request.claim_secret().len(), 43);
    assert_ne!(request.state, request.claim_secret());
    let url = request.start_url();
    assert!(url.starts_with("https://zeppbridge.com/api/zepp/oauth/start?state="));
    assert!(!url.contains(request.claim_secret()));
    let hash = hex::encode(Sha256::digest(request.claim_secret().as_bytes()));
    assert!(url.ends_with(&format!("&claim={hash}")));
}

#[test]
fn tokens_refresh_ahead_of_expiry_and_never_show_up_in_debug_output() {
    let tokens = tokens();
    assert!(!tokens.needs_refresh(10_000 - REFRESH_MARGIN_SECONDS - 1));
    assert!(tokens.needs_refresh(10_000 - REFRESH_MARGIN_SECONDS));
    let without_expiry = OfficialTokens {
        expires_at: None,
        ..tokens.clone()
    };
    assert!(!without_expiry.needs_refresh(i64::MAX));
    let debug = format!("{tokens:?}");
    assert!(!debug.contains("access-fixture"));
    assert!(!debug.contains("refresh-fixture"));
}

#[test]
fn a_token_body_without_an_access_token_is_not_a_connection() {
    let body: TokenBody = serde_json::from_value(serde_json::json!({
        "status": "ready", "access_token": "", "user_id": "1"
    }))
    .unwrap();
    assert!(body.into_tokens(0, None).is_none());

    // A refresh response can omit user_id; the old one stays.
    let body: TokenBody = serde_json::from_value(serde_json::json!({
        "status": "ready", "access_token": "a", "expires_in": 3600
    }))
    .unwrap();
    let fresh = body.into_tokens(100, Some("3000000002")).unwrap();
    assert_eq!(fresh.user_id, "3000000002");
    assert_eq!(fresh.expires_at, Some(3700));
}

#[test]
fn the_store_keeps_secrets_out_of_the_data_directory() {
    let dir = temp_dir();
    let backend = MemoryBackend::named();
    let store = OfficialStore::with_backend(&dir, backend.clone());
    assert!(store.load().unwrap().is_none());

    store.save(&tokens(), 42).unwrap();
    assert_eq!(store.load().unwrap(), Some(tokens()));
    let meta_text = std::fs::read_to_string(dir.join("official.json")).unwrap();
    assert!(!meta_text.contains("access-fixture"));
    assert!(!meta_text.contains("refresh-fixture"));
    assert_eq!(store.meta().unwrap().unwrap().connected_at, 42);

    // Saving again after a refresh keeps the original connect time.
    store.save(&tokens(), 99).unwrap();
    assert_eq!(store.meta().unwrap().unwrap().connected_at, 42);

    store.mark_needs_reauth().unwrap();
    assert!(store.load().unwrap().is_none());
    assert!(store.meta().unwrap().unwrap().needs_reauth);
    assert!(backend.entries.lock().unwrap().is_empty());

    store.save(&tokens(), 100).unwrap();
    store.clear().unwrap();
    assert!(store.meta().unwrap().is_none());
    assert!(backend.entries.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// The env-var backend returns the legacy token for any name: an official token stored there would read back as the legacy token.
#[test]
fn a_store_that_cannot_keep_named_entries_refuses_official_tokens() {
    let dir = temp_dir();
    let store = OfficialStore::with_backend(&dir, Arc::new(MemoryBackend::default()));
    assert!(store.save(&tokens(), 1).is_err());
    assert!(store.meta().unwrap().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/* ---------- Commit generation and compare-and-swap write-back ---------- */

/// The user cancels, disconnects or starts again during claim / profile: the old result must not write credentials.
/// After a disconnect, the same old result cannot write back either. This is the only test that bumps the generation; keep it in one piece.
#[test]
fn a_superseded_login_never_writes_and_disconnect_voids_it_too() {
    let dir = temp_dir();
    let backend = MemoryBackend::named();
    let store = OfficialStore::with_backend(&dir, backend.clone());

    let stale = invalidate_pending_logins();
    invalidate_pending_logins(); // The user cancelled and started again
    assert!(!store.save_login(&tokens(), Some("old"), 1, stale).unwrap());
    assert!(store.meta().unwrap().is_none());
    assert!(backend.entries.lock().unwrap().is_empty());

    let live = current_generation();
    assert!(store.save_login(&tokens(), Some("runner"), 1, live).unwrap());
    assert_eq!(
        store.meta().unwrap().unwrap().nickname.as_deref(),
        Some("runner")
    );

    store.disconnect().unwrap();
    assert!(!store.save_login(&tokens(), None, 2, live).unwrap());
    assert!(store.meta().unwrap().is_none());
    assert!(backend.entries.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// A refresh write-back is compare-and-swap on "the store still holds that refresh token". When a concurrent refresh
/// already stored a new token, neither this result nor this refusal can overwrite it.
#[test]
fn refresh_results_only_land_on_the_token_they_came_from() {
    let dir = temp_dir();
    let store = OfficialStore::with_backend(&dir, MemoryBackend::named());
    let used = tokens();
    store.save(&used, 1).unwrap();

    let newer = OfficialTokens {
        access_token: "access-newer".into(),
        refresh_token: Some("refresh-newer".into()),
        ..used.clone()
    };
    assert!(store.replace_if_current(&used, &newer, 2).unwrap());

    // The old refresh arrives late: it does not overwrite and cannot mark the account as needing reauthorization.
    let late = OfficialTokens {
        access_token: "access-late".into(),
        refresh_token: Some("refresh-late".into()),
        ..used.clone()
    };
    assert!(!store.replace_if_current(&used, &late, 3).unwrap());
    assert!(!store.mark_needs_reauth_if_current(&used).unwrap());
    assert_eq!(store.load().unwrap(), Some(newer.clone()));

    // After a disconnect, a refresh result cannot write back either.
    store.clear().unwrap();
    assert!(!store.replace_if_current(&newer, &late, 4).unwrap());
    assert!(store.meta().unwrap().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// Account changed: the previous account's credential entries are deleted too, so no unreferenced token stays.
#[test]
fn switching_accounts_removes_the_previous_accounts_secrets() {
    let dir = temp_dir();
    let backend = MemoryBackend::named();
    let store = OfficialStore::with_backend(&dir, backend.clone());
    store.save(&tokens(), 1).unwrap();
    let other = OfficialTokens {
        user_id: "3000000009".into(),
        ..tokens()
    };
    store.save(&other, 2).unwrap();
    let keys: Vec<String> = backend.entries.lock().unwrap().keys().cloned().collect();
    assert!(
        keys.iter().all(|key| key.contains("3000000009")),
        "{keys:?}"
    );
    assert_eq!(keys.len(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

/// A local fake relay: counts each refresh request and returns a new token after a deliberate 300ms delay.
async fn slow_refresh_relay() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0u8; 8192];
                let _ = socket.read(&mut buffer).await;
                let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                let body = format!(
                    r#"{{"status":"ready","access_token":"access-{n}","refresh_token":"refresh-{n}","expires_in":3600}}"#
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    (base, hits)
}

/// A status read and a sync both find the token near expiry: only one refresh is sent, and both get the same new token.
#[tokio::test]
async fn concurrent_refreshes_are_single_flight() {
    let dir = temp_dir();
    let store = OfficialStore::with_backend(&dir, MemoryBackend::named());
    store.save(&tokens(), 1).unwrap();
    let (base, hits) = slow_refresh_relay().await;
    let client = OfficialClient::with_bases(&base, &base).unwrap();
    let now = 10_000; // Already expired, must refresh

    let (a, b) = tokio::join!(
        fresh_tokens(&store, &client, now),
        fresh_tokens(&store, &client, now)
    );
    let (a, b) = (a.unwrap().unwrap(), b.unwrap().unwrap());
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(a, b);
    assert_eq!(a.refresh_token.as_deref(), Some("refresh-1"));
    assert_eq!(store.load().unwrap(), Some(a));
    let _ = std::fs::remove_dir_all(dir);
}

/* ---------- Fetch completeness (local fake server) ---------- */

use super::fake_server::{query_param, serve, Reply};
use super::fetch::{OfficialFetcher, OfficialKind};

fn day_of(text: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

async fn fetch(
    route: impl Fn(&str) -> Reply + Send + Sync + 'static,
    start: &str,
    end: &str,
) -> (
    Result<super::fetch::ChunkOutcome>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let (base, hits) = serve(route).await;
    let client = OfficialClient::with_bases(&base, &base).unwrap();
    let fetcher = OfficialFetcher {
        client: &client,
        access_token: "token-fixture",
        time_zone: "Asia/Shanghai".into(),
    };
    let outcome = fetcher
        .fetch_chunk(OfficialKind::ActivityDaily, day_of(start), day_of(end))
        .await;
    (outcome, hits)
}

/// The server returns only the cursor day each time. After 3 follow-ups it is still truncated: the 4 days fetched are
/// returned as usual, the gap is stated, and it is not reported as "fully complete".
#[tokio::test]
async fn a_chunk_that_stays_truncated_reports_its_gap() {
    let (outcome, hits) = fetch(
        |target| {
            let day = query_param(target, "startDate").unwrap();
            Reply::json(
                200,
                format!(r#"{{"items":[{{"date":"{day}","steps":1}}]}}"#),
            )
        },
        "2026-09-01",
        "2026-09-30",
    )
    .await;
    let outcome = outcome.unwrap();
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 4);
    assert_eq!(outcome.records.len(), 4);
    let gap = outcome.gap.expect("truncation at the limit must be marked incomplete");
    assert!(gap.contains("2026-09-05"), "{gap}");
}

/// A business error inside HTTP 200 is not "no data".
#[tokio::test]
async fn a_business_error_inside_http_200_is_an_error() {
    let (outcome, _) = fetch(
        |_| {
            Reply::json(
                200,
                r#"{"code":-50000,"message":"synthetic business error"}"#,
            )
        },
        "2026-09-01",
        "2026-09-07",
    )
    .await;
    assert!(matches!(
        outcome,
        Err(ZeppBridgeError::CloudRejected { code: -50000, .. })
    ));
}

/// Every unknown shape is an error; only `items: []` is a valid empty result.
#[tokio::test]
async fn only_an_empty_items_array_means_no_data() {
    for body in [r#"{}"#, r#"{"items":null}"#, r#"{"items":{}}"#, r#"[]"#] {
        let (outcome, _) = fetch(move |_| Reply::json(200, body), "2026-09-01", "2026-09-07").await;
        assert!(
            matches!(outcome, Err(ZeppBridgeError::ParseError(_))),
            "{body} must not be treated as empty data"
        );
    }
    let (outcome, _) = fetch(
        |_| Reply::json(200, r#"{"items":[]}"#),
        "2026-09-01",
        "2026-09-07",
    )
    .await;
    let outcome = outcome.unwrap();
    assert!(outcome.records.is_empty());
    assert!(outcome.gap.is_none());
}

/// Records without a date must not disappear silently.
#[tokio::test]
async fn items_without_a_date_are_counted_in_the_gap() {
    let (outcome, _) = fetch(
        |_| {
            Reply::json(
                200,
                r#"{"items":[{"date":"2026-09-07","steps":1},{"steps":2}]}"#,
            )
        },
        "2026-09-01",
        "2026-09-07",
    )
    .await;
    let outcome = outcome.unwrap();
    assert_eq!(outcome.records.len(), 1);
    assert!(outcome.gap.unwrap().contains("1 official"));
}

/// An oversized response without Content-Length stops at the limit and gives an actionable message.
#[tokio::test]
async fn an_oversized_body_without_a_length_is_cut_off() {
    let (outcome, _) = fetch(
        |_| Reply {
            status: 200,
            body: vec![b' '; 33 * 1024 * 1024],
            length: false,
        },
        "2026-09-01",
        "2026-09-07",
    )
    .await;
    match outcome {
        Err(ZeppBridgeError::ParseError(message)) => assert!(message.contains("MB"), "{message}"),
        other => panic!("an oversized response must be an error: {other:?}"),
    }
}

/// #145: disconnect must work even when the keychain delete fails. The metadata is the only source of "connected" and is deleted anyway;
/// the remaining token has no reference, and the store's raw text is returned for the log.
#[test]
fn disconnect_still_disconnects_when_the_store_refuses_to_delete() {
    struct RefusesDelete(Arc<MemoryBackend>);
    impl CredentialBackend for RefusesDelete {
        fn set(&self, user_id: &str, token: &str) -> std::result::Result<(), String> {
            self.0.set(user_id, token)
        }
        fn get(&self, user_id: &str) -> std::result::Result<Option<String>, String> {
            self.0.get(user_id)
        }
        fn delete(&self, _user_id: &str) -> std::result::Result<(), String> {
            Err("keychain prompt was not answered".into())
        }
        fn supports_named_entries(&self) -> bool {
            true
        }
    }
    let dir = temp_dir();
    let store = OfficialStore::with_backend(&dir, Arc::new(RefusesDelete(MemoryBackend::named())));
    store.save(&tokens(), 1).unwrap();
    assert!(store.meta().unwrap().is_some());

    // Use clear, not disconnect: disconnect bumps the global generation and would conflict with the one test above that bumps it.
    assert!(store.clear().unwrap().is_some());
    assert!(store.meta().unwrap().is_none());
    assert!(store.load().unwrap().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// Users saw "pending verification after about 1 hour": another process (desktop / CLI / MCP) just refreshed with the same
/// refresh token. The cloud did not rotate the refresh token, only the access token. The late refresh here is refused,
/// and it must not mark the other process's just-renewed account as "needs reauthorization".
#[tokio::test]
async fn a_rejected_refresh_after_another_process_refreshed_keeps_the_account() {
    let dir = temp_dir();
    let backend = MemoryBackend::named();
    let store = OfficialStore::with_backend(&dir, backend.clone());
    let used = tokens();
    store.save(&used, 1).unwrap();
    let newer = OfficialTokens {
        access_token: "access-from-the-other-process".into(),
        ..used.clone()
    };
    let other_dir = dir.clone();
    let other_newer = newer.clone();
    let (base, _) = serve(move |_| {
        // The other process wrote back a new token during this request's round trip (same refresh token).
        OfficialStore::with_backend(&other_dir, backend.clone())
            .save(&other_newer, 2)
            .unwrap();
        Reply::json(401, r#"{"error":"invalid_grant"}"#)
    })
    .await;
    let client = OfficialClient::with_bases(&base, &base).unwrap();
    let got = refresh_tokens(&store, &client, &used, 10_000)
        .await
        .unwrap();
    assert_eq!(got, newer);
    let meta = store.meta().unwrap().unwrap();
    assert!(!meta.needs_reauth);
    assert_eq!(store.load().unwrap(), Some(newer));
    let _ = std::fs::remove_dir_all(dir);
}

/// Nobody refreshed and the refresh is refused: this is a real reauthorization case.
#[tokio::test]
async fn a_rejected_refresh_nobody_else_answered_needs_reauth() {
    let dir = temp_dir();
    let store = OfficialStore::with_backend(&dir, MemoryBackend::named());
    let used = tokens();
    store.save(&used, 1).unwrap();
    let (base, _) = serve(|_| Reply::json(401, r#"{"error":"invalid_grant"}"#)).await;
    let client = OfficialClient::with_bases(&base, &base).unwrap();
    let error = refresh_tokens(&store, &client, &used, 10_000)
        .await
        .unwrap_err();
    assert!(error.needs_reauth());
    assert!(store.meta().unwrap().unwrap().needs_reauth);
    assert_eq!(store.load().unwrap(), None);
    let _ = std::fs::remove_dir_all(dir);
}
