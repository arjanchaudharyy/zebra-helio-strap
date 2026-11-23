//! Request policy: regional host and path checks, retry and backoff, status and business code classification, response size limit (split out of connectors/zepp.rs, same logic).

use super::*;

/// Validate and canonicalize a Zepp regional host.
///
/// The connector only ever talks to the HTTPS origin.  A bare hostname is
/// accepted as a convenience and canonicalized to HTTPS; an explicit scheme
/// must be HTTPS.  Credentials, ports, paths, queries, fragments, subdomains
/// and look-alike domains are rejected before a token can be attached.
pub fn validate_region_host(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ZeppBridgeError::InvalidHost("Host is empty".into()));
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_owned()
    } else {
        format!("https://{trimmed}")
    };

    // `Url::port()` intentionally normalizes an explicit default port away
    // (`:443` becomes `None`), but an explicit port is still outside this
    // connector's allow-list. Inspect the authority before parsing so the
    // rejected form cannot be mistaken for the origin-only form.
    if let Some((_, authority_and_rest)) = candidate.split_once("://") {
        let authority = authority_and_rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default();
        let host_part = match authority.rsplit_once('@') {
            Some((_, host)) => host,
            None => authority,
        };
        if host_part
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse::<u16>().ok())
            .is_some()
        {
            return Err(ZeppBridgeError::InvalidHost("Port is not allowed".into()));
        }
    }

    let url = Url::parse(&candidate)
        .map_err(|_| ZeppBridgeError::InvalidHost("Host URL cannot be parsed".into()))?;
    if url.scheme() != "https" {
        return Err(ZeppBridgeError::InvalidHost("Only HTTPS is allowed".into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ZeppBridgeError::InvalidHost("Credentials are not allowed".into()));
    }
    if url.port().is_some() {
        return Err(ZeppBridgeError::InvalidHost("Port is not allowed".into()));
    }
    if url.path() != "/" && !url.path().is_empty() {
        return Err(ZeppBridgeError::InvalidHost("Path is not allowed".into()));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(ZeppBridgeError::InvalidHost(
            "Query or fragment is not allowed".into(),
        ));
    }

    let host = url
        .host_str()
        .ok_or_else(|| ZeppBridgeError::InvalidHost("Host name is missing".into()))?
        .to_ascii_lowercase();
    let valid_zepp = host.starts_with("api-mifit") && host.ends_with(".zepp.com");
    let valid_huami = host.starts_with("api-mifit") && host.ends_with(".huami.com");
    if !(valid_zepp || valid_huami) {
        return Err(ZeppBridgeError::InvalidHost(
            "Only api-mifit*.zepp.com or api-mifit*.huami.com is allowed".into(),
        ));
    }

    Ok(format!("https://{host}"))
}

/// The server's `Retry-After`, converted to milliseconds.
///
/// Both valid forms are accepted: seconds (`Retry-After: 30`) and an HTTP date
/// (`Retry-After: Wed, 21 Oct 2026 07:28:00 GMT`). The value is clamped to `MAX_BACKOFF_MS`.
/// A response that says one hour must not hang the sync. The user closes the window long before that.
///
/// This is a pure function so it can be tested directly. A header map is much easier to build than a 429 response.
pub(super) fn retry_after_ms(headers: &header::HeaderMap) -> Option<u64> {
    let raw = headers.get(header::RETRY_AFTER)?.to_str().ok()?.trim();
    if raw.is_empty() {
        return None;
    }
    // Parse as seconds first. This is the most common form.
    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(seconds.saturating_mul(1_000).min(MAX_BACKOFF_MS));
    }
    // Then parse as an HTTP date. A time in the past means "retry now", so wait 0 ms.
    let target = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
    let delta = target.with_timezone(&chrono::Utc) - chrono::Utc::now();
    let millis = delta.num_milliseconds().max(0) as u64;
    Some(millis.min(MAX_BACKOFF_MS))
}

/// The timeout for one request, and whether to retry after a timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RequestTiming {
    pub(super) timeout: Duration,
    pub(super) retry_on_timeout: bool,
}

pub(super) const DEFAULT_TIMING: RequestTiming = RequestTiming {
    timeout: Duration::from_secs(30),
    retry_on_timeout: true,
};

/// #157: the weight endpoint is slow. Even `limit=15` takes more than 20 seconds, so with a 30 second
/// timeout it never finished. Give it two minutes, and do not retry on timeout. Sending a request that
/// did not return in two minutes two more times only adds four minutes to this sync.
pub(super) const WEIGHT_TIMING: RequestTiming = RequestTiming {
    timeout: Duration::from_secs(120),
    retry_on_timeout: false,
};

impl RequestTiming {
    /// The outer fallback timeout. It must be longer than the reqwest per-request timeout. If the outer one fires first,
    /// it cuts off a request that could still return (the outer value was once fixed at 35 seconds, so a longer per-request timeout did nothing).
    pub(super) fn guard(self) -> Duration {
        self.timeout + Duration::from_secs(5)
    }

    /// Whether one more try is allowed after try number `attempt` (already incremented) fails.
    pub(super) fn may_retry(self, attempt: usize, timed_out: bool) -> bool {
        attempt < MAX_ATTEMPTS && (self.retry_on_timeout || !timed_out)
    }
}

