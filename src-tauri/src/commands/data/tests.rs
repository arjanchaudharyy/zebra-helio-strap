use super::*;

use crate::models::{DeviceMatchStatus, DeviceProfile};
use serde_json::json;
use std::fs;

#[test]
fn nested_identity_and_helio_name_survive_empty_outer_metadata() {
    let profiles = parse_device_profiles(&json!({"devices": [{
        "deviceSource": 62, "deviceType": 0,
        "deviceId": null, "productName": "  ",
        "additionalInfo": {"deviceId": "REAL-CORE-ID", "productName": "Helio Core"}
    }]}));
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].device_id.as_deref(), Some("REAL-CORE-ID"));
    assert_eq!(
        profiles[0].catalog_id.as_deref(),
        Some("amazfit-helio-core")
    );
}

#[test]
fn source_number_does_not_hide_nested_identity_or_merge_two_devices() {
    let profiles = parse_device_profiles(&json!({"devices": [
        {"deviceSource": 10289411, "additionalInfo": {"deviceId": "STRAP-ONE"}},
        {"deviceSource": 10289411, "additionalInfo": {"deviceId": "STRAP-TWO"}}
    ]}));
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].device_id.as_deref(), Some("STRAP-ONE"));
    assert_eq!(profiles[1].device_id.as_deref(), Some("STRAP-TWO"));
}

#[test]
fn parse_device_profile_reads_additional_info() {
    let value = json!({
        "items": [{
            "deviceId": "A194",
            "displayName": "Amazfit GTR 4",
            "additionalInfo": {
                "productVersion": "3.9.1.2",
                "sn": "2143123A1B23456"
            }
        }]
    });
    let profile = parse_device_profile(&value);
    assert_eq!(profile.name.as_deref(), Some("Amazfit GTR 4"));
    assert_eq!(profile.firmware.as_deref(), Some("3.9.1.2"));
    assert_eq!(profile.serial.as_deref(), Some("2143123A1B23456"));
    assert_eq!(profile.device_id.as_deref(), Some("A194"));
}

/// The device response shape from real feedback: the object has no product name field at all,
/// only numbers such as `deviceSource` / `deviceType` / `productId`.
///
/// This test locks in two things. When the catalog has no matching code, the device must honestly be unidentified (it must not
/// be "fixed" by guessing a model). And the diagnostic report must include the model numbers, or the built-in
/// catalog can never be completed, and this watch stays unidentified for every user.
///
/// `12340001` is a code that is in neither the official table nor the catalog (the official table maps `10813699` to
/// Active Max, so it cannot be used as an "unknown code" any more).
#[test]
fn a_device_response_with_no_product_name_stays_unknown_and_reports_model_numbers() {
    let payload = json!({
        "items": [{
            "deviceId": "0123456789abcdef",
            "deviceSource": 12340001,
            "deviceType": 5,
            "macAddress": "AA:BB:CC:DD:EE:FF",
            "sn": "SERIAL-PRIVATE-998877",
            "firmwareVersion": "6.2.208.7",
            "additionalInfo": serde_json::to_string(&json!({
                "productId": "12340001",
                "productVersion": "6.2.208.7",
                "hardwareVersion": "1.0",
                "btmac": "AA:BB:CC:DD:EE:FF",
                "sn": "SERIAL-PRIVATE-998877"
            })).unwrap()
        }]
    });

    let profile = parse_device_profile(&payload);
    assert_eq!(
        profile.match_status,
        DeviceMatchStatus::Unknown,
        "When the catalog does not have these codes, do not guess a model"
    );
    assert!(profile.canonical_name.is_none());
    assert_eq!(profile.firmware.as_deref(), Some("6.2.208.7"));

}

