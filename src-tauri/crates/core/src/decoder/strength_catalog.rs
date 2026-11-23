//! Zepp strength exercise catalog (`GET /sport/config/muscle`): parsing, muscle group mapping and a local file cache.
//!
//! The static table in `strength.rs` covers only the 125 old exercises (0..=130). New firmware uses codes from 1001
//! up to 10067 (about one thousand, measured in GitHub #163), and Zepp keeps adding more.
//! Only Zepp knows the names and muscle groups of these exercises:
//!
//! - The request header `lang` (such as `en_US`, `pt_BR`) sets the language. It is not a query parameter.
//! - **The request must send `cv` (the app build, such as `151871_10.4.0`)**. Without it, Zepp returns only about 125 old entries.
//! - Each exercise gives `mainPositions` / `subPositions` (primary / secondary muscle position numbers). Position numbers do not depend on language.
//!
//! See [`muscle_group_for_position`] for the position-to-group mapping. Unknown position numbers get no group;
//! we do not guess. Names are stored exactly as Zepp sends them (one copy per language). The backend does not translate or rewrite them.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The app build we send when we fetch the catalog. Zepp uses it to choose the old subset or the full catalog.
/// This is the value the phone app sends, as captured in #163.
pub const STRENGTH_CATALOG_CV: &str = "151871_10.4.0";
/// The `vn` version header that goes with [`STRENGTH_CATALOG_CV`].
pub const STRENGTH_CATALOG_VN: &str = "10.4.0";
/// Cache age (seconds) after which the cache is stale. Zepp adds exercises rarely, so once a month is enough.
pub const STRENGTH_CATALOG_MAX_AGE_SECONDS: i64 = 30 * 24 * 60 * 60;
/// Retry delay (seconds) when we got only the old subset.
pub const STRENGTH_CATALOG_SUBSET_RETRY_SECONDS: i64 = 24 * 60 * 60;
/// A full catalog has at least this many entries. Fewer means the server sent the old subset again (for example, `cv` is no longer accepted).
/// In that case we do not overwrite an existing full cache.
pub const STRENGTH_CATALOG_FULL_MIN: usize = 200;
/// Language of exercise names for export and MCP.
pub const STRENGTH_CATALOG_EXPORT_LANG: &str = "en_US";

/// One exercise in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StrengthCatalogAction {
    /// `actionType`. It is the same code as column 28 of the per-set `lap`.
    pub action_code: i32,
    /// Exercise name from Zepp in the request language, stored as is.
    pub name: String,
    /// Primary muscle groups (the nine group keys, deduplicated, in position order).
    #[serde(default)]
    pub primary_groups: Vec<String>,
    /// Secondary muscle groups (primary groups removed).
    #[serde(default)]
    pub secondary_groups: Vec<String>,
    /// Raw primary / secondary position numbers, kept for audit (left/right side and finer muscles may be useful later).
    #[serde(default)]
    pub main_positions: Vec<i32>,
    #[serde(default)]
    pub sub_positions: Vec<i32>,
    /// `selfWeightType`: 1 = bodyweight exercise, 0 = weighted.
    #[serde(default)]
    pub self_weight: Option<bool>,
}

/// The catalog cache for one language (`<data dir>/strength-catalog-<lang>.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StrengthCatalogCache {
    pub version: u32,
    /// Zepp language code, such as `en_US`.
    pub lang: String,
    /// RFC 3339 fetch time.
    pub fetched_at: String,
    pub actions: Vec<StrengthCatalogAction>,
}

/// Zepp muscle position number -> nine muscle group keys (the same set as `STRENGTH_MUSCLE_GROUPS`).
///
/// Source: the mapping table in #163, checked against exercise names in the full catalog.
/// Triceps 22/23, biceps 10/11, chest 5/6, deltoids 3/4/38/39, lats 19/20, trapezius 34/35,
/// erector spinae 36/37, rectus abdominis 1/2, obliques 7/8, quadriceps 14/15, hamstrings 28/29, glutes 26/27,
/// calves 16/17 and 30/31, forearms 12/13 and 24/25, neck 40/41.
/// Not in that table: 32/33 and 46/47 appear only on moves like high knees and kneeling iliopsoas stretch (hip flexors),
/// 42/43 are the inner thigh (adductors), 44/45 are the IT band / tensor fasciae latae. All of these map to legs.
/// 48/49 appear on posterior pelvic tilt, side waist stretch and high knees. The muscle is unclear, so they get no group.
pub fn muscle_group_for_position(position: i32) -> Option<&'static str> {
    Some(match position {
        1 | 2 | 7 | 8 => "core",
        3 | 4 | 38 | 39 => "shoulders",
        // The neck maps to shoulders, the same as `neck_muscles` in the static table.
        40 | 41 => "shoulders",
        5 | 6 => "chest",
        10 | 11 => "biceps",
        22 | 23 => "triceps",
        12 | 13 | 24 | 25 => "forearms",
        19 | 20 | 34 | 35 | 36 | 37 => "back",
        26 | 27 => "glutes",
        14 | 15 | 16 | 17 | 28 | 29 | 30 | 31 | 32 | 33 | 42 | 43 | 44 | 45 | 46 | 47 => "legs",
        _ => return None,
    })
}

