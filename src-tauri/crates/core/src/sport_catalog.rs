use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

const CATALOG_JSON: &str = include_str!("../../../../src/assets/workouts/catalog.json");

#[derive(Debug, Deserialize)]
struct CatalogDocument {
    sports: Vec<SportEntry>,
}

#[derive(Debug, Deserialize)]
struct SportEntry {
    /// The Zepp numeric type. `null` means **no** known number produces this sport.
    ///
    /// The catalog does two jobs: it maps Zepp numbers to sports, and it gives the user
    /// a list of sports for manual correction. The two lists differ. For some sports we
    /// know the sport exists and users need it, but we do not know the Zepp number. A
    /// guessed number makes the normalizer assign all records with that number to this
    /// sport, and a guess becomes wrong history. So the number stays empty: the sport is
    /// in the correction list, but it is never assigned automatically.
    #[serde(default)]
    code: Option<i64>,
    key: String,
    label_zh: String,
}

/// One sport in the catalog, for the correction dropdown in the UI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SportOption {
    pub key: String,
    pub label: String,
}

fn entries() -> &'static HashMap<i64, String> {
    static ENTRIES: OnceLock<HashMap<i64, String>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        let document: CatalogDocument =
            serde_json::from_str(CATALOG_JSON).expect("bundled workout catalog must be valid JSON");
        document
            .sports
            .into_iter()
            .filter_map(|entry| entry.code.map(|code| (code, entry.key)))
            .collect()
    })
}

pub fn resolve(type_id: i64) -> Option<&'static str> {
    entries().get(&type_id).map(String::as_str)
}

/// Cloud history and Zepp OS protocol numbers are separate namespaces (#155).
/// The conflicting cloud code 15 is only established for Balance 2.
pub fn resolve_cloud(
    type_id: i64,
    device_source: Option<i64>,
    name: Option<&str>,
) -> Option<&'static str> {
    resolve_cloud_with_device(type_id, device_source, name, None)
}

/// `catalog_id` must belong to the physical device identified by this row,
/// never merely the account's current or only watch. Conflicting recognised
/// identities are insufficient evidence for the device-specific code 15.
pub fn resolve_cloud_with_device(
    type_id: i64,
    device_source: Option<i64>,
    name: Option<&str>,
    catalog_id: Option<&str>,
) -> Option<&'static str> {
    if type_id != 15 {
        return resolve(type_id);
    }
    let source = crate::device_catalog::match_catalog(&crate::device_catalog::CatalogMatchInput {
        device_source_codes: device_source.into_iter().collect(),
        ..Default::default()
    });
    let named = crate::device_catalog::match_catalog(&crate::device_catalog::CatalogMatchInput {
        device_names: name.into_iter().collect(),
        ..Default::default()
    });
    let local = catalog_id.and_then(|id| {
        crate::device_catalog::catalog_entries()
            .iter()
            .find(|entry| entry.catalog_id == id)
    });
    let mut identities = source
        .iter()
        .map(|matched| matched.entry)
        .chain(named.iter().map(|matched| matched.entry))
        .chain(local);
    let first = identities.next()?;
    let key = |entry: &crate::device_catalog::CatalogEntry| {
        entry
            .canonical_device_key
            .clone()
            .unwrap_or_else(|| entry.catalog_id.clone())
    };
    let expected = key(first);
    if expected != "amazfit-balance-2" || identities.any(|entry| key(entry) != expected) {
        return None;
    }
    Some("open_water_swimming")
}

/// Text sport type name -> catalog key. Import files (GPX `<type>`, Strava, TCX), text
/// types in legacy channel payloads and type names the official API does not know share
/// this table (#157: `mountain_bike` / `MTB` stayed as is before, so the FIT export
/// became a generic sport).
///
/// Only unambiguous names are here. An unknown name returns `None`, and the caller
/// decides the fallback. "Swimming" (pool or open water is unknown) is not here; the
/// importer applies its own fallback.
pub fn text_alias(value: &str) -> Option<&'static str> {
    let key: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();
    Some(match key.as_str() {
        "run" | "running" => "run",
        "trailrun" | "trailrunning" => "trail_running",
        "virtualrun" | "treadmill" => "treadmill",
        "ride" | "biking" | "cycling" | "bike" | "gravelride" => "ride",
        "virtualride" | "indoorcycling" => "indoor_cycling",
        "mountainbikeride" | "mountainbiking" | "mountainbike" | "mountaincycling" | "mtb" => {
            "mountain_biking"
        }
        "ebikeride" | "ebike" => "e_bike",
        "walk" | "walking" => "walking",
        "hike" | "hiking" => "hiking",
        "weighttraining" | "strengthtraining" | "strength" => "strength",
        "yoga" => "yoga",
        "pilates" => "pilates",
        "rowing" => "rowing",
        "elliptical" => "elliptical",
        "nordicski" | "crosscountryskiing" => "cross_country_skiing",
        "hiit" | "highintensityintervaltraining" => "hiit",
        "kayaking" => "kayaking",
        "rockclimbing" | "rockclimb" => "rock_climbing",
        "soccer" | "football" => "soccer",
        "tennis" => "tennis",
        "badminton" => "badminton",
        "basketball" => "basketball",
        _ => return None,
    })
}