/// When a deviceSource from collected feedback is in the catalog, devices of that model are identified automatically.
///
/// This is the other half of the test above: codes not in the catalog stay unidentified. `8716547` has seven
/// user picks, so now it is T-Rex 3 directly, and the user does not need to pick it again. This closes the whole loop
/// (user pick -> feedback data -> catalog -> automatic identification).
#[test]
fn a_contributed_device_source_number_is_recognised_without_any_product_name() {
    let payload = json!({
        "items": [{
            "deviceId": "0123456789abcdef",
            "deviceSource": 8716547,
            "deviceType": 0,
            "macAddress": "AA:BB:CC:DD:EE:FF",
            "sn": "SERIAL-PRIVATE-998877",
            "firmwareVersion": "6.2.208.7"
        }]
    });

    let profile = parse_device_profile(&payload);
    assert_eq!(profile.catalog_id.as_deref(), Some("amazfit-t-rex-3"));
    assert_eq!(profile.match_status, DeviceMatchStatus::Exact);
}

/// A body fat scale with an unknown model (`deviceType` = 1) shows as the "scale" class. It is not an unidentified device
/// and does not go into the feedback report. Codes 101-104 in the feedback data are all of this class.
#[test]
fn an_unmatched_scale_is_labelled_by_family_and_left_out_of_the_report() {
    let payload = json!({
        "items": [
            { "deviceId": "SCALE-PRIVATE-1", "deviceSource": 102, "deviceType": 1 },
            { "deviceId": "WATCH-PRIVATE-1", "deviceSource": 12340001, "deviceType": 0 },
            { "deviceId": "WATCH-PRIVATE-2", "deviceSource": 11010307, "deviceType": 0 }
        ]
    });
    let profiles = parse_device_profiles(&payload);
    assert_eq!(profiles[0].kind.as_deref(), Some("scale"));
    assert_eq!(profiles[0].match_status, DeviceMatchStatus::Unknown);
    assert!(profiles[0].canonical_name.is_none(), "A scale gets only a class, not a guessed model");
    assert_eq!(profiles[1].kind, None, "Family code 0 does not invent a class for an unknown watch");
    assert_eq!(
        profiles[2].catalog_id.as_deref(),
        Some("amazfit-cheetah-2-pro")
    );

}

/// Never use `deviceType` for a catalog lookup.
///
/// In the feedback data the one value `deviceType:0` covers twenty watch models. It looks like `deviceSource`
/// and sits next to it in the same JSON object, so it is easy to feed both to the matcher by mistake. Then every
/// device is identified as the same model.
#[test]
fn device_type_is_never_used_to_look_up_the_catalog() {
    let extra = json!({});
    let item = json!({ "deviceType": 8716547, "deviceSource": 0 });
    assert!(
        device_source_numbers(&item, &extra).is_empty(),
        "A deviceType value must not be used as deviceSource"
    );

    let item = json!({ "deviceSource": 8716547, "deviceType": 0 });
    assert_eq!(device_source_numbers(&item, &extra), vec![8716547]);
}

/// `productId` / `hardwareVersion` are sometimes internal code names that, after normalization, are identical to catalog
/// aliases. Feeding them to the matcher is not a guess: it uses an equivalence that already exists.
#[test]
fn internal_codename_fields_can_still_match_a_catalog_alias() {
    let value = json!({
        "items": [{
            "deviceId": "0123456789abcdef",
            "additionalInfo": {
                "productId": "amazfit_t-rex_3",
                "productVersion": "1.2.3.4"
            }
        }]
    });
    let profile = parse_device_profile(&value);
    assert_eq!(
        profile.canonical_name.as_deref(),
        Some("Amazfit T-Rex 3 48mm")
    );
    assert_eq!(profile.match_status, DeviceMatchStatus::Alias);
}

#[test]
fn parse_device_profiles_reads_model_names_from_additional_info() {
    let value = json!({
        "items": [{
            "deviceId": "device-pro",
            "displayName": "My Pro watch",
            "additionalInfo": {
                "productName": "Amazfit T-Rex 3 Pro",
                "deviceName": "T-Rex 3 Pro 48mm",
                "model": "T-Rex 3 Pro"
            }
        }]
    });
    let profile = parse_device_profile(&value);
    assert_eq!(profile.name.as_deref(), Some("My Pro watch"));
    assert_eq!(
        profile.canonical_name.as_deref(),
        Some("Amazfit T-Rex 3 Pro 48mm/44mm")
    );
    assert_eq!(profile.match_status, DeviceMatchStatus::Alias);
}

