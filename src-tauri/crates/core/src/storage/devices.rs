//! Device identity collection, profiles and data summaries (split out of storage/mod.rs by domain, logic unchanged).

use super::*;

pub(super) fn push_alias(aliases: &mut Vec<String>, value: Option<String>) {
    if let Some(value) = value {
        let trimmed = value.trim();
        if trimmed.is_empty() || looks_like_firmware_version(trimmed) {
            return;
        }
        if !aliases.iter().any(|existing| existing == trimmed) {
            aliases.push(trimmed.to_string());
        }
    }
}

/// Whether this value looks like a firmware version rather than a device identifier.
///
/// Cause: a user reported three "unrecognized data sources" in the sidebar that did nothing on click and could not be deleted. Their labels were
/// `0.91.20.5` and `0.91.17.5`. Those are not devices, they are firmware versions. Some Zepp payloads put such strings in
/// the `deviceId` / `sn` position, and `device_identity_hints` checked only
/// the field name, not the value. So each version number became a "device", and each firmware update added one more.
///
/// The rule comes from real shape differences in data, not guesses: real device identifiers in the local database are hex
/// MACs (`D8803CFFFEC19AC6`), numeric serials (`23229501001311`) or product codes
/// (`PRUC72 070007001c`). None contains a dot. The `firmware` column in the same table looks
/// like `0.116.137.19`, `0.132.139.2`, `V0.54.131.3`. So the shape "optional V/v prefix +
/// at least three numeric parts" matches only version numbers.
///
/// The rule is strict on purpose: better to miss an unseen fake device than to mark a real device whose vendor uses
/// dotted decimals as a serial as a ghost that can never come back.
pub fn looks_like_firmware_version(value: &str) -> bool {
    let body = value.strip_prefix(['V', 'v']).unwrap_or(value);
    let mut segments = 0usize;
    for segment in body.split('.') {
        if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        segments += 1;
    }
    segments >= 3
}

pub(super) fn string_field(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
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

pub(super) fn firmware_from_bind_device(raw: &str) -> Option<String> {
    raw.split(':')
        .next_back()
        .map(str::trim)
        .filter(|value| value.chars().any(|ch| ch.is_ascii_digit()) && value.contains('.'))
        .map(str::to_string)
}

pub(super) fn collect_objects<'a>(
    value: &'a serde_json::Value,
    out: &mut Vec<&'a serde_json::Value>,
) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                collect_objects(item, out);
            }
        }
        serde_json::Value::Object(object) => {
            out.push(value);
            for key in ["data", "items", "records", "results", "list", "summary"] {
                if let Some(child) = object.get(key) {
                    collect_objects(child, out);
                }
            }
        }
        _ => {}
    }
}

pub(super) fn device_identity_hints(payload: &serde_json::Value) -> Vec<DeviceIdentityHint> {
    let mut objects = Vec::new();
    collect_objects(payload, &mut objects);
    let mut hints = Vec::new();
    for object in objects {
        let mut aliases = Vec::new();
        push_alias(
            &mut aliases,
            string_field(object, &["device_id", "deviceId", "deviceid"]),
        );
        push_alias(
            &mut aliases,
            string_field(object, &["sn", "serial", "serialNumber"]),
        );
        if aliases.is_empty() {
            continue;
        }
        let bind = string_field(object, &["bind_device", "bindDevice"]);
        // `device_id` and `serial` must pass the same gate: `upsert_device_identity` also writes
        // these two values as aliases, and a check on `aliases` alone does not catch them.
        hints.push(DeviceIdentityHint {
            device_id: string_field(object, &["device_id", "deviceId", "deviceid"])
                .filter(|value| !looks_like_firmware_version(value)),
            serial: string_field(object, &["sn", "serial", "serialNumber"])
                .filter(|value| !looks_like_firmware_version(value)),
            firmware: bind.as_deref().and_then(firmware_from_bind_device),
            timezone: string_field(object, &["syncedTimezone", "timezone", "tz"]).filter(|value| {
                value.contains('/') || value.chars().any(|ch| ch.is_ascii_alphabetic())
            }),
            name: string_field(object, &["displayName", "deviceName", "productName"]),
            aliases,
        });
    }
    hints
}

