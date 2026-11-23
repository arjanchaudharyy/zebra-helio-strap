//! The official Zepp open platform: authorization, tokens, token storage.
//!
//! It is fully separate from the "legacy channel" in `auth` / `connectors`. The legacy channel uses the
//! private `api-mifit*` endpoints after an account login. This module uses `api-open.zepp.com` tokens from official OAuth.
//! An official token never goes into the legacy channel, and the reverse. Each set is stored on its own.
//!
//! The App ID and App Secret live only on the relay (Cloudflare). The desktop app carries neither:
//! the relay builds the authorization URL, and adds the secret to token exchange and refresh.

mod client;
#[cfg(test)]
pub(crate) mod fake_server;
pub mod fetch;
mod store;

pub use client::{ClaimOutcome, OfficialClient, OfficialProfile, RefreshOutcome};
pub use store::{current_generation, invalidate_pending_logins, OfficialMeta, OfficialStore};

use crate::models::{error::Result, ZeppBridgeError};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The relay (on the production domain). zeppbridge.com and zeppbridge.pages.dev are one Cloudflare Pages project
/// with one D1 database. The Zepp console still registers the pages.dev callback (REDIRECT_URI in server/zepp/official.js).
/// Authorization starts here and the callback lands on pages.dev, with state in the same database. Old app versions keep using pages.dev.
pub const RELAY_BASE: &str = "https://zeppbridge.com";
/// The same relay, tried again when the main domain is unreachable. Some networks block the new domain but reach pages.dev
/// (or the reverse). Both addresses use the same D1, so a claim works from either.
pub const RELAY_FALLBACK_BASE: &str = "https://zeppbridge.pages.dev";
/// The official data API.
pub const API_BASE: &str = "https://api-open.zepp.com";

/// Refresh this many seconds before expiry, so a sync does not hit an expired token.
pub const REFRESH_MARGIN_SECONDS: i64 = 5 * 60;

/// The maximum wait for one authorization, from browser open to token claim (same as the relay's hold time).
pub const AUTHORIZATION_WINDOW_SECONDS: u64 = 10 * 60;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfficialTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// The token expiry in Unix seconds. Empty when Zepp gives no `expires_in`; then only a 401 tells us.
    pub expires_at: Option<i64>,
    pub user_id: String,
}

// Tokens never go into logs: Debug only says whether one is present.
impl std::fmt::Debug for OfficialTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OfficialTokens")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at", &self.expires_at)
            .field("user_id", &self.user_id)
            .finish()
    }
}

impl OfficialTokens {
    /// Tells whether the stored copy and the copy used for refresh come from the same authorization and refresh.
    ///
    /// Comparing only the refresh token is not enough: the cloud does not rotate the refresh token. After another process
    /// refreshes, both refresh tokens match but the access tokens differ. This side would refresh again with the same token,
    /// get refused, and mark the account as "needs reauthorization". A changed access token means someone refreshed.
    pub(crate) fn same_grant(&self, other: &OfficialTokens) -> bool {
        self.user_id == other.user_id
            && self.refresh_token == other.refresh_token
            && self.access_token == other.access_token
    }

    /// A refresh is due from [`REFRESH_MARGIN_SECONDS`] seconds before expiry.
    pub fn needs_refresh(&self, now: i64) -> bool {
        self.expires_at
            .is_some_and(|expires_at| now >= expires_at - REFRESH_MARGIN_SECONDS)
    }
}

impl OfficialMeta {
    /// The token is past its expiry. A token with no expiry time is not expired here; only a 401 tells us.
    pub fn is_expired(&self, now: i64) -> bool {
        self.expires_at.is_some_and(|expires_at| now >= expires_at)
    }
}

