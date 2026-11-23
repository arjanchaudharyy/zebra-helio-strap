//! The HTTP client for the relay and the official API. It connects only to fixed hosts (relay main and backup domains, official API) and does not follow redirects.

use super::{OfficialTokens, API_BASE, RELAY_BASE, RELAY_FALLBACK_BASE};
use crate::models::{error::Result, ZeppBridgeError};
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

/// The result of a token claim.
#[derive(Debug)]
pub enum ClaimOutcome {
    /// The user has not finished in the browser yet.
    Pending,
    Ready(OfficialTokens),
    /// The user did not consent.
    Denied,
    /// Zepp did not accept this authorization (token exchange failed).
    Failed,
    /// Expired, already used, or unknown to the relay.
    Expired,
}

/// The two official profile fields we use: the user ID (to check identity) and the nickname (so the user knows which account).
#[derive(Debug, Clone)]
pub struct OfficialProfile {
    pub user_id: String,
    pub nickname: Option<String>,
}

#[derive(Debug)]
pub enum RefreshOutcome {
    Ready(OfficialTokens),
    /// The token is definitely invalid: the user must authorize again.
    Revoked,
}

#[derive(Deserialize)]
pub(super) struct TokenBody {
    status: Option<String>,
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    user_id: Option<String>,
}

impl TokenBody {
    /// Converts the relay's token set to the stored shape. When `user_id` is missing, it uses `fallback_user`
    /// (a refresh response does not always include it again).
    pub(super) fn into_tokens(
        self,
        now: i64,
        fallback_user: Option<&str>,
    ) -> Option<OfficialTokens> {
        let access_token = self.access_token.filter(|value| !value.is_empty())?;
        let user_id = self
            .user_id
            .filter(|value| !value.is_empty())
            .or_else(|| fallback_user.map(str::to_string))?;
        Some(OfficialTokens {
            access_token,
            refresh_token: self.refresh_token.filter(|value| !value.is_empty()),
            expires_at: self.expires_in.map(|seconds| now + seconds.max(0)),
            user_id,
        })
    }
}

/// The size limit for one official API response, same as the legacy channel. 7 days of per-minute heart rate
/// and per-second details of a long workout are far below it.
const MAX_RESPONSE_BODY_BYTES: usize = 32 * 1024 * 1024;

async fn bounded_json(response: reqwest::Response) -> Result<serde_json::Value> {
    crate::connectors::read_json_limited(response, MAX_RESPONSE_BODY_BYTES).await
}

pub struct OfficialClient {
    http: reqwest::Client,
    relay_base: String,
    /// The URL to try when the main relay is unreachable (the request cannot be sent). Empty in tests.
    relay_fallback: Option<String>,
    api_base: String,
}

impl OfficialClient {
    pub fn new() -> Result<Self> {
        let mut client = Self::with_bases(RELAY_BASE, API_BASE)?;
        client.relay_fallback = Some(RELAY_FALLBACK_BASE.to_string());
        Ok(client)
    }

