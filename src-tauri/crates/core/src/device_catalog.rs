use serde::Deserialize;
use std::sync::OnceLock;

const CATALOG_JSON: &str = include_str!("../../../../src/assets/devices/catalog.json");

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct CatalogDocument {
    pub version: u32,
    pub checked_at: String,
    pub sources: Vec<String>,
    pub devices: Vec<CatalogEntry>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct CatalogEntry {
    pub catalog_id: String,
    pub canonical_name: String,
    pub display_name: String,
    #[serde(default)]
    pub name_zh: Option<String>,
    pub kind: String,
    /// Product part numbers (such as `A2322`). These are **not** `deviceSource`.
    #[serde(default)]
    pub model_codes: Vec<String>,
    /// The `deviceSource` numbers from the Zepp device list.
    ///
    /// Some accounts return no product name field in the device response. Only these
    /// numbers remain (issue #4). Since 3.1.1 the source of truth is the deviceSource
    /// table in the official Zepp developer docs. A small frozen set of user-reported
    /// codes from before that table is also kept (see `scripts/assets/build-device-catalog.py`):
    ///
    /// * Store only `deviceSource`, **never `deviceType`**. The latter is a family code
    ///   (0 = watch / band, 1 = body scale), and the value 0 alone covers twenty watches.
    /// * Low codes (< 1_000_000) come only from the official table (old watches use low
    ///   codes, for example 418 = T-Rex 2). Codes 15 / 101 / 102 / 104 are not in it and are rejected.
    ///
    /// One watch can have several neighbouring codes: the low digits are color, size or China-region variants.
    #[serde(default)]
    pub device_source_codes: Vec<i64>,
    /// The `device_source_codes` that come from the official table.
    #[serde(default)]
    pub device_source_official: Vec<i64>,
    pub aliases: Vec<String>,
    pub region: Vec<String>,
    pub status: String,
    pub supported: bool,
    #[serde(default)]
    pub canonical_device_key: Option<String>,
    #[serde(default)]
    pub official_page: Option<String>,
    pub official_url: String,
    #[serde(default)]
    pub image_source_url: Option<String>,
    #[serde(default)]
    pub asset_source: Option<String>,
    #[serde(default)]
    pub image_key: Option<String>,
    #[serde(default)]
    pub asset_hash: Option<String>,
    pub checked_at: String,
    pub provenance: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogMatchStatus {
    Exact,
    Alias,
}

#[derive(Debug, Clone)]
pub struct CatalogMatch<'a> {
    pub entry: &'a CatalogEntry,
    pub status: CatalogMatchStatus,
}

#[derive(Debug, Default)]
pub struct CatalogMatchInput<'a> {
    /// The `deviceSource` numbers from the device response. Put only `deviceSource`
    /// here, not `deviceType`. See `CatalogEntry::device_source_codes`.
    pub device_source_codes: Vec<i64>,
    pub model_codes: Vec<&'a str>,
    pub product_names: Vec<&'a str>,
    pub device_names: Vec<&'a str>,
    pub display_name: Option<&'a str>,
}

fn document() -> &'static CatalogDocument {
    static DOCUMENT: OnceLock<CatalogDocument> = OnceLock::new();
    DOCUMENT.get_or_init(|| {
        serde_json::from_str(CATALOG_JSON).expect("bundled device catalog must be valid JSON")
    })
}

pub fn catalog_entries() -> &'static [CatalogEntry] {
    &document().devices
}

pub fn normalize_model(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn words(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word.to_lowercase())
        .collect()
}

