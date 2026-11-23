//! Read login credentials from cookies / Web Storage and clean them.

use super::*;

pub(super) async fn collect_cookies(
    window: &WebviewWindow,
    page_url: &str,
) -> Vec<(String, String)> {
    // Start with values visible to the current page. They are the freshest
    // representation of the completed login and must win over cookie-store
    // entries with the same name.
    let mut pairs = Vec::new();
    if let Some(header) = document_cookie(window).await {
        append_missing_pairs(&mut pairs, parse_cookie_header(&header));
    }

    // Credentials are not always in cookies. The watch face site is a frontend app, and it is normal
    // for it to write login data to localStorage / sessionStorage. Then `document.cookie` and the
    // webview cookie jar cannot see it, and the user must open developer tools to copy the App Token
    // (people on Reddit did this). So we also look at storage and use keys with matching names as a
    // credential source.
    if let Some(entries) = web_storage_entries(window).await {
        append_missing_pairs(&mut pairs, entries);
    }

    // `cookies()` returns the runtime store for every URL. That allowed a
    // previous Xiaomi/Google/etc. account to supply the first matching
    // userid/apptoken pair. Restrict the fallback to cookies applicable to the
    // page that just completed the Zepp login.
    if let Ok(url) = reqwest::Url::parse(page_url) {
        let window_for_store = window.clone();
        let scoped = tokio::task::spawn_blocking(move || {
            window_for_store
                .cookies_for_url(url)
                .map(|cookies| {
                    cookies
                        .into_iter()
                        .map(|cookie| (cookie.name().to_string(), cookie.value().to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        append_missing_pairs(&mut pairs, scoped);
    }
    pairs
}

pub(super) fn append_missing_pairs(
    target: &mut Vec<(String, String)>,
    incoming: Vec<(String, String)>,
) {
    for pair in incoming {
        if !target
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(&pair.0))
        {
            target.push(pair);
        }
    }
}

/// Collect key-value pairs from localStorage / sessionStorage that can be credentials.
///
/// Take only the keys with names that look related. Do not read the whole storage back: it holds other
/// user data that we have no reason to touch. All values go through the same
/// `sanitize_user_id` / `sanitize_app_token` checks as cookies. Values with a bad format are ignored.
pub(super) async fn web_storage_entries(window: &WebviewWindow) -> Option<Vec<(String, String)>> {
    const SCRIPT: &str = r#"(function(){
  try {
    var wanted = ['hm-user-login-info','hm_user_login_info','userid','user_id','apptoken','app_token','app-token','token_info','loginInfo','domains','cname','region','country_code','wf_baseUrl'];
    var out = {};
    [window.localStorage, window.sessionStorage].forEach(function(store){
      if (!store) return;
      for (var i = 0; i < store.length; i++) {
        var key = store.key(i);
        if (!key) continue;
        var lowered = key.toLowerCase();
        if (wanted.some(function(name){ return lowered.indexOf(name) !== -1; })) {
          if (!(key in out)) out[key] = store.getItem(key) || '';
        }
      }
    });
    return JSON.stringify(out);
  } catch (e) { return '{}'; }
})()"#;

    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let sent = std::sync::Mutex::new(Some(tx));
    window
        .eval_with_callback(SCRIPT, move |raw| {
            if let Some(tx) = sent.lock().ok().and_then(|mut guard| guard.take()) {
                let _ = tx.send(decode_eval_string(&raw));
            }
        })
        .ok()?;
    let raw = tokio::time::timeout(COOKIE_EVAL_TIMEOUT, rx)
        .await
        .ok()
        .and_then(Result::ok)?;
    let parsed: serde_json::Map<String, Value> = serde_json::from_str(&raw).ok()?;
    let entries: Vec<(String, String)> = parsed
        .into_iter()
        .map(|(key, value)| match value {
            Value::String(text) => (key, text),
            other => (key, other.to_string()),
        })
        .collect();
    (!entries.is_empty()).then_some(entries)
}

pub(super) async fn document_cookie(window: &WebviewWindow) -> Option<String> {
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let sent = std::sync::Mutex::new(Some(tx));
    window
        .eval_with_callback(
            "(function(){try{return document.cookie||'';}catch(e){return '';}})()",
            move |raw| {
                if let Some(tx) = sent.lock().ok().and_then(|mut guard| guard.take()) {
                    let _ = tx.send(decode_eval_string(&raw));
                }
            },
        )
        .ok()?;
    tokio::time::timeout(COOKIE_EVAL_TIMEOUT, rx)
        .await
        .ok()
        .and_then(Result::ok)
        .filter(|value| !value.is_empty())
}

pub(super) fn decode_eval_string(raw: &str) -> String {
    serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.trim_matches('"').to_string())
}

/// Parse `document.cookie` / Cookie header text into name/value pairs.
pub(crate) fn parse_cookie_header(header: &str) -> Vec<(String, String)> {
    header
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.split_once('=')?;
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), value.trim().to_string()))
        })
        .collect()
}

