//! JSON parsing for device profiles, and a structure summary for diagnostics (shape only, no values).

use super::*;

/// The `deviceSource` numbers in a device entry.
///
/// **Reads only `deviceSource`**. `deviceType` looks similar, but it is a family code: in the feedback data
/// the one value `deviceType:0` covers twenty watch models, so a catalog lookup with it gives wrong matches.
pub(super) fn device_source_numbers(item: &Value, extra: &Value) -> Vec<i64> {
    let mut out = Vec::new();
    for source in [item, extra] {
        let Some(object) = source.as_object() else {
            continue;
        };
        for key in ["deviceSource", "device_source"] {
            let Some(Value::Number(number)) = object.get(key) else {
                continue;
            };
            if let Some(value) = number.as_i64() {
                if value > 0 && !out.contains(&value) {
                    out.push(value);
                }
            }
        }
    }
    out
}

/// The "body fat scale" family code of `deviceType` in the Zepp device list.
pub(super) const SCALE_DEVICE_TYPE: i64 = 1;

/// The `deviceType` family code in a device entry. Use it only for the broad class (scale or not), never to identify a model.
pub(super) fn device_type_number(item: &Value, extra: &Value) -> Option<i64> {
    [item, extra].into_iter().find_map(|source| {
        let object = source.as_object()?;
        ["deviceType", "device_type"]
            .into_iter()
            .find_map(|key| match object.get(key)? {
                Value::Number(number) => number.as_i64(),
                Value::String(text) => text.trim().parse().ok(),
                _ => None,
            })
    })
}

pub(crate) fn parse_device_profiles(value: &serde_json::Value) -> Vec<DeviceProfile> {
    let items = device_items(value);
    items
        .into_iter()
        .map(|item| {
            let extra = flattened_device_metadata(&item);
            let display_name =
                first_string(&item, &["displayName", "deviceName", "nickname", "name"]).or_else(
                    || first_string(&extra, &["displayName", "deviceName", "nickname", "name"]),
                );
            let mut product_names = merged_string_values(
                &item,
                &extra,
                &["productName", "product_name", "modelName", "model"],
            );
            // `productId` / `hardwareVersion` are sometimes the internal
            // codename ("amazfit_balance2"), which normalizes to exactly the
            // same string as the catalog alias "Amazfit Balance 2". Alias
            // matching is equality on the normalized form, so a value that is
            // not a product name simply matches nothing.
            product_names.extend(merged_string_values(
                &item,
                &extra,
                &["productId", "product_id", "hardwareVersion"],
            ));
            product_names.sort();
            product_names.dedup();
            let mut model_codes = string_values(
                &item,
                &[
                    "modelCode",
                    "model_code",
                    "modelNumber",
                    "hardwareModel",
                    "productCode",
                ],
            );
            model_codes.extend(string_values(
                &extra,
                &[
                    "modelCode",
                    "model_code",
                    "modelNumber",
                    "hardwareModel",
                    "productCode",
                ],
            ));
            // Some accounts' device list carries no product-name field at all
            // (issue #4: nameFieldObjects = 0). The only model-class facts in
            // that payload are these numeric/short identifiers, so they have to
            // reach the matcher; otherwise the watch is unidentifiable by
            // construction no matter how complete the catalog gets.
            //
            // Feeding them in does not invent a mapping: the bundled catalog
            // still has to carry the value before anything matches.
            model_codes.extend(merged_string_values(
                &item,
                &extra,
                &[
                    "deviceSource",
                    "device_source",
                    "deviceType",
                    "device_type",
                    "productId",
                    "product_id",
                    "hardwareVersion",
                ],
            ));
            model_codes.sort();
            model_codes.dedup();
            // deviceSource takes a separate path and does not mix with the strings above.
            //
            // The list above also has `deviceType`, which is a family code: in the feedback data the one
            // value deviceType:0 covers twenty watch models. If both go into one list, the catalog cannot
            // accept deviceSource without also accepting deviceType by mistake.
            let device_source_codes = device_source_numbers(&item, &extra);
            let device_names = merged_string_values(&item, &extra, &["deviceName", "deviceType"]);
            let device_id = first_string(&item, &["deviceId", "device_id", "macAddress"])
                .or_else(|| first_string(&extra, &["deviceId", "device_id", "macAddress"]))
                // Keep the legacy source-only key, but never let a model code
                // hide an actual device identity in nested metadata.
                .or_else(|| first_string(&item, &["deviceSource"]));
            if let Some(device_id) = device_id.as_deref() {
                if device_id.starts_with('A')
                    && device_id.chars().skip(1).all(|c| c.is_ascii_digit())
                {
                    model_codes.push(device_id.to_string());
                }
            }
            let names = product_names.iter().map(String::as_str).collect::<Vec<_>>();
            let device_name_refs = device_names.iter().map(String::as_str).collect::<Vec<_>>();
            let model_code_refs = model_codes.iter().map(String::as_str).collect::<Vec<_>>();
            let matched = match_catalog(&CatalogMatchInput {
                device_source_codes,
                model_codes: model_code_refs,
                product_names: names,
                device_names: device_name_refs,
                display_name: display_name.as_deref(),
            });
            // A name must not be a firmware version.
            //
            // The device list of some accounts has **no name field at all** (feedback report
            // `404560ac` is one: its `nameFieldObjects` is 0), and
            // `product_names` picked up `hardwareVersion`, a value like `0.135.23.0`.
            // The sidebar then showed a data source named "0.91.20.5"
            // with nothing inside it (a Reddit report, and a Bip 6 user
            // in the feedback data wrote "Says 0.135.23.0").
            //
            // The check is `looks_like_firmware_version`. The write side (`device_identities`
            // in `storage/mod.rs`) already uses it. This path, from the cloud response directly to
            // the UI, was the one that missed it. Block only its use as a name: it stays in
            // `product_names` / `model_codes` for catalog matching. If it does not match there,
            // it does not match, and it does not make a false model.
            //
            // After the block the name is `None`. The frontend (`useDevices.ts`) then shows
            // "Unidentified device" and offers a way to pick the model manually.
            let named = |value: &String| !looks_like_firmware_version(value);
            let display_name = display_name.filter(|value| named(value));
            let mut profile = DeviceProfile {
                name: display_name
                    .clone()
                    .or_else(|| product_names.iter().find(|v| named(v)).cloned()),
                display_name,
                name_code: None,
                canonical_name: None,
                catalog_id: None,
                kind: None,
                image_key: None,
                match_status: DeviceMatchStatus::Unknown,
                has_local_data: false,
                last_data_at: None,
                hidden: false,
                firmware: first_string(
                    &extra,
                    &[
                        "productVersion",
                        "firmwareVersion",
                        "hardwareVersion",
                        "fwVersion",
                    ],
                ),
                serial: first_string(&extra, &["sn", "serial", "serialNumber"]),
                device_id,
                timezone: first_string(&extra, &["bind_timezone", "timezone", "tz"]).filter(
                    |value| value.contains('/') || value.chars().any(|ch| ch.is_ascii_alphabetic()),
                ),
            };
            if let Some(matched) = matched {
                apply_catalog_match(&mut profile, matched.entry, matched.status);
            } else if device_type_number(&item, &extra) == Some(SCALE_DEVICE_TYPE) {
                // A body fat scale with an unknown model: mark only the class, do not guess the model.
                //
                // `deviceType` is a family code (0 = watch / band, 1 = body fat scale, 7 = earbuds). Use it
                // **only** for the broad class. In the feedback data the low codes 101-104 all have `deviceType:1`,
                // and users picked several different watches for them in the picker. They are really the scale row.
                // With this, the UI shows "Scale" and not "Unidentified device", and does not ask the user to pick a model.
                profile.kind = Some("scale".into());
            }
            profile
        })
        .filter(|profile| {
            profile.device_id.is_some() || profile.serial.is_some() || profile.name.is_some()
        })
        .fold(Vec::new(), |mut kept: Vec<DeviceProfile>, profile| {
            // The same watch must not show twice in the sidebar.
            //
            // The cloud list gives one row for each binding of the same device (new phone, unbind and rebind).
            // When the names were different, this was hard to see. Now every unknown device is "Unidentified device",
            // so duplicates are obvious. Merge only rows with **exactly the same identifiers**: two really different
            // watches with the same name must not merge, or the user loses a device.
            let duplicate = kept.iter().any(|existing| {
                existing.device_id == profile.device_id
                    && existing.serial == profile.serial
                    && (existing.device_id.is_some() || existing.serial.is_some())
            });
            if !duplicate {
                kept.push(profile);
            }
            kept
        })
}

