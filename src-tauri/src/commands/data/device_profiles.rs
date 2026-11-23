//! Device profile commands: cloud refresh, local cache, catalog matching, and local data enrichment.

use super::*;

#[tauri::command]
pub async fn get_device_profile(
    state: tauri::State<'_, AppState>,
    device_id: Option<String>,
    source_scope: Option<String>,
) -> std::result::Result<DeviceProfile, AppError> {
    resolve_device_profile(&state, device_id.as_deref(), source_scope.as_deref()).await
}

/// Return every device bound to the current account. The command is
/// cache-first; a caller opts into the bounded network refresh explicitly so
/// an offline account can still inspect its last known device list.
#[tauri::command]
pub async fn get_device_profiles(
    state: tauri::State<'_, AppState>,
    refresh: Option<bool>,
) -> std::result::Result<DeviceProfilesResult, AppError> {
    let cached = read_device_profile_cache(&state.data_dir);
    let mut profiles = cached.profiles;
    let mut cached_at = cached.cached_at;
    let mut refreshed = false;
    let mut refresh_error = None;
    let mut refresh_error_code = None;

    if refresh.unwrap_or(false) {
        match refresh_device_profiles_from_cloud(&state).await {
            Ok((remote_profiles, fetched_at)) => {
                profiles = remote_profiles;
                cached_at = Some(fetched_at);
                refreshed = true;
            }
            Err(error) => {
                // Keep the last good cache and expose a safe, non-secret error
                // string plus its stable code for the settings surface.
                refresh_error = Some(error.message.clone());
                refresh_error_code = Some(error.code.clone());
            }
        }
    }

    // If local data enrichment ("has local data", "latest record") cannot read the database, do not fail the whole list.
    // In user feedback, "Re-identify devices" at once reported "Local database is not available", but the cloud step
    // had worked and the cache was written. Because of that, the settings page showed no devices at all.
    profiles = match enrich_profiles_with_local_data(&state, profiles.clone()).await {
        Ok(enriched) => enriched,
        Err(error) => {
            crate::diagnostics::log(&format!(
                "Local data enrichment for the device list failed. Showing the cloud list for now: {}",
                error.message
            ));
            profiles
        }
    };
    let hidden_keys = spawn_independent_read(state.data_dir.clone(), |db| db.hidden_device_keys())
        .await
        .unwrap_or_default();
    let (hidden_profiles, profiles) = split_hidden_profiles(profiles, &hidden_keys);
    let now = Utc::now();
    let age_seconds = cached_at.map(|value| (now - value).num_seconds().max(0));
    let status = if refreshed {
        "fresh"
    } else if refresh_error.is_some() {
        if cached_at.is_some() {
            "refresh_failed"
        } else {
            "unavailable"
        }
    } else if cached_at.is_none() {
        "missing"
    } else if age_seconds.unwrap_or(i64::MAX) > DEVICE_CACHE_MAX_AGE_SECONDS {
        "stale"
    } else {
        "fresh"
    };

    Ok(DeviceProfilesResult {
        profiles,
        hidden_profiles,
        cache: DeviceCacheMetadata {
            status: status.to_string(),
            cached_at,
            age_seconds,
            refreshed,
            refresh_error,
            refresh_error_code,
        },
    })
}

/// Picks the hidden devices out of the list and marks them `hidden`. Returns (hidden, shown).
pub(super) fn split_hidden_profiles(
    profiles: Vec<DeviceProfile>,
    hidden_keys: &std::collections::HashSet<String>,
) -> (Vec<DeviceProfile>, Vec<DeviceProfile>) {
    let (mut hidden, visible): (Vec<_>, Vec<_>) = profiles.into_iter().partition(|profile| {
        is_hidden_device(
            hidden_keys,
            profile.device_id.as_deref(),
            profile.serial.as_deref(),
        )
    });
    for profile in &mut hidden {
        profile.hidden = true;
    }
    (hidden, visible)
}

/// Hides or restores a device (hides it in the list only; the data stays).
#[tauri::command]
pub async fn set_device_hidden(
    state: tauri::State<'_, AppState>,
    device_key: String,
    hidden: bool,
) -> std::result::Result<(), AppError> {
    with_write(&state.data_dir, &state.db, WritePurpose::Metadata, |db| {
        db.set_device_hidden(&device_key, hidden)
    })
    .await
}