fn known_keys() -> &'static HashSet<String> {
    static KEYS: OnceLock<HashSet<String>> = OnceLock::new();
    KEYS.get_or_init(|| options().iter().map(|entry| entry.key.clone()).collect())
}

/// The allowed values when the user corrects a sport type.
///
/// The bundled catalog is the source, not a short fixed list. The catalog has more than
/// a hundred sports. With only a dozen allowed values, the user cannot even pick
/// "squash", and a record keeps the wrong type.
pub fn is_known_key(key: &str) -> bool {
    known_keys().contains(key)
}

/// The deduplicated sport options, sorted by Chinese name, for the UI to render directly.
pub fn options() -> &'static [SportOption] {
    static OPTIONS: OnceLock<Vec<SportOption>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let document: CatalogDocument =
            serde_json::from_str(CATALOG_JSON).expect("bundled workout catalog must be valid JSON");
        let mut seen = HashSet::new();
        let mut options: Vec<SportOption> = document
            .sports
            .into_iter()
            .filter(|entry| seen.insert(entry.key.clone()))
            .map(|entry| SportOption {
                key: entry.key,
                label: entry.label_zh,
            })
            .collect();
        options.sort_by(|a, b| a.label.cmp(&b.label).then(a.key.cmp(&b.key)));
        options
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_15_requires_unambiguous_balance_2_cloud_context() {
        assert_eq!(resolve(15), None);
        assert_eq!(resolve_cloud(15, None, None), None);
        assert_eq!(
            resolve_cloud(15, Some(9568512), None),
            Some("open_water_swimming")
        );
        assert_eq!(
            resolve_cloud(15, None, Some("Amazfit Balance 2")),
            Some("open_water_swimming")
        );
        assert_eq!(
            resolve_cloud(15, Some(9568512), Some("Amazfit T-Rex 3")),
            None
        );
        assert_eq!(
            resolve_cloud_with_device(15, Some(9568512), None, Some("amazfit-t-rex-3")),
            None
        );
        assert_eq!(
            resolve_cloud_with_device(15, None, None, Some("amazfit-balance-2")),
            Some("open_water_swimming")
        );
        assert_eq!(
            resolve_cloud_with_device(15, None, None, Some("not-a-device")),
            None
        );
        assert_eq!(resolve_cloud(224, None, None), Some("hiking"));
    }

    /// #157: text names for mountain biking (`mountain_bike`, `MTB`, the official
    /// `MOUNTAIN_BIKING`) all map to `mountain_biking`. Each key in the alias table must be in the catalog.
    #[test]
    fn text_aliases_reach_catalog_keys() {
        for name in [
            "mountain_bike",
            "MTB",
            "MOUNTAIN_BIKING",
            "Mountain Bike Ride",
        ] {
            assert_eq!(text_alias(name), Some("mountain_biking"), "{name}");
        }
        assert_eq!(text_alias("Running"), Some("run"));
        assert_eq!(
            text_alias("swimming"),
            None,
            "pool or open water is unknown, so do not guess"
        );
        assert_eq!(text_alias("tug_of_war"), None);
        for name in [
            "run",
            "trailrun",
            "treadmill",
            "ride",
            "indoorcycling",
            "mtb",
            "ebike",
            "walk",
            "hike",
            "strength",
            "yoga",
            "pilates",
            "rowing",
            "elliptical",
            "nordicski",
            "hiit",
            "kayaking",
            "rockclimbing",
            "soccer",
            "tennis",
            "badminton",
            "basketball",
        ] {
            let key = text_alias(name).unwrap();
            assert!(is_known_key(key), "{name} -> {key} is not in the catalog");
        }
    }

    #[test]
    fn catalog_keeps_cloud_overrides_and_covers_extended_types() {
        assert_eq!(resolve(6), Some("walking"));
        assert_eq!(resolve(9), Some("ride"));
        assert_eq!(resolve(52), Some("strength"));
        assert_eq!(resolve(92), Some("badminton"));
        assert_eq!(resolve(130), Some("cross_training"));
        assert_eq!(resolve(225), Some("rucking"));
        assert_eq!(resolve(105), None);
    }

    /// 46 / 47 / 141 / 193: cross-country skiing, mountain biking, rowing, fishing.
    ///
    /// The evidence is issue #143 (Amazfit Balance 2), checked one by one against the Zepp App:
    /// "46 = cross-country skiing (skate), 47 = mountain bike, 141 = rowing,
    /// 193 = fishing". 141 / 193 are not the same numbers as 66 (rowing) / 64 (fishing)
    /// in the device protocol table. Cloud numbers win; see the catalog notes.
    #[test]
    fn issue_143_codes() {
        assert_eq!(resolve(46), Some("cross_country_skiing"));
        assert_eq!(resolve(47), Some("mountain_biking"));
        assert_eq!(resolve(141), Some("water_rowing"));
        assert_eq!(resolve(193), Some("fishing"));
    }

    /// 215 dog walking, 216 padel, 108 stair climbing (users checked against the Zepp App).
    /// Stair climbing and the 54 stair machine are different, so they have two keys. Alpine
    /// skiing (#157) has no known number yet; it is only for manual correction.
    #[test]
    fn codes_added_in_3_1_1() {
        assert_eq!(resolve(215), Some("dog_walking"));
        assert_eq!(resolve(216), Some("padel"));
        assert_eq!(resolve(108), Some("stair_climbing"));
        assert_eq!(resolve(54), Some("stair_climber"));
        assert!(is_known_key("alpine_skiing"));
    }

    /// On a T-Rex 3, "mountaineering" records as 13. Before, we guessed walking. These
    /// records climb much more than walking (6) and hiking (22). This matches the official
    /// channel, which puts MOUNTAINEERING under hiking.
    #[test]
    fn cloud_13_is_mountaineering_not_walking() {
        assert_eq!(resolve(13), Some("hiking"));
        assert_eq!(resolve(6), Some("walking"));
    }

    /// #155: in the table for the official deviceSource (9568515 is the Balance 2 in the official table), 15 is open water swimming.
    #[test]
    fn issue_155_balance_2_by_official_device_source_swims_in_open_water() {
        assert_eq!(
            resolve_cloud_with_device(15, Some(9568515), None, None),
            Some("open_water_swimming")
        );
    }

    /// 211 is road cycling.
    ///
    /// 10 feedback reports and 199 records have this number. One report says "road
    /// cycling with zepp code 211 was read as unknown workout". Of the 28 unknown numbers,
    /// this is the only one with text evidence. The others have only counts and no
    /// meaning, so none of them changed.
    #[test]
    fn code_211_is_road_cycling() {
        assert_eq!(resolve(211), Some("road_cycling"));
        assert!(is_known_key("road_cycling"));
    }

    /// 12 is the elliptical.
    ///
    /// As with 211, the evidence is clear text in a report, not a count. One feedback report
    /// says "Zepp workout code number 12 is an official activity, it's the Elliptical
    /// activity.". Other frequent numbers have only counts and no meaning, so none of them changed.
    ///
    /// The elliptical was not in the catalog before. This entry also fills a gap in the
    /// correction list: before, the user could not pick the elliptical manually.
    #[test]
    fn code_12_is_elliptical() {
        assert_eq!(resolve(12), Some("elliptical"));
        assert!(is_known_key("elliptical"));
    }

    /// Issue #24 supplies code 7, matching Zepp/ZeppBridge screenshots and
    /// a Zepp GPX labelled Trail running. Cloud history codes are not the
    /// Zepp OS device-protocol enum. Do not guess a replacement swimming code.
    #[test]
    fn cloud_code_7_is_trail_running_and_swimming_remains_selectable() {
        assert_eq!(resolve(7), Some("trail_running"));
        assert!(is_known_key("trail_running"));
        assert!(is_known_key("open_water_swimming"));
        assert_eq!(resolve(14), Some("pool_swimming"));
    }

    #[test]
    fn numeric_codes_are_unique() {
        let document: CatalogDocument = serde_json::from_str(CATALOG_JSON).unwrap();
        let mut codes = HashSet::new();
        for entry in document.sports {
            if let Some(code) = entry.code {
                assert!(codes.insert(code), "duplicate cloud code {code}");
            }
        }
    }

    #[test]
    fn override_allowlist_is_the_whole_catalog() {
        assert!(is_known_key("strength"));
        assert!(is_known_key("badminton"));
        assert!(!is_known_key("not-a-sport"));
        let options = options();
        assert!(
            options.len() > 100,
            "the catalog must have more than a hundred sports: {}",
            options.len()
        );
        assert!(options.iter().all(|entry| !entry.label.trim().is_empty()));
        let mut keys: Vec<&str> = options.iter().map(|entry| entry.key.as_str()).collect();
        keys.sort_unstable();
        let before = keys.len();
        keys.dedup();
        assert_eq!(before, keys.len(), "the options must not have duplicate keys");
    }
}