pub(super) fn merge_device_metadata(target: &mut Map<String, Value>, value: &Value, depth: usize) {
    if depth > 6 {
        return;
    }
    let decoded;
    let value = if let Some(raw) = value.as_str() {
        decoded = serde_json::from_str::<Value>(raw).ok();
        decoded.as_ref().unwrap_or(value)
    } else {
        value
    };
    let Some(object) = value.as_object() else {
        return;
    };
    for (key, child) in object {
        let entry = target.entry(key.clone()).or_insert(Value::Null);
        if entry.is_null() || entry.as_str().is_some_and(|text| text.trim().is_empty()) {
            *entry = child.clone();
        }
        if matches!(
            key.as_str(),
            "additionalInfo" | "bind_device" | "bindDevice" | "deviceInfo" | "device_info"
        ) {
            merge_device_metadata(target, child, depth + 1);
        }
    }
}

pub(super) fn flattened_device_metadata(item: &Value) -> Value {
    let mut merged = Map::new();
    merge_device_metadata(&mut merged, item, 0);
    Value::Object(merged)
}

pub(super) fn device_items(value: &serde_json::Value) -> Vec<serde_json::Value> {
    if let Some(array) = value.as_array() {
        return array.clone();
    }
    if let Some(object) = value.as_object() {
        for key in ["items", "devices", "list", "results", "data"] {
            if let Some(child) = object.get(key) {
                let items = device_items(child);
                if !items.is_empty() {
                    return items;
                }
            }
        }
    }
    vec![value.clone()]
}

pub(super) fn string_values(value: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    let mut values = Vec::new();
    for key in keys {
        match object.get(*key) {
            Some(serde_json::Value::String(text)) if !text.trim().is_empty() => {
                values.push(text.trim().to_string());
            }
            Some(serde_json::Value::Number(number)) => values.push(number.to_string()),
            Some(serde_json::Value::Array(items)) => {
                values.extend(items.iter().filter_map(|item| match item {
                    serde_json::Value::String(text) if !text.trim().is_empty() => {
                        Some(text.trim().to_string())
                    }
                    serde_json::Value::Number(number) => Some(number.to_string()),
                    _ => None,
                }));
            }
            _ => {}
        }
    }
    values.sort();
    values.dedup();
    values
}

pub(super) fn merged_string_values(
    primary: &serde_json::Value,
    secondary: &serde_json::Value,
    keys: &[&str],
) -> Vec<String> {
    let mut values = string_values(primary, keys);
    values.extend(string_values(secondary, keys));
    values.sort();
    values.dedup();
    values
}

pub(super) fn first_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        match object.get(*key) {
            Some(serde_json::Value::String(text)) if !text.trim().is_empty() => {
                return Some(text.trim().to_string());
            }
            Some(serde_json::Value::Number(number)) => return Some(number.to_string()),
            _ => {}
        }
    }
    None
}