fn groups_for_positions(positions: &[i32], exclude: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for group in positions
        .iter()
        .filter_map(|position| muscle_group_for_position(*position))
    {
        if !out.iter().any(|g| g == group) && !exclude.iter().any(|g| g == group) {
            out.push(group.to_string());
        }
    }
    out
}

fn int_list(value: Option<&Value>) -> Vec<i32> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                item.as_i64()
                    .or_else(|| item.as_str().and_then(|s| s.trim().parse().ok()))
            })
            .filter_map(|v| i32::try_from(v).ok())
            .collect(),
        // Some endpoints send the array as a string such as "3,4,38".
        Some(Value::String(text)) => text
            .split(',')
            .filter_map(|part| part.trim().parse::<i32>().ok())
            .collect(),
        _ => Vec::new(),
    }
}

/// Parses the `/sport/config/muscle` response. Accepts a bare array, and also wrappers such as `{data: [...]}`
/// and `{data: {list: [...]}}`. Unknown entries are skipped without an error.
pub fn parse_strength_catalog(payload: &Value) -> Vec<StrengthCatalogAction> {
    let items = match payload {
        Value::Array(items) => items.as_slice(),
        Value::Object(map) => match map.get("data") {
            Some(Value::Array(items)) => items.as_slice(),
            Some(Value::Object(inner)) => match inner.get("list").or_else(|| inner.get("items")) {
                Some(Value::Array(items)) => items.as_slice(),
                _ => &[],
            },
            _ => &[],
        },
        _ => &[],
    };
    let mut out: Vec<StrengthCatalogAction> = Vec::new();
    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let Some(action_code) = obj
            .get("actionType")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            })
            .and_then(|v| i32::try_from(v).ok())
            .filter(|code| (0..=100_000).contains(code))
        else {
            continue;
        };
        let name = obj
            .get("actionName")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if name.is_empty() || name.chars().count() > 200 {
            continue;
        }
        if out
            .iter()
            .any(|existing| existing.action_code == action_code)
        {
            continue;
        }
        let main_positions = int_list(obj.get("mainPositions"));
        let sub_positions = int_list(obj.get("subPositions"));
        let primary_groups = groups_for_positions(&main_positions, &[]);
        let secondary_groups = groups_for_positions(&sub_positions, &primary_groups);
        let self_weight = obj
            .get("selfWeightType")
            .and_then(Value::as_i64)
            .map(|v| v == 1);
        out.push(StrengthCatalogAction {
            action_code,
            name: name.to_string(),
            primary_groups,
            secondary_groups,
            main_positions,
            sub_positions,
            self_weight,
        });
    }
    out.sort_by_key(|action| action.action_code);
    out
}

/// UI language (`zh`, `pt-BR`, ...) -> Zepp `lang` header. Zepp returns English for languages it does not know,
/// so this only needs to produce the correct format.
pub fn zepp_lang_for_locale(locale: &str) -> &'static str {
    let lower = locale.trim().to_ascii_lowercase().replace('_', "-");
    match lower.as_str() {
        "zh-tw" | "zh-hk" | "zh-mo" | "zh-hant" => "zh_TW",
        l if l.starts_with("zh-hant") => "zh_TW",
        l if l.starts_with("zh") => "zh_CN",
        "pt-pt" => "pt_PT",
        l if l.starts_with("pt") => "pt_BR",
        l if l.starts_with("es") => "es_ES",
        l if l.starts_with("de") => "de_DE",
        l if l.starts_with("fr") => "fr_FR",
        l if l.starts_with("it") => "it_IT",
        l if l.starts_with("nl") => "nl_NL",
        l if l.starts_with("pl") => "pl_PL",
        l if l.starts_with("ru") => "ru_RU",
        l if l.starts_with("hi") => "hi_IN",
        l if l.starts_with("ja") => "ja_JP",
        l if l.starts_with("ko") => "ko_KR",
        _ => "en_US",
    }
}

/// Allows only the `xx` / `xx_YY` shapes, so arbitrary strings never go into request headers or file names.
pub fn is_valid_zepp_lang(lang: &str) -> bool {
    let bytes = lang.as_bytes();
    match bytes.len() {
        2 => bytes.iter().all(u8::is_ascii_lowercase),
        5 => {
            bytes[..2].iter().all(u8::is_ascii_lowercase)
                && bytes[2] == b'_'
                && bytes[3..].iter().all(u8::is_ascii_uppercase)
        }
        _ => false,
    }
}