fn contains_complete_alias(display_name: &str, alias: &str) -> bool {
    let alias_words = words(alias);
    // A single generic word (for example, "Balance") cannot identify one
    // product from a nickname. Numbered or multi-word aliases are stable.
    if alias_words.len() < 2
        && !alias_words
            .iter()
            .any(|word| word.chars().any(|c| c.is_ascii_digit()))
    {
        return false;
    }
    // Product aliases can be embedded in a user nickname written in CJK
    // characters (for example, a Chinese name prefix followed by "T-Rex 3").  The old word-window
    // matcher merged the CJK prefix and the first Latin token, so it never
    // saw the complete alias.  Match the punctuation-free alias while still
    // requiring ASCII boundaries, which rejects near misses such as
    // "T-Rex 30" without inventing a product for arbitrary text.
    let display = normalize_model(display_name);
    let needle = normalize_model(alias);
    if needle.is_empty() {
        return false;
    }
    let mut offset = 0;
    while let Some(found) = display[offset..].find(&needle) {
        let start = offset + found;
        let end = start + needle.len();
        let before = display[..start].chars().next_back();
        let after = display[end..].chars().next();
        let ascii_boundary = |character: Option<char>| {
            character
                .map(|value| !value.is_ascii_alphanumeric())
                .unwrap_or(true)
        };
        if ascii_boundary(before) && ascii_boundary(after) {
            return true;
        }
        offset = start + needle.len();
        if offset >= display.len() {
            break;
        }
    }
    false
}

