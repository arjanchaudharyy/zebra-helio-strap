//! Strength exercise catalog commands. The local cache comes first. Zepp is asked only when the cache is stale (`/sport/config/muscle`, with `lang` + `cv`).

use super::*;
use crate::auth::AuthManager;
use std::collections::BTreeMap;
use std::time::Instant;
use zeppbridge_core::decoder::strength_catalog::{
    catalog_cache_is_stale, parse_strength_catalog, read_catalog_cache, write_catalog_cache,
    zepp_lang_for_locale, StrengthCatalogAction, StrengthCatalogCache,
    STRENGTH_CATALOG_EXPORT_LANG, STRENGTH_CATALOG_FULL_MIN,
};

/// Allows only one fetch at a time, so switching between workout details does not stack identical requests.
type FetchState = std::sync::Arc<tokio::sync::Mutex<Option<Instant>>>;
static FETCH_STATE: std::sync::Mutex<BTreeMap<&'static str, FetchState>> =
    std::sync::Mutex::new(BTreeMap::new());
const RETRY_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
pub struct StrengthCatalogResult {
    /// The Zepp language code in use (`en_US`, `pt_BR`, ...).
    pub lang: String,
    pub fetched_at: Option<String>,
    /// `fresh` = cache is new enough or just fetched; `stale` = fetch failed, old cache in use; `unavailable` = no catalog at all.
    pub status: String,
    pub actions: Vec<StrengthCatalogAction>,
}

/// Gets the exercise catalog for the UI language. A failed fetch is not an error: it returns the old cache or an empty catalog, and the UI falls back to `#id`.
#[tauri::command]
pub async fn get_strength_catalog(
    state: tauri::State<'_, AppState>,
    locale: String,
    refresh: Option<bool>,
) -> std::result::Result<StrengthCatalogResult, AppError> {
    let lang = zepp_lang_for_locale(&locale);
    if lang != STRENGTH_CATALOG_EXPORT_LANG {
        let data_dir = state.data_dir.clone();
        let auth = state.auth.clone();
        tauri::async_runtime::spawn(async move {
            refresh_if_stale(&data_dir, &auth, STRENGTH_CATALOG_EXPORT_LANG).await;
        });
    }
    let mut cache = read_catalog_cache(&state.data_dir, lang);
    let mut stale = catalog_cache_is_stale(cache.as_ref(), Utc::now());
    if stale && (cache.is_none() || refresh.unwrap_or(false)) {
        if let Some(fresh) = refresh_if_stale(&state.data_dir, &state.auth, lang).await {
            cache = Some(fresh);
            stale = false;
        }
    }
    Ok(match cache {
        Some(cache) => StrengthCatalogResult {
            lang: cache.lang,
            fetched_at: Some(cache.fetched_at),
            status: if stale { "stale" } else { "fresh" }.to_string(),
            actions: cache.actions,
        },
        None => StrengthCatalogResult {
            lang: lang.to_string(),
            fetched_at: None,
            status: "unavailable".to_string(),
            actions: Vec::new(),
        },
    })
}

/// Fetches one language and writes the cache. Returns `None` when the legacy connector is not linked (official sign-in only, no apptoken),
/// when the network fails, or when the catalog comes back empty. The caller then keeps the old cache.
async fn refresh_if_stale(
    data_dir: &Path,
    auth: &AuthManager,
    lang: &'static str,
) -> Option<StrengthCatalogCache> {
    let fetch = FETCH_STATE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(lang)
        .or_default()
        .clone();
    let mut failed = fetch.lock().await;
    let cache = read_catalog_cache(data_dir, lang);
    if !catalog_cache_is_stale(cache.as_ref(), Utc::now()) {
        return cache;
    }
    if failed.is_some_and(|last| last.elapsed() < RETRY_AFTER) {
        return None;
    }
    let result = refresh_strength_catalog(data_dir, auth, lang).await;
    *failed = result.is_none().then(Instant::now);
    result
}

async fn refresh_strength_catalog(
    data_dir: &Path,
    auth: &AuthManager,
    lang: &str,
) -> Option<StrengthCatalogCache> {
    let auth = auth.load_auth().ok().flatten()?;
    let connector = ZeppConnector::new(auth).ok()?;
    let payload = match connector.fetch_strength_catalog(lang).await {
        Ok(payload) => payload,
        Err(error) => {
            crate::diagnostics::log(&format!(
                "Strength exercise catalog fetch failed ({lang}): {}",
                error.code()
            ));
            return None;
        }
    };
    let actions = parse_strength_catalog(&payload);
    if actions.is_empty() {
        crate::diagnostics::log(&format!("Strength exercise catalog is empty ({lang}). Keeping the old cache"));
        return None;
    }
    let existing = read_catalog_cache(data_dir, lang);
    // The server returned only an old subset, and a full catalog is already on this device. Do not overwrite
    // the larger catalog with the smaller one. Only move the timestamp forward and retry in one day.
    let cache = match existing {
        Some(mut full)
            if actions.len() < STRENGTH_CATALOG_FULL_MIN
                && full.actions.len() >= STRENGTH_CATALOG_FULL_MIN =>
        {
            crate::diagnostics::log(&format!(
                "Strength exercise catalog returned only {} entries ({lang}). Keeping the full local catalog",
                actions.len()
            ));
            full.fetched_at = Utc::now().to_rfc3339();
            full
        }
        _ => StrengthCatalogCache {
            version: 1,
            lang: lang.to_string(),
            fetched_at: Utc::now().to_rfc3339(),
            actions,
        },
    };
    if let Err(error) = write_catalog_cache(data_dir, &cache) {
        crate::diagnostics::log(&format!("Could not write the strength exercise catalog cache: {error}"));
    }
    Some(cache)
}