/// Exponential backoff with jitter.
///
/// Used when the server gives no `Retry-After`. Jitter stops streams that started at the same time
/// from returning in the same millisecond and colliding again. Sync runs several streams at once, so a
/// backoff without jitter turns one rate limit into three.
pub(super) fn exponential_backoff_ms(attempt: usize, base_ms: u64) -> u64 {
    // When the base is 0 (after the retry table is used up), use a minimum. Otherwise "backoff" does not wait.
    let base = base_ms.max(200);
    let scaled = base.saturating_mul(1u64 << attempt.min(5));
    let capped = scaled.min(MAX_BACKOFF_MS);
    // Jitter is in [75%, 100%]. Use getrandom, because this crate already has it.
    // Another rand crate is not worth it for one backoff. If no random value is available, use no jitter.
    // The backoff itself is what matters. Jitter is only a bonus.
    let mut byte = [0u8; 1];
    if getrandom::getrandom(&mut byte).is_err() {
        return capped;
    }
    let jitter = 75 + (u64::from(byte[0]) % 26); // 75..=100
    capped * jitter / 100
}

/// Classification is kept pure so callers/tests can verify the security
/// boundary without constructing a live HTTP response.
pub fn classify_status(status: u16) -> Option<ZeppBridgeError> {
    match status {
        401 | 403 => Some(ZeppBridgeError::NeedsReauth(format!("HTTP {status}"))),
        404 => Some(ZeppBridgeError::Unavailable(format!("HTTP {status}"))),
        429 | 500..=599 => Some(ZeppBridgeError::RetryExhausted {
            status,
            message: "Service temporarily unavailable".into(),
        }),
        200..=299 => None,
        _ => Some(ZeppBridgeError::HttpStatus {
            status,
            message: "Service returned a non-success status".into(),
        }),
    }
}

/// The payloads of these three streams are wrapped in `{ code, message, data }`. `code = 1` is success.
///
/// # Evidence
///
/// This was counted, not copied from docs: a real local database had 3466 stored payloads, and
/// **1075 of them had this wrapper**: `workouts` 484, `workout_detail` 334,
/// `sleep` 257. **Every one had `code` 1 and `message` `"success"`**.
/// The other four streams (`daily_summary` / `wellness` / `hrv` / `heart_rate`) have data at the top level
/// with no wrapper, so this function does nothing for them.
///
/// # Why this check is required
///
/// Before this, **no code at all** checked this `code`. Only HTTP 401/403
/// counted as "sign in again" (see `classify_status`). So a credential that the business layer
/// had already rejected got HTTP 200 and a payload with no `data`. The normalizer found nothing and threw
/// "data could not be parsed". The user was never told to sign in again, and never got data.
///
/// # Why not mark it as "needs re-auth" directly
///
/// Because **no failure code was ever observed**: all 1075 were successes. Sending the user to scan the QR
/// code again because of a number never seen trades a sure bad experience for a guess. This does only one
/// thing: it turns the payload into an error with the code and the cloud's own text, so it shows in the UI and the diagnostics report.
/// The next report with a specific code lets us map it exactly to `NeedsReauth`.
pub(super) const CLOUD_SUCCESS_CODE: i64 = 1;

pub(super) fn classify_business_code(payload: &Value) -> Option<ZeppBridgeError> {
    let code = payload.as_object()?.get("code")?.as_i64()?;
    if code == CLOUD_SUCCESS_CODE {
        return None;
    }
    let message = payload
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("(the cloud gave no explanation)")
        .to_string();
    Some(ZeppBridgeError::CloudRejected { code, message })
}

/// The maximum size of one JSON response. Without a limit, a very large payload could
/// fill memory in `.json()`. With `Content-Length`, reject early. Without it, count chunks.
pub(super) const MAX_RESPONSE_BODY_BYTES: usize = 32 * 1024 * 1024;

pub(super) fn validate_api_path(path: &str) -> Result<()> {
    // `Url::join` treats a path that starts with `//` as a scheme-relative
    // URL and replaces the host. Reject that, and any query/fragment, before
    // joining; the origin check after join is the second fence.
    if !path.starts_with('/')
        || path.starts_with("//")
        || path.contains('?')
        || path.contains('#')
        || path.contains('\\')
        || path.contains('\0')
    {
        return Err(ZeppBridgeError::ConfigError("Invalid API path".into()));
    }
    Ok(())
}

pub(super) fn same_region_origin(next: &Url, base: &Url) -> bool {
    next.scheme() == "https"
        && base.scheme() == "https"
        && next.host_str().is_some()
        && next.host_str() == base.host_str()
        && next.port_or_known_default() == base.port_or_known_default()
}

pub(super) fn validate_member_id(input: &str) -> Result<&str> {
    let trimmed = input.trim();
    if trimmed == "-1" {
        return Ok(trimmed);
    }
    if trimmed.is_empty() || trimmed.len() > 32 || !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Err(ZeppBridgeError::ConfigError("Invalid member_id".into()));
    }
    Ok(trimmed)
}

pub(super) async fn read_json_body(response: reqwest::Response) -> Result<Value> {
    crate::connectors::read_json_limited(response, MAX_RESPONSE_BODY_BYTES).await
}

pub fn validate_track_id(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > 32 || !trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Err(ZeppBridgeError::ConfigError("Invalid trackid".into()));
    }
    Ok(trimmed.to_owned())
}

pub fn validate_detail_source(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty()
        || trimmed.len() > 64
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(ZeppBridgeError::ConfigError("Invalid source".into()));
    }
    Ok(trimmed.to_owned())
}