pub(crate) async fn refresh_device_profile(state: &AppState) {
    let _ = refresh_device_profiles_from_cloud(state).await;
}

pub(super) async fn refresh_device_profiles_from_cloud(
    state: &AppState,
) -> std::result::Result<(Vec<DeviceProfile>, DateTime<Utc>), AppError> {
    let profiles = match state.auth.load_auth().map_err(AppError::from)? {
        Some(auth) => {
            let connector = ZeppConnector::new(auth).map_err(AppError::from)?;
            let payload = connector.fetch_devices().await.map_err(AppError::from)?;
            parse_device_profiles(&payload)
        }
        // With only official sign-in there is no apptoken, so there is no device list endpoint to call. Before,
        // this returned "not configured", the device list was always empty, and the settings page always showed
        // "Unidentified device", even when each workout named the watch. Now it falls back to
        // `deviceSource` / `deviceName` from the local official workouts and uses the same catalog matching.
        None => {
            let items = spawn_independent_read(state.data_dir.clone(), |db| {
                db.official_workout_device_items()
            })
            .await?;
            if items.is_empty() {
                return Err(AppError::new(
                    "err.sync.not_connected",
                    "Zepp authentication is not configured",
                ));
            }
            parse_device_profiles(&Value::Array(items))
        }
    };
    if profiles.is_empty() {
        return Err(AppError::new("err.core.unavailable", "Zepp returned no devices"));
    }

    // This database write used to take only the in-process `state.db` mutex, not the cross-process write lock.
    // While replay, compaction or sync held the write lock, each autocommit upsert here waited in the
    // SQLite busy_timeout. Meanwhile `state.db` stayed locked, so all later commands
    // (status, overview, settings page load) queued behind it, and the UI looked like "clicks do nothing".
    // During a maintenance window, skip it: the catalog cache file is still written, and the next refresh fills the identity rows.
    let write_guard =
        if crate::storage::replay_in_progress() || crate::storage::compaction_in_progress() {
            None
        } else {
            match zeppbridge_core::storage::write_lock::try_acquire(
                &state.data_dir,
                WritePurpose::Metadata,
            ) {
                Ok(guard) => Some(guard),
                Err(error @ zeppbridge_core::storage::write_lock::WriteLockError::Busy { .. }) => {
                    crate::diagnostics::log(&format!("Device identity table not updated this time: {error}"));
                    None
                }
                Err(error) => {
                    return Err(AppError::new(
                        "err.core.io",
                        format!("Could not get the write lock: {error}"),
                    ));
                }
            }
        };
    if write_guard.is_some() {
        let db = state.db.lock().await;
        for hint in profiles.iter().map(device_hint_from_profile) {
            db.upsert_device_identity(&hint).map_err(AppError::from)?;
        }
    }

    let cached_at = Utc::now();
    let cache_file = DeviceProfilesFile {
        version: 1,
        cached_at,
        profiles: profiles.clone(),
    };
    let encoded = encode_device_cache(&cache_file).map_err(|error| {
        AppError::new("err.core.parse", format!("Could not encode the device catalog cache: {error}"))
    })?;
    write_file_atomically(&state.data_dir.join("devices.json"), &encoded)
        .map_err(|error| AppError::new("err.core.io", format!("Could not write the device catalog cache: {error}")))?;
    Ok((profiles, cached_at))
}