pub fn catalog_cache_path(data_dir: &Path, lang: &str) -> Option<PathBuf> {
    is_valid_zepp_lang(lang).then(|| data_dir.join(format!("strength-catalog-{lang}.json")))
}

pub fn read_catalog_cache(data_dir: &Path, lang: &str) -> Option<StrengthCatalogCache> {
    let path = catalog_cache_path(data_dir, lang)?;
    let raw = std::fs::read_to_string(path).ok()?;
    let cache = serde_json::from_str::<StrengthCatalogCache>(&raw).ok()?;
    (cache.lang == lang && !cache.actions.is_empty()).then_some(cache)
}

pub fn write_catalog_cache(data_dir: &Path, cache: &StrengthCatalogCache) -> std::io::Result<()> {
    let Some(path) = catalog_cache_path(data_dir, &cache.lang) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid catalog language",
        ));
    };
    let encoded = serde_json::to_vec(cache).map_err(std::io::Error::other)?;
    crate::paths::write_file_atomically(&path, &encoded)
}

/// Whether the cache needs a refetch: missing, time unreadable, or older than a month. For the old subset only, older than a day.
pub fn catalog_cache_is_stale(
    cache: Option<&StrengthCatalogCache>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    let Some(cache) = cache else {
        return true;
    };
    // With only the old subset (the server did not accept `cv`), retry after a day, not on every app start.
    let max_age = if cache.actions.len() < STRENGTH_CATALOG_FULL_MIN {
        STRENGTH_CATALOG_SUBSET_RETRY_SECONDS
    } else {
        STRENGTH_CATALOG_MAX_AGE_SECONDS
    };
    match chrono::DateTime::parse_from_rfc3339(&cache.fetched_at) {
        Ok(at) => (now - at.with_timezone(&chrono::Utc)).num_seconds() > max_age,
        Err(_) => true,
    }
}

/// Any cached catalog (muscle groups do not depend on language, so any copy works). The export language copy comes first.
pub fn read_any_catalog_cache(data_dir: &Path) -> Option<StrengthCatalogCache> {
    if let Some(cache) = read_catalog_cache(data_dir, STRENGTH_CATALOG_EXPORT_LANG) {
        return Some(cache);
    }
    let entries = std::fs::read_dir(data_dir).ok()?;
    let mut best: Option<StrengthCatalogCache> = None;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(lang) = name
            .to_str()
            .and_then(|n| n.strip_prefix("strength-catalog-"))
            .and_then(|n| n.strip_suffix(".json"))
        else {
            continue;
        };
        if let Some(cache) = read_catalog_cache(data_dir, lang) {
            if best
                .as_ref()
                .is_none_or(|b| cache.actions.len() > b.actions.len())
            {
                best = Some(cache);
            }
        }
    }
    best
}

fn find_action(cache: &StrengthCatalogCache, code: i32) -> Option<&StrengthCatalogAction> {
    cache
        .actions
        .binary_search_by_key(&code, |action| action.action_code)
        .ok()
        .map(|idx| &cache.actions[idx])
}

fn secondary_without(action: &StrengthCatalogAction, primary: Option<&str>) -> Vec<String> {
    action
        .secondary_groups
        .iter()
        .filter(|group| Some(group.as_str()) != primary)
        .cloned()
        .collect()
}

/// Fills in a strength workout from the catalog:
///
/// - Codes the static table does not know (new exercises from 1001) get the first mapped group of `mainPositions` as the primary group.
/// - Each set / each exercise gets its secondary muscle groups.
/// - When the catalog is English, add the Zepp English exercise name (for export and MCP; the UI picks its own language).
///
/// Primary groups that the static table already has do not change (each was checked by hand). Codes missing from the catalog stay empty.
pub fn enrich_strength_detail(
    detail: &mut crate::models::StrengthWorkoutDetail,
    cache: Option<&StrengthCatalogCache>,
) {
    let Some(cache) = cache else {
        return;
    };
    let with_names = cache.lang == STRENGTH_CATALOG_EXPORT_LANG;
    for set in &mut detail.sets {
        let Some(action) = find_action(cache, set.action_code) else {
            continue;
        };
        if set.muscle_group.is_none() {
            set.muscle_group = action.primary_groups.first().cloned();
        }
        set.secondary_muscle_groups = secondary_without(action, set.muscle_group.as_deref());
        if with_names {
            set.exercise_name = Some(action.name.clone());
        }
    }
    for group in &mut detail.exercise_groups {
        let Some(action) = find_action(cache, group.action_code) else {
            continue;
        };
        if group.muscle_group.is_none() {
            group.muscle_group = action.primary_groups.first().cloned();
        }
        group.secondary_muscle_groups = secondary_without(action, group.muscle_group.as_deref());
        if with_names {
            group.exercise_name = Some(action.name.clone());
        }
    }
}

#[cfg(test)]
#[path = "strength_catalog_tests.rs"]
mod tests;
