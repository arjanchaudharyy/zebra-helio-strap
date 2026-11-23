//! Send the full-window payload that the ledger prepared, and turn the result into one of the three outcomes the ledger records.
//!
//! Call order (the command layer takes the write lock):
//!
//! 1. Inside the write lock: `Database::prepare_plan_publish` -> `Prepared::Send { publish_id, body }`;
//! 2. **Release the write lock**, then [`send_window`] goes online;
//! 3. Inside the write lock: `Database::finish_plan_publish(publish_id, &outcome)`.
//!
//! Do not hold the write lock while online. Sync follows the same rule.

use crate::models::{error::Result, ZeppBridgeError};
use crate::official::{fresh_tokens, refresh_tokens, OfficialClient, OfficialStore};
use crate::storage::training_plan::SendOutcome;
use chrono::Utc;
use serde_json::Value;

pub const WORKOUTS_PATH: &str = "/users/-/workouts";
pub const WORKOUTS_VERSION: &str = "2.0.0";

/// Before the send, make sure a usable official token exists. If not, do not touch the ledger: that leaves a
/// `pending` row that can never be sent.
pub async fn ensure_connected(store: &OfficialStore, client: &OfficialClient) -> Result<()> {
    match fresh_tokens(store, client, Utc::now().timestamp()).await? {
        Some(_) => Ok(()),
        None => Err(ZeppBridgeError::NeedsReauth(
            "Zepp official authorization is not connected yet".into(),
        )),
    }
}

/// Send once. If the token is refused, refresh it and send again once. A refused refresh counts as "clearly not applied".
///
/// This returns no error: every outcome goes into the ledger.
pub async fn send_window(
    store: &OfficialStore,
    client: &OfficialClient,
    body: &Value,
) -> SendOutcome {
    let now = Utc::now().timestamp();
    let tokens = match fresh_tokens(store, client, now).await {
        Ok(Some(tokens)) => tokens,
        Ok(None) => return not_authorized(),
        Err(error) if error.needs_reauth() => return not_authorized(),
        Err(error) => {
            // The refresh failed and no old token is usable: the request was never sent.
            return SendOutcome::Rejected {
                http_status: 0,
                error_code: error.code().to_string(),
            };
        }
    };
    let query = [("version", WORKOUTS_VERSION.to_string())];
    let mut attempt = client
        .post_json(&tokens.access_token, WORKOUTS_PATH, &query, body)
        .await;
    if matches!(attempt, Ok((401 | 403, _))) {
        match refresh_tokens(store, client, &tokens, Utc::now().timestamp()).await {
            Ok(fresh) => {
                attempt = client
                    .post_json(&fresh.access_token, WORKOUTS_PATH, &query, body)
                    .await;
            }
            Err(_) => return not_authorized(),
        }
    }
    classify(attempt)
}

fn not_authorized() -> SendOutcome {
    SendOutcome::Rejected {
        http_status: 401,
        error_code: "err.core.needs_reauth".into(),
    }
}

/// Status code -> outcome.
///
/// - 2xx and the payload has no negative `code`: delivered (the official API returns success for any payload, so this only means delivered);
/// - 2xx with `code < 0`, or 4xx: clearly refused, and the ledger rolls back;
/// - 5xx or a network error: unknown if it was applied.
pub fn classify(attempt: Result<(u16, Value)>) -> SendOutcome {
    match attempt {
        Ok((status, body)) => {
            let code = body.get("code").and_then(Value::as_i64);
            match status {
                200..=299 if code.is_none_or(|code| code >= 0) => SendOutcome::Delivered,
                200..=299 | 400..=499 => SendOutcome::Rejected {
                    http_status: status,
                    error_code: if matches!(status, 401 | 403) {
                        "err.core.needs_reauth".into()
                    } else {
                        "err.training_plan.rejected".into()
                    },
                },
                _ => SendOutcome::Unknown {
                    error_code: "err.core.http_status".into(),
                },
            }
        }
        Err(error) => SendOutcome::Unknown {
            error_code: error.code().to_string(),
        },
    }
}