    /// For tests: points at a local fake server. Production code uses only the two fixed hosts of [`Self::new`].
    pub fn with_bases(relay_base: &str, api_base: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("ZeppBridge/", env!("CARGO_PKG_VERSION")));
        // Unit tests talk to loopback relays. A Windows system proxy must not
        // intercept those requests (it can return 502 before the relay sees them).
        #[cfg(test)]
        let http = http.no_proxy();
        let http = http.build()?;
        Ok(Self {
            http,
            relay_base: relay_base.trim_end_matches('/').to_string(),
            relay_fallback: None,
            api_base: api_base.trim_end_matches('/').to_string(),
        })
    }

    /// Sends one POST to the relay. If the request is never sent (DNS, connect, timeout), it tries the backup URL.
    /// Any HTTP response stops the retry: that is the relay's answer, not a network problem.
    async fn post_relay(&self, path: &str, body: &serde_json::Value) -> Result<reqwest::Response> {
        let primary = self
            .http
            .post(format!("{}{path}", self.relay_base))
            .json(body)
            .send()
            .await;
        match (primary, self.relay_fallback.as_deref()) {
            (Ok(response), _) => Ok(response),
            (Err(_), Some(fallback)) => Ok(self
                .http
                .post(format!("{fallback}{path}"))
                .json(body)
                .send()
                .await?),
            (Err(error), None) => Err(error.into()),
        }
    }

    pub async fn claim(&self, state: &str, claim_secret: &str, now: i64) -> Result<ClaimOutcome> {
        let response = self
            .post_relay(
                "/api/zepp/oauth/claim",
                &json!({ "state": state, "claim_secret": claim_secret }),
            )
            .await?;
        let status = response.status().as_u16();
        if status == 404 {
            return Ok(ClaimOutcome::Expired);
        }
        if status == 202 {
            return Ok(ClaimOutcome::Pending);
        }
        if status == 503 {
            return Err(ZeppBridgeError::Unavailable("The official authorization service is not enabled yet".into()));
        }
        if !(200..300).contains(&status) {
            return Err(ZeppBridgeError::HttpStatus {
                status,
                message: "The authorization relay did not accept this claim".into(),
            });
        }
        let body: TokenBody = serde_json::from_value(bounded_json(response).await?)?;
        Ok(match body.status.as_deref() {
            Some("ready") => match body.into_tokens(now, None) {
                Some(tokens) => ClaimOutcome::Ready(tokens),
                None => ClaimOutcome::Failed,
            },
            Some("denied") => ClaimOutcome::Denied,
            Some("pending") => ClaimOutcome::Pending,
            _ => ClaimOutcome::Failed,
        })
    }

    pub async fn refresh(&self, tokens: &OfficialTokens, now: i64) -> Result<RefreshOutcome> {
        let Some(refresh_token) = tokens.refresh_token.as_deref() else {
            return Ok(RefreshOutcome::Revoked);
        };
        let response = self
            .post_relay(
                "/api/zepp/oauth/refresh",
                &json!({ "access_token": tokens.access_token, "refresh_token": refresh_token }),
            )
            .await?;
        let status = response.status().as_u16();
        if status == 401 {
            return Ok(RefreshOutcome::Revoked);
        }
        if !(200..300).contains(&status) {
            return Err(ZeppBridgeError::HttpStatus {
                status,
                message: "Official token refresh failed for now".into(),
            });
        }
        let body: TokenBody = serde_json::from_value(bounded_json(response).await?)?;
        match body.into_tokens(now, Some(&tokens.user_id)) {
            Some(mut fresh) => {
                // When the refresh response has no new refresh_token, the old one stays valid.
                if fresh.refresh_token.is_none() {
                    fresh.refresh_token = tokens.refresh_token.clone();
                }
                Ok(RefreshOutcome::Ready(fresh))
            }
            None => Err(ZeppBridgeError::ParseError("The refresh response has no access token".into())),
        }
    }

    /// `GET /users/-/profile`: checks that this token works and which user it belongs to.
    pub async fn profile(&self, access_token: &str) -> Result<OfficialProfile> {
        let response = self
            .http
            .get(format!("{}/users/-/profile", self.api_base))
            .bearer_auth(access_token)
            .send()
            .await?;
        let status = response.status().as_u16();
        if status == 401 || status == 403 {
            return Err(ZeppBridgeError::NeedsReauth("The official token was refused".into()));
        }
        if !(200..300).contains(&status) {
            return Err(ZeppBridgeError::HttpStatus {
                status,
                message: "The official profile API did not respond".into(),
            });
        }
        let body = bounded_json(response).await?;
        let user_id = match body.get("userId") {
            Some(serde_json::Value::String(id)) if !id.is_empty() => id.clone(),
            Some(serde_json::Value::Number(id)) => id.to_string(),
            _ => return Err(ZeppBridgeError::ParseError("The official profile has no user ID".into())),
        };
        let nickname = body
            .get("nickName")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        Ok(OfficialProfile { user_id, nickname })
    }

    /// One GET on the official data API. `path` starts with `/`; query parameters are appended as is.
    ///
    /// 401 / 403 means the token is not accepted: the caller refreshes or asks the user to authorize again.
    /// 400 + `code = -50000` means this app does not have this data enabled (stress, SpO2 and others need a special
    /// business request). That counts as "unavailable", not a failure.
    pub async fn get_json(
        &self,
        access_token: &str,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<serde_json::Value> {
        let response = self
            .http
            .get(format!("{}{path}", self.api_base))
            .query(query)
            .bearer_auth(access_token)
            .send()
            .await?;
        let status = response.status().as_u16();
        if status == 401 || status == 403 {
            return Err(ZeppBridgeError::NeedsReauth("The official token was refused".into()));
        }
        if status == 400 || status == 404 {
            let body = bounded_json(response).await.unwrap_or_default();
            let code = body.get("code").and_then(serde_json::Value::as_i64);
            if status == 404 || code == Some(-50000) {
                return Err(ZeppBridgeError::Unavailable(format!(
                    "Official API {path} is not enabled for this app"
                )));
            }
            return Err(ZeppBridgeError::HttpStatus {
                status,
                message: format!("Official API {path} refused this request"),
            });
        }
        if !(200..300).contains(&status) {
            return Err(ZeppBridgeError::HttpStatus {
                status,
                message: format!("Official API {path} is not responding for now"),
            });
        }
        bounded_json(response).await
    }

    /// One POST on the official write API. It returns only the status code and payload; the caller decides what success is.
    /// The training plan API answers success for any payload, so this code cannot decide for it.
    ///
    /// A network failure (offline, timeout) returns as an error. The request can still have reached the server,
    /// so the caller must treat it as "unknown", not as "not sent".
    pub async fn post_json(
        &self,
        access_token: &str,
        path: &str,
        query: &[(&str, String)],
        body: &serde_json::Value,
    ) -> Result<(u16, serde_json::Value)> {
        let response = self
            .http
            .post(format!("{}{path}", self.api_base))
            .query(query)
            .bearer_auth(access_token)
            .json(body)
            .send()
            .await?;
        let status = response.status().as_u16();
        let body = bounded_json(response).await.unwrap_or_default();
        Ok((status, body))
    }

    /// Revokes the authorization at Zepp. Called on disconnect; a failure does not stop local token removal.
    pub async fn revoke(&self, access_token: &str) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/users/-/thirdparty/unauthorize", self.api_base))
            .bearer_auth(access_token)
            .json(&json!({}))
            .send()
            .await?;
        let status = response.status().as_u16();
        // An already invalid token also counts as revoked.
        if (200..300).contains(&status) || status == 401 || status == 403 {
            Ok(())
        } else {
            Err(ZeppBridgeError::HttpStatus {
                status,
                message: "Zepp did not confirm the revocation".into(),
            })
        }
    }
}