pub(super) async fn resolve_device_profile(
    state: &AppState,
    device_id: Option<&str>,
    source_scope: Option<&str>,
) -> std::result::Result<DeviceProfile, AppError> {
    if source_scope
        .map(|scope| scope.eq_ignore_ascii_case("user_fused"))
        .unwrap_or(false)
    {
        return Ok(DeviceProfile {
            name_code: Some(FUSED_CODE.into()),
            match_status: DeviceMatchStatus::Unknown,
            ..DeviceProfile::default()
        });
    }
    if source_scope
        .map(|scope| scope.eq_ignore_ascii_case("unknown"))
        .unwrap_or(false)
    {
        return Ok(device_id
            .map(unknown_device_profile)
            .unwrap_or_else(|| unknown_device_profile("")));
    }
    let Some(device_id) = device_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(DeviceProfile {
            name_code: Some(UNDETERMINED_CODE.into()),
            match_status: DeviceMatchStatus::Unknown,
            ..DeviceProfile::default()
        });
    };
    let from_db = {
        let device_id = device_id.to_string();
        spawn_independent_read(state.data_dir.clone(), move |db| {
            db.lookup_device_profile(&device_id)
        })
        .await?
    };
    let cached_profile = read_device_profile_cache(&state.data_dir)
        .profiles
        .into_iter()
        .find(|profile| profile_matches(profile, device_id));
    if let Some(profile) = from_db {
        let profile = if let Some(cached) = cached_profile {
            merge_cached_device_profile(profile, cached)
        } else {
            profile
        };
        return enrich_profile_with_local_data(state, profile, Some(device_id), None).await;
    }
    if let Some(profile) = cached_profile {
        return enrich_profile_with_local_data(state, profile, Some(device_id), None).await;
    }
    Ok(unknown_device_profile(device_id))
}

/// Writes `devices.json`: the cache of the cloud device list.
///
/// `has_local_data` / `last_data_at` are computed from the local database each time the list is read
/// (`enrich_profiles_with_local_data`), so in the cache these two are always false / null.
/// A user once read this file and thought "the local data flag is wrong". So they are not written.
/// On read they get default values and are computed again as before.
pub(super) fn encode_device_cache(file: &DeviceProfilesFile) -> serde_json::Result<Vec<u8>> {
    let mut value = serde_json::to_value(file)?;
    if let Some(profiles) = value.get_mut("profiles").and_then(Value::as_array_mut) {
        for profile in profiles.iter_mut().filter_map(Value::as_object_mut) {
            profile.remove("has_local_data");
            profile.remove("last_data_at");
        }
    }
    serde_json::to_vec_pretty(&value)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct DeviceProfilesFile {
    #[serde(default = "default_device_cache_version")]
    pub(super) version: u32,
    pub(super) cached_at: DateTime<Utc>,
    pub(super) profiles: Vec<DeviceProfile>,
}

pub(super) fn default_device_cache_version() -> u32 {
    1
}

#[derive(Debug, Default)]
pub(super) struct CachedDeviceProfiles {
    pub(super) profiles: Vec<DeviceProfile>,
    pub(super) cached_at: Option<DateTime<Utc>>,
}

pub(super) fn read_device_profile_cache(data_dir: &std::path::Path) -> CachedDeviceProfiles {
    let path = data_dir.join("devices.json");
    let raw = std::fs::read_to_string(path).ok();
    if let Some(raw) = raw {
        if let Ok(file) = serde_json::from_str::<DeviceProfilesFile>(&raw) {
            return CachedDeviceProfiles {
                profiles: file.profiles,
                cached_at: Some(file.cached_at),
            };
        }
        if let Ok(list) = serde_json::from_str::<Vec<DeviceProfile>>(&raw) {
            return CachedDeviceProfiles {
                profiles: list,
                cached_at: modified_at(&data_dir.join("devices.json")),
            };
        }
        if let Ok(single) = serde_json::from_str::<DeviceProfile>(&raw) {
            return CachedDeviceProfiles {
                profiles: vec![single],
                cached_at: modified_at(&data_dir.join("devices.json")),
            };
        }
    }
    let legacy = data_dir.join("device.json");
    std::fs::read_to_string(&legacy)
        .ok()
        .and_then(|raw| serde_json::from_str::<DeviceProfile>(&raw).ok())
        .map(|profile| CachedDeviceProfiles {
            profiles: vec![profile],
            cached_at: modified_at(&legacy),
        })
        .unwrap_or_default()
}

pub(super) fn modified_at(path: &Path) -> Option<DateTime<Utc>> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(DateTime::<Utc>::from(modified))
}

pub(super) fn profile_matches(profile: &DeviceProfile, needle: &str) -> bool {
    [&profile.device_id, &profile.serial]
        .into_iter()
        .flatten()
        .any(|value| value.eq_ignore_ascii_case(needle))
}