/// Only one refresh runs at a time. Status reads and syncs can both find a token near expiry.
/// Two refreshes would use the same refresh token, and the later one can be refused and mark the account as
/// "needs reauthorization". This lock queues refreshes in the process. Across processes (desktop and CLI / MCP), a
/// file lock in the data directory also applies, see [`OfficialStore::refresh_flight`].
static REFRESH_FLIGHT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Forces one refresh (the token is near expiry, or the data API refused it).
///
/// After it gets the refresh turn, it reads the store first. If someone else refreshed (the stored refresh token is not `used`),
/// it uses the stored token. If the account is disconnected, it needs reauthorization. The write-back is compare-and-swap, see
/// [`OfficialStore::replace_if_current`].
pub async fn refresh_tokens(
    store: &OfficialStore,
    client: &OfficialClient,
    used: &OfficialTokens,
    now: i64,
) -> Result<OfficialTokens> {
    let _flight = REFRESH_FLIGHT.lock().await;
    let mut other_processes = store.refresh_flight().await;
    let reauth = || ZeppBridgeError::NeedsReauth("Official authorization expired. Authorize again".into());
    match store.load()? {
        None => return Err(reauth()),
        Some(current) if !current.same_grant(used) => return Ok(current),
        Some(_) => {}
    }
    match client.refresh(used, now).await? {
        RefreshOutcome::Ready(fresh) => {
            if store.replace_if_current(used, &fresh, now)? {
                Ok(fresh)
            } else {
                store.load()?.ok_or_else(reauth)
            }
        }
        RefreshOutcome::Revoked => {
            // A refusal usually means another process (desktop / CLI / MCP) just refreshed with the same refresh token.
            // Users saw "pending verification after about 1 hour", which is when the access token first expires.
            // Read the store again. If the other process stored a new token, use it. Without the cross-process lock,
            // the other process can still be in flight, so wait for it to finish (or for the lock) and look again.
            if let Some(current) = store.load()? {
                if !current.same_grant(used) {
                    return Ok(current);
                }
            }
            if other_processes.is_none() {
                other_processes = store.refresh_flight().await;
                if let Some(current) = store.load()? {
                    if !current.same_grant(used) {
                        return Ok(current);
                    }
                }
            }
            drop(other_processes);
            if store.mark_needs_reauth_if_current(used)? {
                Err(reauth())
            } else {
                store.load()?.ok_or_else(reauth)
            }
        }
    }
}

/// Gets a usable token before a sync: refresh through the relay near expiry, and mark "needs reauthorization" on refusal.
///
/// - Never connected, or already marked as needing reauthorization: `Ok(None)`.
/// - A temporary refresh failure (offline, relay 5xx): use the old token until it expires, then return the error.
/// - Zepp explicitly refuses the refresh: clear the token, mark the metadata `needs_reauth`, return `NeedsReauth`.
pub async fn fresh_tokens(
    store: &OfficialStore,
    client: &OfficialClient,
    now: i64,
) -> Result<Option<OfficialTokens>> {
    let Some(tokens) = store.load()? else {
        return Ok(None);
    };
    if !tokens.needs_refresh(now) {
        return Ok(Some(tokens));
    }
    match refresh_tokens(store, client, &tokens, now).await {
        Ok(fresh) => Ok(Some(fresh)),
        Err(error @ ZeppBridgeError::NeedsReauth(_)) => Err(error),
        Err(error) if tokens.expires_at.is_some_and(|expires_at| now < expires_at) => {
            tracing::warn!("Official token refresh failed for now, using the old token: {error}");
            Ok(Some(tokens))
        }
        Err(error) => Err(error),
    }
}

/// The two random values of one authorization.
///
/// `state` appears in the browser address bar. `claim_secret` stays on this machine; the relay only sees its
/// SHA-256. So a person who sees only the address bar cannot claim the token.
pub struct AuthorizationRequest {
    pub state: String,
    claim_secret: String,
}

impl AuthorizationRequest {
    pub fn new() -> Result<Self> {
        Ok(Self {
            state: random_token()?,
            claim_secret: random_token()?,
        })
    }

    pub fn claim_secret(&self) -> &str {
        &self.claim_secret
    }

    /// The URL to open in the system browser. The relay records this authorization and sends the user to the Zepp page.
    pub fn start_url(&self) -> String {
        let claim_hash = hex::encode(Sha256::digest(self.claim_secret.as_bytes()));
        format!(
            "{RELAY_BASE}/api/zepp/oauth/start?state={}&claim={claim_hash}",
            self.state
        )
    }
}

/// 32 random bytes, base64url without padding (43 characters, which matches the relay's check).
fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| ZeppBridgeError::ConfigError(format!("Cannot generate random bytes: {error}")))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod cross_process_tests;
#[cfg(test)]
mod tests;