#[test]
fn official_workout_device_items_resolve_to_the_catalog_model() {
    // The output shape of `Database::official_workout_device_items`: an account with only official sign-in
    // cannot get the device list, so device entries are built from source / deviceName in the workout list.
    let value = json!([{
        "deviceSource": 10551555,
        "deviceName": "Amazfit T-Rex 3 Pro (48mm)"
    }]);
    let profiles = parse_device_profiles(&value);
    assert_eq!(profiles.len(), 1);
    let profile = &profiles[0];
    assert_eq!(profile.name.as_deref(), Some("Amazfit T-Rex 3 Pro (48mm)"));
    assert_eq!(
        profile.canonical_name.as_deref(),
        Some("Amazfit T-Rex 3 Pro 48mm/44mm")
    );
    assert_eq!(profile.match_status, DeviceMatchStatus::Exact);
    // With no deviceId, fall back to deviceSource as the key, so the settings row can open and the model can change.
    assert_eq!(profile.device_id.as_deref(), Some("10551555"));

    // A watch not yet in the catalog must also be in the list (with the Zepp name), so the user can open it and pick a model.
    let unknown = parse_device_profile(&json!([{
        "deviceSource": 99999999,
        "deviceName": "Amazfit Prototype"
    }]));
    assert_eq!(unknown.match_status, DeviceMatchStatus::Unknown);
    assert_eq!(unknown.name.as_deref(), Some("Amazfit Prototype"));
    assert_eq!(unknown.device_id.as_deref(), Some("99999999"));
}

#[test]
fn parse_device_profiles_reads_double_nested_bind_metadata() {
    let nested = serde_json::to_string(&json!({
        "bindDevice": serde_json::to_string(&json!({
            "productName": "Amazfit Balance 2",
            "productVersion": "6.2.208.7"
        })).unwrap()
    }))
    .unwrap();
    let value = json!({
        "items": [{
            "deviceId": "private-device-id",
            "displayName": "My watch",
            "additionalInfo": nested
        }]
    });
    let profile = parse_device_profile(&value);
    assert_eq!(profile.catalog_id.as_deref(), Some("amazfit-balance-2"));
    assert_eq!(profile.canonical_name.as_deref(), Some("Amazfit Balance 2"));
    assert_eq!(profile.firmware.as_deref(), Some("6.2.208.7"));
}

/// A firmware version must not be a device name.
///
/// The field structure comes from feedback report `404560ac` (Bip 6): its
/// `nameFieldObjects` is **0**. That device list has no
/// `displayName` / `deviceName` / `nickname` / `name`, and the only value left to use as a name is
/// `additionalInfo.hardwareVersion`. This caused two separate reports:
/// the Bip 6 user's "Says 0.135.23.0", and a Reddit report of ghost data sources
/// named `0.91.20.5` / `0.91.17.5` in the sidebar.
#[test]
fn a_firmware_version_never_becomes_the_device_name() {
    let value = json!({
        "items": [{
            "deviceId": "private-device-id",
            "deviceSource": 10158337,
            "deviceType": 0,
            "firmwareVersion": "0.135.23.0",
            "additionalInfo": {
                "sn": "SN-BIP6",
                "hardwareVersion": "0.135.23.0",
                "productId": "0.135.23.0",
                "productVersion": "0.135.23.0"
            }
        }]
    });
    let profiles = parse_device_profiles(&value);
    assert_eq!(profiles.len(), 1);
    let profile = &profiles[0];
    // This device's deviceSource is in the catalog, so the name is the real model.
    // The key point: it is never that version string.
    assert_eq!(profile.catalog_id.as_deref(), Some("amazfit-bip-6"));
    assert_eq!(profile.canonical_name.as_deref(), Some("Amazfit Bip 6"));
    // The response has no name field, so `name` stays empty. The catalog fills the display name.
    // In the old behavior both fields got `"0.135.23.0"`.
    assert_eq!(profile.name, None);
    assert_eq!(profile.display_name.as_deref(), Some("Amazfit Bip 6"));
    // The firmware version still shows as the firmware version, and catalog matching still gets
    // deviceSource. Only its use as a name is blocked.
    assert_eq!(profile.firmware.as_deref(), Some("0.135.23.0"));
}