/// SQLite's identity index intentionally stores only stable lookup fields and
/// the user-facing name. Keep that nickname as the primary value while
/// recovering the versioned catalog fields from the richer devices.json cache
/// after an application restart.
pub(super) fn merge_cached_device_profile(
    mut indexed: DeviceProfile,
    cached: DeviceProfile,
) -> DeviceProfile {
    if indexed.display_name.is_none() {
        indexed.display_name = cached.display_name;
    }
    if indexed.canonical_name.is_none() {
        indexed.canonical_name = cached.canonical_name;
    }
    if indexed.catalog_id.is_none() {
        indexed.catalog_id = cached.catalog_id;
    }
    if indexed.kind.is_none() {
        indexed.kind = cached.kind;
    }
    if indexed.image_key.is_none() {
        indexed.image_key = cached.image_key;
    }
    if indexed.match_status == DeviceMatchStatus::Unknown {
        indexed.match_status = cached.match_status;
    }
    if indexed.firmware.is_none() {
        indexed.firmware = cached.firmware;
    }
    if indexed.serial.is_none() {
        indexed.serial = cached.serial;
    }
    if indexed.device_id.is_none() {
        indexed.device_id = cached.device_id;
    }
    if indexed.timezone.is_none() {
        indexed.timezone = cached.timezone;
    }
    indexed
}

pub(super) fn device_hint_from_profile(
    profile: &DeviceProfile,
) -> crate::models::DeviceIdentityHint {
    let mut aliases = Vec::new();
    if let Some(device_id) = &profile.device_id {
        aliases.push(device_id.clone());
    }
    if let Some(serial) = &profile.serial {
        aliases.push(serial.clone());
    }
    crate::models::DeviceIdentityHint {
        aliases,
        name: profile.name.clone(),
        firmware: profile.firmware.clone(),
        serial: profile.serial.clone(),
        device_id: profile.device_id.clone(),
        timezone: profile.timezone.clone(),
    }
}

/// A record the cloud merges across devices: no specific watch. The UI makes the label from the code; the backend gives no display name.
pub(super) const FUSED_CODE: &str = "ui.device.fused";
/// The record does not say which watch made it (phone-recorded, or an old response with missing fields).
pub(super) const UNDETERMINED_CODE: &str = "ui.device.undetermined";

pub(super) fn unknown_device_profile(device_id: &str) -> DeviceProfile {
    let device_id = device_id.trim();
    DeviceProfile {
        name_code: Some(UNDETERMINED_CODE.into()),
        device_id: (!device_id.is_empty()).then(|| device_id.to_string()),
        match_status: DeviceMatchStatus::Unknown,
        ..DeviceProfile::default()
    }
}

pub(super) async fn enrich_profiles_with_local_data(
    state: &AppState,
    profiles: Vec<DeviceProfile>,
) -> std::result::Result<Vec<DeviceProfile>, AppError> {
    // Query "latest record" once for all devices (one scan of each of the four tables), not once for each device.
    let index =
        spawn_independent_read(state.data_dir.clone(), |db| db.device_latest_data_index()).await?;
    let mut enriched = Vec::with_capacity(profiles.len());
    for profile in profiles {
        enriched.push(enrich_profile_with_local_data(state, profile, None, Some(&index)).await?);
    }
    Ok(enriched)
}