pub fn match_catalog(input: &CatalogMatchInput<'_>) -> Option<CatalogMatch<'static>> {
    // Check deviceSource first. It is the only field that points to exactly one
    // watch, and the name fields are missing on these accounts.
    for candidate in &input.device_source_codes {
        if let Some(entry) = catalog_entries().iter().find(|entry| {
            entry.supported
                && entry.status == "active"
                && entry.device_source_codes.contains(candidate)
        }) {
            return Some(CatalogMatch {
                entry,
                status: CatalogMatchStatus::Exact,
            });
        }
    }

    for candidate in &input.model_codes {
        let normalized = normalize_model(candidate);
        if normalized.is_empty() {
            continue;
        }
        if let Some(entry) = catalog_entries().iter().find(|entry| {
            entry.supported
                && entry.status == "active"
                && entry
                    .model_codes
                    .iter()
                    .any(|code| normalize_model(code) == normalized)
        }) {
            return Some(CatalogMatch {
                entry,
                status: CatalogMatchStatus::Exact,
            });
        }
    }

    for candidate in input.product_names.iter().chain(input.device_names.iter()) {
        let normalized = normalize_model(candidate);
        if normalized.is_empty() {
            continue;
        }
        if let Some(entry) = catalog_entries().iter().find(|entry| {
            entry.supported
                && entry.status == "active"
                && std::iter::once(&entry.display_name)
                    .chain(entry.aliases.iter())
                    .chain(entry.name_zh.iter())
                    .any(|alias| normalize_model(alias) == normalized)
        }) {
            return Some(CatalogMatch {
                entry,
                status: CatalogMatchStatus::Alias,
            });
        }
    }

    let display_name = input.display_name?;
    catalog_entries()
        .iter()
        .filter(|entry| entry.supported && entry.status == "active")
        .flat_map(|entry| {
            std::iter::once(&entry.display_name)
                .chain(entry.aliases.iter())
                .chain(entry.name_zh.iter())
                .filter_map(move |alias| {
                    contains_complete_alias(display_name, alias)
                        .then_some((alias.split_whitespace().count(), entry))
                })
        })
        .max_by_key(|(length, _)| *length)
        .map(|(_, entry)| CatalogMatch {
            entry,
            status: CatalogMatchStatus::Alias,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feedback_models_match_names_without_promoting_unconfirmed_sources() {
        for (name, id) in [
            ("Amazfit Pace", "amazfit-pace"),
            ("Pace", "amazfit-pace"),
            ("Helio Core", "amazfit-helio-core"),
        ] {
            let matched = match_catalog(&CatalogMatchInput {
                product_names: vec![name],
                ..Default::default()
            })
            .unwrap();
            assert_eq!(matched.entry.catalog_id, id);
            assert!(matched.entry.device_source_codes.is_empty());
        }
        for code in [62, 102, 400, 9699584] {
            assert!(match_catalog(&CatalogMatchInput {
                device_source_codes: vec![code],
                ..Default::default()
            })
            .is_none());
        }
    }

    #[test]
    fn catalog_contains_real_devices_and_version() {
        assert!(document().version >= 1);
        assert!(document().checked_at.starts_with("2026-"));
        assert!(catalog_entries()
            .iter()
            .any(|entry| entry.catalog_id == "amazfit-t-rex-3"));
        assert!(catalog_entries()
            .iter()
            .any(|entry| entry.catalog_id == "amazfit-helio-strap"));
        assert!(catalog_entries()
            .iter()
            .any(|entry| entry.catalog_id == "amazfit-helio-ring"));
    }

    #[test]
    fn original_bip_alias_does_not_capture_later_generations() {
        for (name, expected) in [
            ("Bip 1", "amazfit-bip"),
            ("Amazfit Bip", "amazfit-bip"),
            ("Bip 3 Pro", "amazfit-bip-3-pro"),
            ("Amazfit Bip 6", "amazfit-bip-6"),
        ] {
            let matched = match_catalog(&CatalogMatchInput {
                product_names: vec![name],
                ..CatalogMatchInput::default()
            })
            .unwrap();
            assert_eq!(matched.entry.catalog_id, expected);
        }
    }

    /// User-reported deviceSource numbers identify the watch.
    ///
    /// This is the only path for issue #4 accounts. Their device response has no
    /// product name field, so name matching never finds anything.
    #[test]
    fn a_device_source_number_identifies_the_model() {
        for (code, catalog_id) in [
            (8716547_i64, "amazfit-t-rex-3"),
            (9568515, "amazfit-balance-2"),
            (10289410, "amazfit-helio-strap"),
            (10158337, "amazfit-bip-6"),
            (11141379, "amazfit-balance-3"),
            // Three codes that got a second independent report.
            (10813697, "amazfit-active-max"),
            (11206915, "amazfit-bip-max"),
            (10944771, "amazfit-active-3-premium"),
        ] {
            let matched = match_catalog(&CatalogMatchInput {
                device_source_codes: vec![code],
                ..CatalogMatchInput::default()
            })
            .unwrap_or_else(|| panic!("deviceSource {code} must match a model"));
            assert_eq!(matched.entry.catalog_id, catalog_id, "deviceSource {code}");
            assert_eq!(matched.status, CatalogMatchStatus::Exact);
        }
    }

    /// Neighbouring codes of one watch all point to it. The low digits are color or size variants.
    #[test]
    fn neighbouring_device_source_numbers_stay_on_the_same_product() {
        for code in [8716544_i64, 8716545, 8716547] {
            let matched = match_catalog(&CatalogMatchInput {
                device_source_codes: vec![code],
                ..CatalogMatchInput::default()
            })
            .unwrap();
            assert_eq!(matched.entry.catalog_id, "amazfit-t-rex-3");
        }
    }

    /// The catalog never holds family codes. Low codes come only from the official table.
    ///
    /// The `deviceType` values (0, 1, 7) and the low `deviceSource` values outside the
    /// official table (15, 101, 102, 104) conflict in the reports: different users
    /// named different watches for one number (101-104 are body scales). If the catalog
    /// held them, every account with such a device would match the same model.
    #[test]
    fn low_codes_come_only_from_the_official_table() {
        for entry in catalog_entries() {
            for code in &entry.device_source_codes {
                assert!(
                    *code >= 1_000_000 || entry.device_source_official.contains(code),
                    "{} holds low code {code}, which is not in the official table",
                    entry.catalog_id
                );
            }
            for code in &entry.device_source_official {
                assert!(
                    entry.device_source_codes.contains(code),
                    "{} official code {code} is missing from the match list",
                    entry.catalog_id
                );
            }
        }
        for code in [0_i64, 1, 7, 15, 101, 102, 104] {
            assert!(
                match_catalog(&CatalogMatchInput {
                    device_source_codes: vec![code],
                    ..CatalogMatchInput::default()
                })
                .is_none(),
                "{code} must not match any model"
            );
        }
    }

    /// The official table (docs.zepp.com Device List) settles codes that crowd reports
    /// could not agree on. It also adds watches that reports never identified.
    #[test]
    fn the_official_table_settles_codes_crowd_reports_could_not() {
        for (code, catalog_id) in [
            // Four codes that were rejected before because reports conflicted.
            (10_813_699_i64, "amazfit-active-max"),
            (11_141_376, "amazfit-balance-3"),
            (7_995_649, "amazfit-gts-4-43mm"),
            (8_454_401, "amazfit-bip-5-46mm"),
            // Cheetah 2 Pro from a Korean user, and Cheetah Square that had a name but no code.
            (11_010_307, "amazfit-cheetah-2-pro"),
            (8_257_793, "amazfit-cheetah-square"),
            // Old watches with low codes from the official table.
            (418, "amazfit-t-rex-2-47mm"),
            (254, "amazfit-band-7"),
            (92, "amazfit-gts-2-mini"),
            (68, "amazfit-pop"),
            (244, "amazfit-gtr-2-new-46mm"),
            // The last crowd code added before the freeze.
            (11_092_225, "amazfit-balance-ultra"),
        ] {
            let found = match_catalog(&CatalogMatchInput {
                device_source_codes: vec![code],
                ..CatalogMatchInput::default()
            })
            .unwrap_or_else(|| panic!("{code} must match {catalog_id}"));
            assert_eq!(found.entry.catalog_id, catalog_id, "deviceSource {code}");
            assert_eq!(found.status, CatalogMatchStatus::Exact);
        }
    }

    /// Crowd codes from before the official table (not listed in it) still resolve. They are frozen, not deleted.
    #[test]
    fn frozen_crowd_codes_still_resolve() {
        for (code, catalog_id) in [
            (10_486_017_i64, "amazfit-balance-2"),
            (9_978_112, "amazfit-cheetah-2-ultra"),
            (10_289_411, "amazfit-helio-strap"),
            (8_651_008, "amazfit-helio-ring"),
            (11_145_475, "amazfit-balance-3"),
            (11_272_451, "amazfit-helio-strap-pro"),
            (11_469_059, "amazfit-helio-strap-pro"),
        ] {
            let found = match_catalog(&CatalogMatchInput {
                device_source_codes: vec![code],
                ..CatalogMatchInput::default()
            })
            .unwrap_or_else(|| panic!("{code} must match {catalog_id}"));
            assert_eq!(found.entry.catalog_id, catalog_id);
            assert!(!found.entry.device_source_official.contains(&code));
        }
    }

    /// Reported models that had no catalog card match by name and can be picked by hand, but claim no codes.
    #[test]
    fn reported_models_without_codes_are_selectable_by_name() {
        for (name, catalog_id) in [
            ("Amazfit NEO", "amazfit-neo"),
            ("Bip S", "amazfit-bip-s"),
            ("Mi Band 4 NFC", "mi-smart-band-4-nfc"),
            ("Balance 3 Ti", "amazfit-balance-3"),
            ("Zepp E Square", "zepp-e-square"),
        ] {
            let found = match_catalog(&CatalogMatchInput {
                product_names: vec![name],
                ..CatalogMatchInput::default()
            })
            .unwrap_or_else(|| panic!("{name} must match {catalog_id}"));
            assert_eq!(found.entry.catalog_id, catalog_id);
        }
        for catalog_id in ["amazfit-neo", "amazfit-bip-s", "mi-smart-band-4-nfc"] {
            let entry = catalog_entries()
                .iter()
                .find(|entry| entry.catalog_id == catalog_id)
                .unwrap();
            assert!(entry.device_source_codes.is_empty(), "{catalog_id}");
        }
    }

    /// Balance 2 XT can be found and picked by hand, even without product art.
    ///
    /// Issue #42: it was missing from the model list, so the user could not pick it by hand.
    /// It is the first catalog entry with an empty `image_key`. The UI falls back to an inline SVG placeholder.
    /// Showing the Balance 2 image would show the wrong watch, which is worse than no image.
    #[test]
    fn balance_2_xt_is_selectable_without_product_art() {
        let entry = catalog_entries()
            .iter()
            .find(|entry| entry.catalog_id == "amazfit-balance-2-xt")
            .expect("Balance 2 XT must be in the catalog");
        assert!(entry.supported && entry.status == "active");
        assert!(entry.image_key.is_none(), "it has no product art yet");
        assert!(entry.asset_hash.is_none());
        // It is a retail variant of Balance 2, not a new canonical model.
        assert_eq!(
            entry.canonical_device_key.as_deref(),
            Some("amazfit-balance-2")
        );
        assert_eq!(entry.device_source_codes, vec![10_486_019]);

        let matched = match_catalog(&CatalogMatchInput {
            device_names: vec!["Amazfit Balance 2 XT"],
            ..CatalogMatchInput::default()
        })
        .expect("the name must match Balance 2 XT");
        assert_eq!(matched.entry.catalog_id, "amazfit-balance-2-xt");
    }

    /// Each code belongs to exactly one watch.
    #[test]
    fn every_device_source_number_belongs_to_exactly_one_product() {
        let mut seen: Vec<(i64, &str)> = Vec::new();
        for entry in catalog_entries() {
            for code in &entry.device_source_codes {
                if let Some((_, other)) = seen.iter().find(|(value, _)| value == code) {
                    panic!(
                        "deviceSource {code} is on both {other} and {}",
                        entry.catalog_id
                    );
                }
                seen.push((*code, entry.catalog_id.as_str()));
            }
        }
        assert!(seen.len() >= 20, "too few codes in the catalog: {}", seen.len());
    }

    #[test]
    fn matching_uses_code_then_exact_names_then_complete_display_alias() {
        let exact = match_catalog(&CatalogMatchInput {
            model_codes: vec!["A2323"],
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(exact.status, CatalogMatchStatus::Exact);
        assert_eq!(exact.entry.catalog_id, "amazfit-t-rex-3");

        let alias = match_catalog(&CatalogMatchInput {
            product_names: vec!["Helio Strap"],
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(alias.status, CatalogMatchStatus::Alias);
        assert_eq!(alias.entry.catalog_id, "amazfit-helio-strap");

        let display = match_catalog(&CatalogMatchInput {
            // "My T-Rex 3" with the CJK prefix "my" (\u{6211}\u{7684}).
            display_name: Some("\u{6211}\u{7684} T-Rex 3"),
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(display.entry.catalog_id, "amazfit-t-rex-3");

        let cjk_t_rex = match_catalog(&CatalogMatchInput {
            // A CJK nickname prefix glued to the alias.
            display_name: Some("\u{51cc}\u{82cd}\u{7684}T-Rex 3"),
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(cjk_t_rex.entry.catalog_id, "amazfit-t-rex-3");

        let cjk_helio = match_catalog(&CatalogMatchInput {
            display_name: Some("\u{51cc}\u{82cd}\u{7684}Helio Strap"),
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(cjk_helio.entry.catalog_id, "amazfit-helio-strap");

        let pro = match_catalog(&CatalogMatchInput {
            product_names: vec!["T-Rex 3 Pro 48mm"],
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(pro.entry.catalog_id, "amazfit-t-rex-3-pro-48-44mm");

        let ultra = match_catalog(&CatalogMatchInput {
            // "My Amazfit T-Rex Ultra" with the CJK prefix "my" (\u{6211}\u{7684}).
            display_name: Some("\u{6211}\u{7684} Amazfit T-Rex Ultra"),
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(ultra.entry.catalog_id, "amazfit-t-rex-ultra-47mm");

        let square = match_catalog(&CatalogMatchInput {
            product_names: vec!["Active 2 Square"],
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(square.entry.catalog_id, "amazfit-active-2-square");

        let bip_pro = match_catalog(&CatalogMatchInput {
            product_names: vec!["Bip 3 Pro"],
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(bip_pro.entry.catalog_id, "amazfit-bip-3-pro");

        let bip_five = match_catalog(&CatalogMatchInput {
            // "My Amazfit Bip 5" with the CJK prefix "my" (\u{6211}\u{7684}).
            display_name: Some("\u{6211}\u{7684} Amazfit Bip 5"),
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(bip_five.entry.catalog_id, "amazfit-bip-5-46mm");

        let bip_six = match_catalog(&CatalogMatchInput {
            // "My Bip 6" with the CJK prefix "my" (\u{6211}\u{7684}).
            display_name: Some("\u{6211}\u{7684} Bip 6"),
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(bip_six.entry.catalog_id, "amazfit-bip-6");

        let black = match_catalog(&CatalogMatchInput {
            product_names: vec!["GTR 4 46mm Black"],
            ..CatalogMatchInput::default()
        })
        .unwrap();
        assert_eq!(black.entry.catalog_id, "amazfit-gtr-4-46mm-black");

        assert!(match_catalog(&CatalogMatchInput {
            display_name: Some("Unknown band"),
            ..CatalogMatchInput::default()
        })
        .is_none());

        assert!(match_catalog(&CatalogMatchInput {
            product_names: vec!["Helio Armband"],
            ..CatalogMatchInput::default()
        })
        .is_none());
    }
}