/// The half the catalog cannot identify: the name stays empty rather than use a version number.
///
/// For `name == None` the frontend (`useDevices.ts`) shows "Unidentified device"
/// and offers a manual pick. "0.91.20.5" is not a model and gives no way out.
#[test]
fn an_unmatched_device_shows_no_name_rather_than_a_firmware_string() {
    let value = json!({
        "items": [{
            "deviceId": "MAC-UNKNOWN",
            "deviceSource": 999_999_999_i64,
            "additionalInfo": { "sn": "SN-UNKNOWN", "hardwareVersion": "0.91.20.5" }
        }]
    });
    let profiles = parse_device_profiles(&value);
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, None);
    assert_eq!(profiles[0].display_name, None);
    assert_eq!(profiles[0].firmware.as_deref(), Some("0.91.20.5"));
}

/// Many bindings of the same watch merge into one row. Two really different watches must not merge.
#[test]
fn repeated_bindings_of_one_watch_collapse_but_distinct_watches_do_not() {
    let value = json!({
        "items": [
            { "deviceId": "MAC-ONE", "additionalInfo": { "sn": "SN-ONE", "hardwareVersion": "0.91.20.5" } },
            { "deviceId": "MAC-ONE", "additionalInfo": { "sn": "SN-ONE", "hardwareVersion": "0.91.20.5" } },
            { "deviceId": "MAC-TWO", "additionalInfo": { "sn": "SN-TWO", "hardwareVersion": "0.91.20.5" } }
        ]
    });
    let profiles = parse_device_profiles(&value);
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].device_id.as_deref(), Some("MAC-ONE"));
    assert_eq!(profiles[1].device_id.as_deref(), Some("MAC-TWO"));
    assert!(profiles.iter().all(|profile| profile.name.is_none()));
}

#[test]
fn parse_device_profiles_keeps_every_device() {
    let value = json!({
        "items": [
            {
                "deviceId": "MAC-ONE",
                "displayName": "Watch One",
                "additionalInfo": { "sn": "SN-ONE", "productVersion": "1.0.0" }
            },
            {
                "deviceId": "MAC-TWO",
                "displayName": "Watch Two",
                "additionalInfo": { "sn": "SN-TWO", "productVersion": "2.0.0" }
            }
        ]
    });
    let profiles = parse_device_profiles(&value);
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].serial.as_deref(), Some("SN-ONE"));
    assert_eq!(profiles[1].device_id.as_deref(), Some("MAC-TWO"));
    assert_ne!(profiles[0].device_id, profiles[1].device_id);
}