pub(super) async fn enrich_profile_with_local_data(
    state: &AppState,
    mut profile: DeviceProfile,
    requested_device_id: Option<&str>,
    latest_index: Option<&std::collections::HashMap<String, String>>,
) -> std::result::Result<DeviceProfile, AppError> {
    if profile.display_name.is_none() {
        profile.display_name = profile.name.clone();
    }
    if profile.match_status == DeviceMatchStatus::Unknown {
        let model_codes = profile.device_id.as_deref().into_iter().collect::<Vec<_>>();
        let names = profile.name.as_deref().into_iter().collect::<Vec<_>>();
        let display_name = profile.display_name.as_deref();
        if let Some(matched) = match_catalog(&CatalogMatchInput {
            // This path recovers from a profile already stored locally. It has only device_id,
            // no raw device response, so no deviceSource number is available.
            device_source_codes: Vec::new(),
            model_codes,
            product_names: names.clone(),
            device_names: names,
            display_name,
        }) {
            apply_catalog_match(&mut profile, matched.entry, matched.status);
        }
    }
    // If the user picked a model, use the user's choice.
    //
    // This used to have an `if match_status == Unknown` condition, meaning "use the user's choice only when
    // automatic identification fails". That assumes automatic identification is never wrong, but it **is** wrong
    // sometimes: the catalog matches by alias, so a watch with a clashing alias is identified as another model. After the user
    // clicked "Wrong, let me pick", the choice was saved but never shown, and the UI still showed the wrong model.
    //
    // Now the user's choice always wins over automatic identification, and it is marked honestly as
    // `UserAssigned`, not disguised as an identification result.
    {
        let keys = [
            requested_device_id,
            profile.device_id.as_deref(),
            profile.serial.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        let assigned = {
            let db = state.db.lock().await;
            db.device_model_override(&keys)?
        };
        if let Some(assigned) = assigned {
            apply_user_assignment(&mut profile, &assigned.catalog_id);
        }
    }
    let aliases = [
        requested_device_id,
        profile.device_id.as_deref(),
        profile.serial.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::to_string)
    .collect::<Vec<_>>();
    let (has_local_data, last_data_at) = match latest_index {
        Some(index) => zeppbridge_core::storage::device_summary_from_index(index, &aliases),
        None => {
            let aliases = aliases.clone();
            spawn_independent_read(state.data_dir.clone(), move |db| {
                db.device_data_summary(&aliases)
            })
            .await?
        }
    };
    profile.has_local_data = has_local_data;
    profile.last_data_at = last_data_at;
    // A single profile is still returned (the sleep page and workout details need its name and image). It is only marked hidden or not.
    if requested_device_id.is_some() {
        let hidden_keys =
            spawn_independent_read(state.data_dir.clone(), |db| db.hidden_device_keys()).await?;
        profile.hidden = is_hidden_device(
            &hidden_keys,
            profile.device_id.as_deref(),
            profile.serial.as_deref(),
        ) || requested_device_id
            .is_some_and(|key| is_hidden_device(&hidden_keys, Some(key), None));
    }
    Ok(profile)
}

/// Applies the model the user picked to this profile.
///
/// This is a separate function because of a past bug: this logic was inside an
/// `if match_status == Unknown`, so a device that automatic identification had matched could never take
/// the user's correction. As a function, a test can lock in "the user's choice wins, whatever the identification".
///
/// Returns whether it applied. If the catalog has no such id, nothing changes (it does not reset to unknown).
pub(super) fn apply_user_assignment(profile: &mut DeviceProfile, catalog_id: &str) -> bool {
    let Some(entry) = crate::device_catalog::catalog_entries()
        .iter()
        .find(|entry| entry.catalog_id == catalog_id)
    else {
        return false;
    };
    apply_catalog_match(profile, entry, CatalogMatchStatus::Exact);
    // Mark the source honestly: the user picked this, it is not an identification result.
    profile.match_status = DeviceMatchStatus::UserAssigned;
    true
}

pub(super) fn apply_catalog_match(
    profile: &mut DeviceProfile,
    entry: &crate::device_catalog::CatalogEntry,
    status: CatalogMatchStatus,
) {
    profile.canonical_name = Some(entry.canonical_name.clone());
    profile.catalog_id = Some(
        entry
            .canonical_device_key
            .clone()
            .unwrap_or_else(|| entry.catalog_id.clone()),
    );
    profile.kind = Some(entry.kind.clone());
    profile.image_key = entry.image_key.clone();
    profile.match_status = match status {
        CatalogMatchStatus::Exact => DeviceMatchStatus::Exact,
        CatalogMatchStatus::Alias => DeviceMatchStatus::Alias,
    };
    if profile.display_name.is_none() {
        profile.display_name = profile
            .name
            .clone()
            .or_else(|| Some(entry.canonical_name.clone()));
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn parse_device_profile(value: &serde_json::Value) -> DeviceProfile {
    parse_device_profiles(value)
        .into_iter()
        .next()
        .unwrap_or_default()
}
