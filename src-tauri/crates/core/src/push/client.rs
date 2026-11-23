//! HTTP sending and URL checks for push.
//!
//! Hard rules: send only to the URL the user entered; do not follow redirects; never put URL credentials or keys in logs or errors.

use std::net::IpAddr;
use std::time::Duration;

/// Network errors and 5xx get a few retries (direct connections are sometimes reset in the TLS handshake); 4xx is a real error, no retry.
const MAX_ATTEMPTS: u32 = 5;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// The result of a push target URL check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteUrl {
    /// The site root without a trailing slash, for example `https://health.example.com`.
    pub base: String,
    /// `base + "/v1"`.
    pub api: String,
    pub host: String,
}

/// General rule: https for any host; http only for loopback, private networks (10/8, 172.16/12, 192.168/16) and `.local`.
pub fn http_allowed(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => v4.is_loopback() || v4.is_private(),
            IpAddr::V6(v6) => v6.is_loopback(),
        };
    }
    let lower = bare.to_ascii_lowercase();
    lower == "localhost" || lower.ends_with(".local")
}

/// The PA site URL: a copy of `ZeppBridge-Cloud/uploader/lib/site.mjs::normalizeSite`.
///
/// - `example.com` -> `https://health.example.com`; no extra prefix when it already has `health.`;
/// - A full URL is used as is, without the trailing slash and the old `/v1`;
/// - Only https; reject user/password, query, fragment and bare IPs;
/// - With `dev_loopback`, allow `http://127.0.0.1:<port>` (only for tests and local self-tests).
pub fn normalize_pa_site(input: &str, dev_loopback: bool) -> Result<SiteUrl, &'static str> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("site_empty");
    }
    if raw.chars().any(char::is_whitespace) || raw.contains('?') || raw.contains('#') {
        return Err("site_invalid");
    }
    let (scheme, rest) = match raw.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest.to_string()),
        None => {
            if raw.contains(['/', '@', '\\']) {
                return Err("site_invalid");
            }
            let mut host = raw.to_ascii_lowercase();
            if host.ends_with('.') {
                host.pop();
            }
            let hostname = host.split(':').next().unwrap_or_default().to_string();
            if is_ip_host(&hostname) || !hostname.contains('.') {
                return Err("site_invalid");
            }
            if !hostname.starts_with("health.") {
                host = format!("health.{host}");
            }
            ("https".to_string(), host)
        }
    };
    let (authority, path) = match rest.find('/') {
        Some(index) => (rest[..index].to_string(), rest[index..].to_string()),
        None => (rest, String::new()),
    };
    if authority.contains('@') || authority.is_empty() {
        return Err("site_invalid");
    }
    let authority = authority.to_ascii_lowercase();
    let hostname = authority
        .rsplit_once(':')
        .filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty())
        .map_or(authority.as_str(), |(host, _)| host)
        .to_string();
    let loopback = scheme == "http" && hostname == "127.0.0.1";
    if loopback {
        if !dev_loopback {
            return Err("site_http_not_allowed");
        }
    } else {
        if scheme != "https" {
            return Err("site_http_not_allowed");
        }
        if is_ip_host(&hostname) || !hostname.contains('.') {
            return Err("site_invalid");
        }
    }
    let mut path = path.trim_end_matches('/').to_string();
    if let Some(stripped) = path.strip_suffix("/v1") {
        path = stripped.to_string();
    }
    let base = format!("{scheme}://{authority}{path}");
    Ok(SiteUrl {
        api: format!("{base}/v1"),
        base,
        host: authority,
    })
}

fn is_ip_host(hostname: &str) -> bool {
    hostname.starts_with('[') || hostname.parse::<std::net::Ipv4Addr>().is_ok()
}

/// Replaces keys like `zbr_...` / `zbw_...` in error text (same method as upload.mjs).
pub fn redact(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(index) = rest.find("zb") {
        let (head, tail) = rest.split_at(index);
        out.push_str(head);
        let bytes = tail.as_bytes();
        if bytes.len() > 3 && (bytes[2] == b'r' || bytes[2] == b'w') && bytes[3] == b'_' {
            let end = tail[4..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .map_or(tail.len(), |offset| offset + 4);
            out.push_str("zb*_<redacted>");
            rest = &tail[end..];
        } else {
            out.push_str("zb");
            rest = &tail[2..];
        }
    }
    out.push_str(rest);
    out
}

/// A client that does not follow redirects and does not use the system proxy.
pub fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("http client: {error}"))
}

/// Sends one request, with backoff retries on network errors and 5xx. `label` holds only the method and relative path, no host or key.
pub async fn send(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    bearer: &str,
    content_type: &str,
    body: Option<Vec<u8>>,
    label: &str,
) -> Result<(), String> {
    for attempt in 1..=MAX_ATTEMPTS {
        let mut request = client
            .request(method.clone(), url)
            .header("authorization", format!("Bearer {bearer}"))
            .header("content-type", content_type);
        if let Some(body) = body.as_ref() {
            request = request.body(body.clone());
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => return Ok(()),
            Ok(response) => {
                let status = response.status().as_u16();
                // 3xx is also a failure: redirects are not followed, so the key cannot go elsewhere.
                if status < 500 || attempt >= MAX_ATTEMPTS {
                    return Err(format!("{label} -> HTTP {status}"));
                }
            }
            Err(error) => {
                if attempt >= MAX_ATTEMPTS {
                    let kind = if error.is_timeout() {
                        "timeout"
                    } else if error.is_connect() {
                        "connect"
                    } else {
                        "network"
                    };
                    return Err(format!("{label} -> network error ({kind})"));
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(2000 * u64::from(attempt))).await;
    }
    Err(format!("{label} -> gave up"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_normalization_matches_site_mjs() {
        let ok = |input: &str| normalize_pa_site(input, false).unwrap().base;
        assert_eq!(ok("example.com"), "https://health.example.com");
        assert_eq!(ok("Health.Example.com."), "https://health.example.com");
        assert_eq!(ok("https://data.example.com/"), "https://data.example.com");
        assert_eq!(ok("https://health.x.com/v1"), "https://health.x.com");
        assert_eq!(
            normalize_pa_site("example.com", false).unwrap().api,
            "https://health.example.com/v1"
        );
        for bad in [
            "",
            "example",
            "1.2.3.4",
            "http://health.example.com",
            "https://user:pw@health.example.com",
            "https://health.example.com/?a=1",
            "example.com/path",
            "https://10.0.0.1",
            "http://127.0.0.1:8787",
        ] {
            assert!(normalize_pa_site(bad, false).is_err(), "{bad}");
        }
        assert_eq!(
            normalize_pa_site("http://127.0.0.1:8787", true)
                .unwrap()
                .api,
            "http://127.0.0.1:8787/v1"
        );
    }

    #[test]
    fn plain_http_only_reaches_private_hosts() {
        for host in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.9",
            "192.168.1.2",
            "nas.local",
            "[::1]",
            "localhost",
        ] {
            assert!(http_allowed(host), "{host}");
        }
        for host in ["8.8.8.8", "172.32.0.1", "example.com", "health.example.com"] {
            assert!(!http_allowed(host), "{host}");
        }
    }

    #[test]
    fn keys_are_redacted_from_messages() {
        let key = format!("zbw_abcdefghij_{}", "A".repeat(43));
        assert_eq!(
            redact(&format!("PUT x failed with {key}.")),
            "PUT x failed with zb*_<redacted>."
        );
        assert_eq!(redact("zbx_ and zb"), "zbx_ and zb");
    }
}