#[test]
fn catalog_matching_preserves_nickname_and_covers_real_devices() {
    let value = json!({
        "items": [
            {
                "deviceId": "A2323",
                "displayName": "My outdoor watch",
                "productName": "Amazfit T-Rex 3"
            },
            {
                "deviceId": "strap-1",
                "displayName": "Training strap",
                "productName": "Helio Strap"
            },
            {
                "deviceId": "A2321",
                "displayName": "Night ring",
                "productName": "Amazfit Helio Ring"
            },
            {
                "deviceId": "unknown-1",
                "displayName": "Unknown device"
            }
        ]
    });
    let profiles = parse_device_profiles(&value);
    assert_eq!(profiles.len(), 4);
    assert_eq!(profiles[0].name.as_deref(), Some("My outdoor watch"));
    assert_eq!(
        profiles[0].canonical_name.as_deref(),
        Some("Amazfit T-Rex 3 48mm")
    );
    assert_eq!(profiles[0].catalog_id.as_deref(), Some("amazfit-t-rex-3"));
    assert_eq!(profiles[0].match_status, DeviceMatchStatus::Exact);
    assert_eq!(
        profiles[1].catalog_id.as_deref(),
        Some("amazfit-helio-strap")
    );
    assert_eq!(profiles[1].match_status, DeviceMatchStatus::Alias);
    assert_eq!(
        profiles[2].catalog_id.as_deref(),
        Some("amazfit-helio-ring")
    );
    assert_eq!(profiles[2].match_status, DeviceMatchStatus::Exact);
    assert_eq!(profiles[3].match_status, DeviceMatchStatus::Unknown);
    assert!(profiles[3].catalog_id.is_none());
}

#[test]
fn indexed_identity_keeps_nickname_and_recovers_cached_catalog_fields() {
    let indexed = super::DeviceProfile {
        name: Some("My outdoor watch".into()),
        device_id: Some("A2323".into()),
        match_status: DeviceMatchStatus::Unknown,
        ..Default::default()
    };
    let cached = super::DeviceProfile {
        name: Some("My outdoor watch".into()),
        display_name: Some("My outdoor watch".into()),
        canonical_name: Some("Amazfit T-Rex 3 48mm".into()),
        catalog_id: Some("amazfit-t-rex-3".into()),
        kind: Some("watch".into()),
        image_key: Some("amazfit-t-rex-3".into()),
        match_status: DeviceMatchStatus::Exact,
        device_id: Some("A2323".into()),
        ..Default::default()
    };
    let merged = merge_cached_device_profile(indexed, cached);
    assert_eq!(merged.name.as_deref(), Some("My outdoor watch"));
    assert_eq!(
        merged.canonical_name.as_deref(),
        Some("Amazfit T-Rex 3 48mm")
    );
    assert_eq!(merged.catalog_id.as_deref(), Some("amazfit-t-rex-3"));
    assert_eq!(merged.match_status, DeviceMatchStatus::Exact);
}