impl Database {
    /// The IANA timezone the devices report, for endpoints that ask for a zone
    /// name rather than an offset.
    pub fn device_time_zone(&self) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT timezone FROM device_identities
                 WHERE timezone IS NOT NULL AND timezone <> ''
                 ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?)
    }

    /// How many retained `wellness` raw responses carry one of these labels.
    pub(super) fn count_wellness_raw(&self, labels: &[&str]) -> Result<i64> {
        let mut total = 0i64;
        for label in labels {
            let pattern = format!("wellness:{label}:%");
            total += self.conn.query_row(
                "SELECT COUNT(*) FROM raw_records WHERE stream = 'wellness' AND source_key LIKE ?1",
                [&pattern],
                |row| row.get::<_, i64>(0),
            )?;
        }
        Ok(total)
    }

    pub fn upsert_device_identity(&self, hint: &DeviceIdentityHint) -> Result<()> {
        let updated_at = Utc::now().to_rfc3339();
        let mut aliases = hint.aliases.clone();
        if let Some(device_id) = hint.device_id.as_ref() {
            aliases.push(device_id.clone());
        }
        if let Some(serial) = hint.serial.as_ref() {
            aliases.push(serial.clone());
        }
        aliases.retain(|value| !value.trim().is_empty());
        aliases.sort();
        aliases.dedup();
        for alias in aliases {
            self.conn.execute(
                "INSERT INTO device_identities
                    (alias, name, firmware, serial, device_id, timezone, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(alias) DO UPDATE SET
                    name = COALESCE(excluded.name, device_identities.name),
                    firmware = COALESCE(excluded.firmware, device_identities.firmware),
                    serial = COALESCE(excluded.serial, device_identities.serial),
                    device_id = COALESCE(excluded.device_id, device_identities.device_id),
                    timezone = COALESCE(excluded.timezone, device_identities.timezone),
                    updated_at = excluded.updated_at",
                params![
                    alias,
                    hint.name,
                    hint.firmware,
                    hint.serial,
                    hint.device_id,
                    hint.timezone,
                    updated_at,
                ],
            )?;
        }
        Ok(())
    }

    pub fn lookup_device_profile(&self, device_id: &str) -> Result<Option<DeviceProfile>> {
        let trimmed = device_id.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        self.conn
            .query_row(
                "SELECT name, firmware, serial, device_id, timezone
                 FROM device_identities WHERE lower(alias) = lower(?1) LIMIT 1",
                [trimmed],
                |row| {
                    Ok(DeviceProfile {
                        name: row.get(0)?,
                        firmware: row.get(1)?,
                        serial: row.get(2)?,
                        device_id: row.get(3)?,
                        timezone: row.get(4)?,
                        ..DeviceProfile::default()
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Derive local-data presence from normalized records without introducing
    /// a product-specific table. User-level fused records are deliberately
    /// excluded: they cannot be attributed to one physical device.
    /// For each device (by lowercase device_id), the time of its latest local data. Takes the maximum across four tables:
    /// samples, daily metrics, sleep and workouts.
    ///
    /// Before, we ran `lower(device_id) = lower(?)` once per device, per alias, per table:
    /// the function on the column blocked the index, so two devices meant a dozen full table scans. During that time we also
    /// held the command-side database lock, and the startup overview queries all waited behind it. Now each of the four tables is scanned once,
    /// and all devices share this table.
    pub fn device_latest_data_index(&self) -> Result<HashMap<String, String>> {
        let mut latest: HashMap<String, String> = HashMap::new();
        for (table, column) in [
            ("metric_samples", "timestamp"),
            ("daily_metrics", "date"),
            ("sleep_sessions", "start_time"),
            ("workouts", "start_time"),
        ] {
            let sql = format!(
                "SELECT lower(device_id), MAX({column}) FROM {table}
                 WHERE device_id IS NOT NULL AND lower(source_scope) = 'device'
                 GROUP BY lower(device_id)"
            );
            let mut statement = self.conn.prepare(&sql)?;
            let rows = statement.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?;
            for row in rows {
                let (device, value) = row?;
                let Some(value) = value else { continue };
                match latest.get_mut(&device) {
                    Some(current) if value.as_str() <= current.as_str() => {}
                    Some(current) => *current = value,
                    None => {
                        latest.insert(device, value);
                    }
                }
            }
        }
        Ok(latest)
    }

    /// Local data summary for one device: whether it has data, and the time of the latest row. Alias match is case-insensitive.
    pub fn device_data_summary(&self, aliases: &[String]) -> Result<(bool, Option<String>)> {
        if aliases.iter().all(|alias| alias.trim().is_empty()) {
            return Ok((false, None));
        }
        Ok(device_summary_from_index(
            &self.device_latest_data_index()?,
            aliases,
        ))
    }

    /// Device evidence for officially authorized accounts: built from the official workout list stored on this machine.
    ///
    /// The official open platform has no device list endpoint, and `/users/{id}/devices` accepts only the legacy
    /// apptoken. An account that uses only official authorization never gets a device list, so the settings page always shows
    /// "unrecognized device", even though each workout names the watch. Each item in the workout list has
    /// `source` (`run.10551555.huami.com`, the middle part is `deviceSource`) and
    /// `deviceName` (product name JSON by language). Here we dedupe by `deviceSource`,
    /// build entries in the device list shape and pass them to `parse_device_profiles`. We do not add a second set of match rules.
    ///
    /// A workout whose `source` middle part is not all digits (for example one recorded on a phone) does not count as a watch.
    /// The person's daily goals from the newest Zepp daily summary that carries them:
    /// `stepGoal`, `calorieGoal` and `burningDurationGoal` (active minutes). Missing goals are `None`.
    pub fn latest_daily_goals(&self) -> Result<serde_json::Value> {
        fn find(v: &serde_json::Value, key: &str) -> Option<i64> {
            match v {
                serde_json::Value::Object(m) => m
                    .get(key)
                    .and_then(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.parse().ok())))
                    .or_else(|| m.values().find_map(|c| find(c, key))),
                serde_json::Value::Array(a) => a.iter().find_map(|c| find(c, key)),
                _ => None,
            }
        }
        let mut statement = self.conn.prepare(
            "SELECT payload, payload_zip FROM raw_records WHERE stream = 'daily_summary' ORDER BY start_utc DESC LIMIT 30",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let Ok(payload) = decode_raw_payload(row.get(0)?, row.get(1)?) else { continue };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) else { continue };
            let steps = find(&value, "stepGoal");
            if steps.is_some() {
                return Ok(serde_json::json!({
                    "steps": steps,
                    "calories": find(&value, "calorieGoal"),
                    "activeMinutes": find(&value, "burningDurationGoal"),
                }));
            }
        }
        Ok(serde_json::json!({ "steps": null, "calories": null, "activeMinutes": null }))
    }

    pub fn official_workout_device_items(&self) -> Result<Vec<serde_json::Value>> {
        let mut statement = self.conn.prepare(
            "SELECT payload, payload_zip FROM raw_records
             WHERE stream = 'workouts' AND provider = 'official'
             ORDER BY start_utc DESC",
        )?;
        let mut rows = statement.query([])?;
        let mut seen = BTreeMap::<i64, Option<String>>::new();
        while let Some(row) = rows.next()? {
            let Ok(payload) = decode_raw_payload(row.get(0)?, row.get(1)?) else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<serde_json::Value>(&payload) else {
                continue;
            };
            let Some(items) = payload.get("items").and_then(|value| value.as_array()) else {
                continue;
            };
            for item in items {
                let Some(code) = item
                    .get("source")
                    .and_then(|value| value.as_str())
                    .and_then(official_source_device_code)
                else {
                    continue;
                };
                let name = item.get("deviceName").and_then(official_device_name);
                let entry = seen.entry(code).or_insert(None);
                if entry.is_none() {
                    *entry = name;
                }
            }
        }
        Ok(seen
            .into_iter()
            .map(|(code, name)| match name {
                Some(name) => serde_json::json!({ "deviceSource": code, "deviceName": name }),
                None => serde_json::json!({ "deviceSource": code }),
            })
            .collect())
    }

    pub(super) fn harvest_device_identities(&self, payload: &serde_json::Value) -> Result<()> {
        for hint in device_identity_hints(payload) {
            self.upsert_device_identity(&hint)?;
        }
        Ok(())
    }

    pub(super) fn get_latest_heart_rate_sample(&self) -> Result<Option<(i32, String)>> {
        let value: Option<(f64, String)> = self
            .conn
            .query_row(
                "SELECT value, timestamp FROM metric_samples
                 WHERE metric = 'heart_rate'
                 ORDER BY timestamp DESC,
                    CASE source_scope WHEN 'user_fused' THEN 0 WHEN 'device' THEN 1 ELSE 2 END,
                    id DESC LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        Ok(value.map(|(value, timestamp)| (value.round() as i32, timestamp)))
    }
}

/// Look up one device's summary in `Database::device_latest_data_index` by alias (case-insensitive).
pub fn device_summary_from_index(
    index: &HashMap<String, String>,
    aliases: &[String],
) -> (bool, Option<String>) {
    let latest = aliases
        .iter()
        .map(|alias| alias.trim())
        .filter(|alias| !alias.is_empty())
        .filter_map(|alias| index.get(&alias.to_lowercase()))
        .max()
        .cloned();
    (latest.is_some(), latest)
}

/// The `deviceSource` in an official workout `source`: `run.10551555.huami.com` -> `10551555`.
pub(super) fn official_source_device_code(source: &str) -> Option<i64> {
    let mut segments = source.trim().split('.');
    segments.next()?;
    let code = segments.next()?;
    if code.is_empty() || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    code.parse().ok().filter(|value| *value > 0)
}

/// An official workout's `deviceName` is a JSON string by language
/// (`{"en-WW":"Amazfit T-Rex 3 Pro (48mm)","zh-CN":...}`). Catalog aliases are in English,
/// so we take `en-WW` first. A value that is not JSON is used as the name as is.
pub(super) fn official_device_name(value: &serde_json::Value) -> Option<String> {
    let raw = value.as_str()?.trim();
    let name = match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Object(names)) => ["en-WW", "en-US", "en"]
            .iter()
            .find_map(|locale| names.get(*locale).and_then(|name| name.as_str()))
            .or_else(|| names.values().find_map(|name| name.as_str()))?
            .trim()
            .to_string(),
        _ => raw.to_string(),
    };
    (!name.is_empty()).then_some(name)
}