/// Extract a user id and app token from fake or real cookie pairs.
pub(crate) fn parse_login_cookies(cookies: &[(String, String)]) -> Option<ExtractedLogin> {
    // The official Watchface frontend treats the separate userid/apptoken
    // cookies as authoritative over the bundled login-info cookie.
    if let (Some(user_id), Some(app_token)) = (
        cookie_value(cookies, &["userid", "user_id", "userId"])
            .and_then(|value| sanitize_user_id(&percent_decode(&value))),
        cookie_value(cookies, &["apptoken", "app_token", "app-token", "appToken"])
            .and_then(|value| sanitize_app_token(&percent_decode(&value))),
    ) {
        return Some(ExtractedLogin {
            user_id,
            app_token,
            region_hint: region_hint_from_pairs(cookies),
        });
    }

    if let Some(login_info) = cookie_value(cookies, &["hm-user-login-info", "hm_user_login_info"]) {
        if let Some(extracted) = extract_from_login_info(&login_info) {
            return Some(extracted);
        }
    }

    None
}

pub(super) fn region_hint_from_pairs(pairs: &[(String, String)]) -> Option<String> {
    const HOST_KEYS: &[&str] = &[
        "wf_baseUrl",
        "cname",
        "domains",
        "region_host",
        "api_host",
        "domain",
        "host",
    ];
    for key in HOST_KEYS {
        if let Some(raw) = cookie_value(pairs, &[*key]) {
            let decoded = decode_possibly_encoded(&raw);
            if let Ok(host) = validate_region_host(&decoded) {
                return Some(host);
            }
            if let Ok(value) = serde_json::from_str::<Value>(&decoded) {
                if let Some(host) = extract_host_from_value(&value) {
                    return Some(host);
                }
            }
        }
    }
    cookie_value(pairs, &["region", "country_code", "country"])
        .map(|value| percent_decode(&value))
        .filter(|value| !value.trim().is_empty())
}

pub(super) fn extract_from_login_info(raw: &str) -> Option<ExtractedLogin> {
    let decoded = decode_possibly_encoded(raw);
    let root: Value = serde_json::from_str(&decoded).ok()?;
    let token_info = match root.get("token_info") {
        Some(Value::String(inner)) => {
            serde_json::from_str::<Value>(&decode_possibly_encoded(inner)).ok()?
        }
        Some(Value::Object(map)) => Value::Object(map.clone()),
        None => root.clone(),
        _ => return None,
    };

    let user_id = json_string(&token_info, &["user_id", "userid", "userId"])
        .and_then(|value| sanitize_user_id(&value))?;
    let app_token = json_string(
        &token_info,
        &["app_token", "apptoken", "appToken", "app-token"],
    )
    .and_then(|value| sanitize_app_token(&value))?;
    let region_hint = json_string(
        &token_info,
        &["region", "region_host", "host", "domain", "api_host"],
    )
    .or_else(|| {
        json_string(
            &root,
            &["region", "region_host", "host", "domain", "api_host"],
        )
    })
    .or_else(|| extract_host_from_value(&root));

    Some(ExtractedLogin {
        user_id,
        app_token,
        region_hint,
    })
}

pub(super) fn json_string(value: &Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        match object.get(*key) {
            Some(Value::String(text)) if !text.trim().is_empty() => {
                return Some(text.trim().to_string());
            }
            Some(Value::Number(number)) => return Some(number.to_string()),
            _ => {}
        }
    }
    None
}

pub(super) fn extract_host_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => validate_region_host(text).ok(),
        Value::Object(map) => map.values().find_map(extract_host_from_value),
        Value::Array(items) => items.iter().find_map(extract_host_from_value),
        _ => None,
    }
}

pub(super) fn cookie_value(cookies: &[(String, String)], names: &[&str]) -> Option<String> {
    cookies.iter().find_map(|(name, value)| {
        names
            .iter()
            .any(|candidate| name.eq_ignore_ascii_case(candidate))
            .then(|| value.clone())
    })
}

pub(super) fn decode_possibly_encoded(raw: &str) -> String {
    let first = percent_decode(raw.trim().trim_matches('"'));
    if first.contains('%') {
        percent_decode(&first)
    } else {
        first
    }
}

pub(super) fn percent_decode(value: &str) -> String {
    let replaced = value.replace("%2C", ",").replace("%2c", ",");
    let bytes = replaced.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                output.push(((high << 4) | low) as u8);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

pub(super) fn sanitize_user_id(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 256
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        None
    } else {
        Some(value.to_string())
    }
}

pub(super) fn sanitize_app_token(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(char::is_control) {
        return None;
    }
    // A value that the system credential manager cannot store cannot be an App Token. Before, this allowed
    // up to 16 KB, six times more than Windows can store: a value that was too long failed only at the save
    // step, and the error said "Cannot write to Windows Credential Manager", with no mention of length.
    //
    // An early rejection has a second benefit: candidates are tried in order. When we reject a JSON block
    // from page storage, the next candidate (the real token in hm-user-login-info) gets its turn.
    if value.encode_utf16().count() > crate::auth::CREDENTIAL_MAX_UTF16_UNITS {
        return None;
    }
    Some(value.to_string())
}