#[test]
fn legacy_devices_json_is_read_with_new_defaults() {
    let suffix = zeppbridge_core::unique_suffix();
    let dir = std::env::temp_dir().join(format!("zeppbridge-device-cache-{suffix}"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("devices.json"),
        r#"[{"name":"Legacy T-Rex","device_id":"SN-LEGACY"}]"#,
    )
    .unwrap();
    let cache = read_device_profile_cache(&dir);
    assert_eq!(cache.profiles.len(), 1);
    assert_eq!(cache.profiles[0].name.as_deref(), Some("Legacy T-Rex"));
    assert_eq!(cache.profiles[0].match_status, DeviceMatchStatus::Unknown);
    assert!(cache.cached_at.is_some());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn fused_and_unknown_profiles_never_claim_a_catalog_device() {
    let fused = super::DeviceProfile {
        name_code: Some("ui.device.fused".to_string()),
        match_status: DeviceMatchStatus::Unknown,
        ..Default::default()
    };
    assert!(fused.catalog_id.is_none());
    let unknown = unknown_device_profile("mystery");
    assert_eq!(unknown.match_status, DeviceMatchStatus::Unknown);
    assert!(unknown.catalog_id.is_none());
    // English users saw a Chinese "device undetermined" label in workout details. With no specific device, give only a code, no display name.
    assert_eq!(unknown.name, None);
    assert_eq!(unknown.display_name, None);
    assert_eq!(unknown.name_code.as_deref(), Some("ui.device.undetermined"));
}

#[test]
fn ai_handoff_redacts_nested_identifiers_and_precise_route_by_default() {
    let source = json!({
        "user": { "id": "user-secret", "name": "private" },
        "token": "token-secret",
        "user_identifier": "user-identifier-secret",
        "device_uuid": "device-uuid-secret",
        "email": "private@example.com",
        "uuid": "generic-uuid-secret",
        "lat_e7": 312000000,
        "lng_e7": 1215000000,
        "file_path": "C:\\Users\\private\\secret.json",
        "nested": [{
            "device_id": "device-secret",
            "serial_number": "serial-secret",
            "record_id": "record-secret",
            "workout_id": "workout-secret",
            "sleep_id": "sleep-secret",
            "gps_route": [{ "lat_e7": 312000000, "lng_e7": 1215000000 }],
            "route": [{ "latitude": 31.2, "longitude": 121.5 }],
            "value": 42
        }]
    });
    let (redacted, redactions) = redact_ai_export(source.clone(), false).unwrap();
    let value: serde_json::Value = serde_json::from_str(&redacted).unwrap();
    assert!(value.get("user").is_none());
    assert!(value.get("token").is_none());
    assert!(value.get("user_identifier").is_none());
    assert!(value.get("device_uuid").is_none());
    assert!(value.get("email").is_none());
    assert!(value.get("uuid").is_none());
    assert!(value.get("lat_e7").is_none());
    assert!(value.get("lng_e7").is_none());
    assert!(value.get("file_path").is_none());
    assert!(value["nested"][0].get("device_id").is_none());
    assert!(value["nested"][0].get("serial_number").is_none());
    assert!(value["nested"][0].get("record_id").is_none());
    assert!(value["nested"][0].get("workout_id").is_none());
    assert!(value["nested"][0].get("sleep_id").is_none());
    assert!(value["nested"][0].get("gps_route").is_none());
    assert!(value["nested"][0].get("route").is_none());
    assert!(!value.to_string().contains("secret"));
    assert!(redactions
        .iter()
        .any(|item| item == "authentication_fields"));
    assert!(redactions.iter().any(|item| item == "identity_fields"));
    assert!(redactions.iter().any(|item| item == "precise_route"));
    assert!(redactions.iter().any(|item| item == "local_paths"));
    assert!(!redacted.contains("C:\\Users\\private"));
}

#[test]
fn ai_handoff_precise_route_requires_explicit_opt_in_but_keeps_identifiers_removed() {
    let source = json!({
        "device_id": "device-secret",
        "route": [{ "latitude": 31.2, "longitude": 121.5 }],
        "coordinates": { "lat": 31.2, "lon": 121.5 }
    });
    let (redacted, redactions) = redact_ai_export(source.clone(), true).unwrap();
    let value: serde_json::Value = serde_json::from_str(&redacted).unwrap();
    assert!(value.get("device_id").is_none());
    assert!(value.get("route").is_some());
    assert!(value.get("coordinates").is_some());
    assert!(!redactions.iter().any(|item| item == "precise_route"));
    assert!(value.to_string().contains("31.2"));
}

#[test]
fn ai_handoff_clipboard_sanitizes_local_paths_without_touching_urls() {
    let source = json!({
        "note": "C:\\Users\\private\\data.json /tmp/private/data.json https://example.com/path"
    });
    let (redacted, redactions) = redact_ai_export(source.clone(), false).unwrap();
    assert!(!redacted.contains("C:\\Users\\private"));
    assert!(!redacted.contains("/tmp/private"));
    assert!(redacted.contains("https://example.com/path"));
    assert!(redactions.iter().any(|item| item == "local_paths"));
}

#[test]
fn ai_handoff_inline_limit_includes_exact_two_mib_boundary() {
    assert_eq!(
        ai_handoff_mode_for_bytes(AI_HANDOFF_INLINE_LIMIT_BYTES),
        "inline"
    );
    assert_eq!(
        ai_handoff_mode_for_bytes(AI_HANDOFF_INLINE_LIMIT_BYTES + 1),
        "attachment"
    );
}

#[test]
fn opener_allowlist_is_eight_ai_providers_the_repo_and_the_map_credit() {
    let capability: serde_json::Value =
        serde_json::from_str(include_str!("../../../capabilities/default.json")).unwrap();
    let permissions = capability["permissions"].as_array().unwrap();
    let opener = permissions
        .iter()
        .find(|permission| permission["identifier"] == "opener:allow-open-url")
        .expect("opener allowlist");
    let urls = opener["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["url"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        urls,
        vec![
            "https://chatgpt.com/",
            "https://claude.ai/",
            "https://gemini.google.com/app",
            "https://www.kimi.com/",
            "https://www.doubao.com/chat/",
            "https://chat.deepseek.com/",
            "https://grok.com/",
            "https://chat.qwen.ai/",
            // The "Star" link in the Settings > "Feedback" card (FeedbackSection.vue).
            "https://github.com/lingcang728/ZeppBridge",
            // The attribution link of the online map in workout details (lib/mapTiles.ts).
            "https://www.openstreetmap.org/copyright",
        ]
    );
    assert!(urls.iter().all(|url| url.starts_with("https://")));
}

#[test]
fn a_user_assignment_overrides_even_a_confident_auto_match() {
    // This test comes from a real bug: the pick logic was inside
    // `if match_status == Unknown`, so for a watch already matched by a catalog alias,
    // the model did not change after the user clicked "Wrong, let me pick". The pick was saved, but the UI
    // always showed the wrong model. Automatic identification can be wrong, and the user's choice must win.
    let target = crate::device_catalog::catalog_entries()
        .iter()
        .find(|entry| entry.image_key.is_some())
        .expect("The bundled catalog must have at least one device with an image");
    let target_id = target.catalog_id.clone();
    let target_name = target.canonical_name.clone();

    let mut profile = DeviceProfile {
        canonical_name: Some("Model identified wrongly".into()),
        match_status: DeviceMatchStatus::Alias,
        ..Default::default()
    };
    assert!(apply_user_assignment(&mut profile, &target_id));
    assert_eq!(
        profile.canonical_name.as_deref(),
        Some(target_name.as_str())
    );
    // The source must be marked honestly as "user assigned", not shown as an identification result.
    assert_eq!(profile.match_status, DeviceMatchStatus::UserAssigned);

    // An id not in the catalog changes nothing. It does not reset the device to unknown.
    let mut untouched = DeviceProfile {
        canonical_name: Some("Kept as is".into()),
        match_status: DeviceMatchStatus::Exact,
        ..Default::default()
    };
    assert!(!apply_user_assignment(&mut untouched, "no-such-catalog-id"));
    assert_eq!(untouched.canonical_name.as_deref(), Some("Kept as is"));
    assert_eq!(untouched.match_status, DeviceMatchStatus::Exact);
}

/// From feedback: a user read devices.json and thought "the local data flag is wrong". Those two fields are computed
/// when the list is read and are not written to the cache. On read they get default values.
#[test]
fn the_device_cache_omits_fields_that_are_computed_on_read() {
    let suffix = zeppbridge_core::unique_suffix();
    let dir = std::env::temp_dir().join(format!("zeppbridge-device-cache-derived-{suffix}"));
    fs::create_dir_all(&dir).unwrap();
    let file = DeviceProfilesFile {
        version: 1,
        cached_at: chrono::Utc::now(),
        profiles: vec![super::DeviceProfile {
            name: Some("T-Rex".into()),
            device_id: Some("SN-1".into()),
            ..Default::default()
        }],
    };
    let encoded = encode_device_cache(&file).unwrap();
    let text = String::from_utf8(encoded.clone()).unwrap();
    assert!(!text.contains("has_local_data"), "{text}");
    assert!(!text.contains("last_data_at"), "{text}");
    fs::write(dir.join("devices.json"), encoded).unwrap();
    let cache = read_device_profile_cache(&dir);
    assert_eq!(cache.profiles.len(), 1);
    assert_eq!(cache.profiles[0].device_id.as_deref(), Some("SN-1"));
    assert!(!cache.profiles[0].has_local_data);
    let _ = fs::remove_dir_all(dir);
}
