//! The user's local correction layer.
//!
//! Three layers stay strictly separate, and none overwrites another:
//!
//! 1. **Zepp's raw facts**: payloads in `raw_records`. Never changed.
//! 2. **ZeppBridge's interpretation**: the `workout_type` the normalizer derives from the raw facts.
//! 3. **The user's local corrections**: this layer.
//!
//! Corrections stay on this machine. They do not go back to the cloud and do not take part in normalizer replay: replay recomputes layer 2,
//! but layer 3 stays as is. The UI must mark a correction as "entered by you" and must not show it as a recognition result.
//!
//! There are two kinds of correction. Both come from real user feedback (issue #3 / #4):
//!
//! * **Workout code alias**: Zepp's custom workout templates give codes that are not in the catalog (for example 226).
//!   We have no evidence of what sport 226 is, so we do not guess. The user names the code once,
//!   and all records with that code use the name.
//! * **Device model assignment**: some accounts' device responses have no product name field at all, only
//!   numbers such as `deviceSource` / `deviceType` / `productId`, so this machine cannot infer the model.
//!   Instead of "unrecognized device", the user picks the model once from the bundled catalog.

use super::Database;
use crate::models::{error::Result, ZeppBridgeError};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;

/// The name a user gives to a Zepp workout code.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkoutCodeLabel {
    pub zepp_type: i32,
    pub label: String,
    /// How many local records use this code. The user sees how much one change affects.
    pub records: i64,
    pub updated_at: String,
}

/// The model a user assigns to a device.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceModelOverride {
    /// The key of the device the user assigned: device_id or serial number.
    pub device_key: String,
    /// The `catalog_id` in the bundled device catalog.
    pub catalog_id: String,
    pub updated_at: String,
}

/// Maximum length of a custom name. Long enough for "Squash" or "My core workout", but short enough not to break the UI
/// or turn into a place for arbitrary text.
pub const MAX_CODE_LABEL_CHARS: usize = 24;

impl Database {
    /// The user's type correction for one workout record.
    ///
    /// Allowed values are **all keys in the bundled sport catalog**, not a short hardcoded list: the catalog has
    /// 131 sports, and with only 15 allowed, a user could not even change a record to "Squash".
    pub fn set_workout_type_override(
        &self,
        workout_id: &str,
        user_override: Option<&str>,
    ) -> Result<()> {
        self.edit_workouts(&[workout_id.to_owned()], Some(user_override), None)?;
        Ok(())
    }

    /// Name an unrecognized Zepp workout code. Pass `None` to undo.
    ///
    /// Accepts only **codes not in the catalog**. A code the catalog knows means we have evidence. To change
    /// one record, the user goes through `set_workout_type_override`. An override here would let a
    /// misspelled name silently replace a verified mapping.
    pub fn set_workout_code_label(&self, zepp_type: i32, label: Option<&str>) -> Result<()> {
        if crate::sport_catalog::resolve(zepp_type as i64).is_some() {
            return Err(ZeppBridgeError::ConfigError(
                "This code is already recognized. Use the type correction for a single record instead.".into(),
            ));
        }
        let Some(label) = label.map(str::trim).filter(|value| !value.is_empty()) else {
            self.conn.execute(
                "DELETE FROM workout_code_labels WHERE zepp_type = ?1",
                params![zepp_type],
            )?;
            return Ok(());
        };
        if label.chars().count() > MAX_CODE_LABEL_CHARS {
            return Err(ZeppBridgeError::ConfigError(format!(
                "A custom workout name can have at most {MAX_CODE_LABEL_CHARS} characters"
            )));
        }
        if label.chars().any(|c| c.is_control()) {
            return Err(ZeppBridgeError::ConfigError(
                "A custom workout name must not contain control characters".into(),
            ));
        }
        self.conn.execute(
            "INSERT INTO workout_code_labels(zepp_type, label, updated_at)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(zepp_type) DO UPDATE SET
                 label = excluded.label,
                 updated_at = excluded.updated_at",
            params![zepp_type, label, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Code -> user name. The query path reads it once, then applies it to records in memory.
    pub fn workout_code_label_map(&self) -> Result<HashMap<i32, String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT zepp_type, label FROM workout_code_labels")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut map = HashMap::new();
        for row in rows {
            let (code, label) = row?;
            map.insert(code, label);
        }
        Ok(map)
    }

    /// All unrecognized codes on this machine, with the names the user already gave and the number of affected records.
    ///
    /// The UI uses it for the "these codes have no name yet" list, so the user fixes them all at once instead of one by one.
    pub fn unknown_workout_code_labels(&self) -> Result<Vec<WorkoutCodeLabel>> {
        let labels = self.workout_code_label_map()?;
        let mut stmt = self.conn.prepare(
            "SELECT zepp_type, COUNT(*) FROM workouts
             WHERE zepp_type IS NOT NULL AND workout_type LIKE 'unknown:%'
             GROUP BY zepp_type ORDER BY COUNT(*) DESC, zepp_type ASC",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, i32>(0)?, row.get::<_, i64>(1)?)))?;
        let mut out = Vec::new();
        for row in rows {
            let (code, records) = row?;
            let updated_at = self
                .conn
                .query_row(
                    "SELECT updated_at FROM workout_code_labels WHERE zepp_type = ?1",
                    params![code],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .unwrap_or_default();
            out.push(WorkoutCodeLabel {
                zepp_type: code,
                label: labels.get(&code).cloned().unwrap_or_default(),
                records,
                updated_at,
            });
        }
        Ok(out)
    }

    /// The user assigns a model to a device. Pass `None` to undo.
    ///
    /// `catalog_id` must exist in the bundled catalog. Otherwise the UI would show a product that does not exist.
    pub fn set_device_model_override(
        &self,
        device_key: &str,
        catalog_id: Option<&str>,
    ) -> Result<()> {
        let device_key = device_key.trim();
        if device_key.is_empty() {
            return Err(ZeppBridgeError::ConfigError("The device identifier must not be empty".into()));
        }
        let catalog_id = catalog_id.map(str::trim).filter(|value| !value.is_empty());
        if catalog_id.is_some_and(|catalog_id| {
            !crate::device_catalog::catalog_entries()
                .iter()
                .any(|entry| entry.catalog_id == catalog_id)
        }) {
            return Err(ZeppBridgeError::ConfigError(
                "This model is not in the bundled device catalog".into(),
            ));
        }
        let previous = self.device_model_override(&[device_key])?;
        if previous.as_ref().map(|value| value.catalog_id.as_str()) == catalog_id {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        // Device keys are looked up case-insensitively; changing capitalization
        // must not leave two competing identifications behind.
        self.conn.execute(
            "DELETE FROM device_model_overrides WHERE lower(device_key) = lower(?1)",
            [device_key],
        )?;
        if let Some(catalog_id) = catalog_id {
            self.conn.execute(
                "INSERT INTO device_model_overrides(device_key, catalog_id, updated_at)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(device_key) DO UPDATE SET
                 catalog_id = excluded.catalog_id,
                 updated_at = excluded.updated_at",
                params![device_key, catalog_id, Utc::now().to_rfc3339()],
            )?;
        }
        // Type 15 depends on device context. Previously stamped summaries must
        // be replayed after identifying or clearing a device, including official
        // sports summaries. Local workout edits live outside the replay stamps.
        for table in ["raw_normalization", "raw_quarantine"] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE raw_record_id IN (SELECT id FROM raw_records WHERE stream = 'workouts')"),
                [],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Check whether the user assigned a model to this device. Tries each key the user may have used, in order.
    pub fn device_model_override(&self, keys: &[&str]) -> Result<Option<DeviceModelOverride>> {
        for key in keys {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            let found = self
                .conn
                .query_row(
                    "SELECT device_key, catalog_id, updated_at FROM device_model_overrides
                     WHERE lower(device_key) = lower(?1)",
                    params![key],
                    |row| {
                        Ok(DeviceModelOverride {
                            device_key: row.get(0)?,
                            catalog_id: row.get(1)?,
                            updated_at: row.get(2)?,
                        })
                    },
                )
                .optional()?;
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::in_memory().unwrap()
    }

    #[test]
    fn code_labels_only_apply_to_codes_the_catalog_cannot_resolve() {
        let db = db();
        // 226 is a custom workout template code a real user reported. It is not in the catalog.
        assert!(crate::sport_catalog::resolve(226).is_none());
        db.set_workout_code_label(226, Some("My custom workout"))
            .unwrap();
        assert_eq!(
            db.workout_code_label_map()
                .unwrap()
                .get(&226)
                .map(String::as_str),
            Some("My custom workout")
        );

        // A code with evidence must not be overwritten by a custom name.
        assert!(crate::sport_catalog::resolve(52).is_some());
        assert!(db.set_workout_code_label(52, Some("Strength")).is_err());

        // Undo.
        db.set_workout_code_label(226, None).unwrap();
        assert!(db.workout_code_label_map().unwrap().is_empty());
    }

    #[test]
    fn code_labels_reject_control_characters_and_overlong_names() {
        let db = db();
        assert!(db.set_workout_code_label(226, Some("a\nb")).is_err());
        let long = "\u{5b57}".repeat(MAX_CODE_LABEL_CHARS + 1); // multibyte char: the limit counts chars, not bytes
        assert!(db.set_workout_code_label(226, Some(&long)).is_err());
        let ok = "\u{5b57}".repeat(MAX_CODE_LABEL_CHARS);
        assert!(db.set_workout_code_label(226, Some(&ok)).is_ok());
    }

    #[test]
    fn type_override_accepts_the_whole_bundled_catalog_not_a_short_hardcoded_list() {
        // "Squash" is in the catalog. The old code allowed only 15 keys, so the user could not pick it.
        assert!(crate::sport_catalog::is_known_key("squash"));
        assert!(crate::sport_catalog::is_known_key("strength"));
        assert!(!crate::sport_catalog::is_known_key(
            "definitely-not-a-sport"
        ));
    }

    #[test]
    fn device_override_must_name_a_model_that_actually_ships_in_the_catalog() {
        let db = db();
        assert!(db
            .set_device_model_override("A1B2C3", Some("not-a-real-catalog-id"))
            .is_err());
        db.set_device_model_override("A1B2C3", Some("amazfit-balance-2"))
            .unwrap();
        let found = db.device_model_override(&["", "a1b2c3"]).unwrap().unwrap();
        assert_eq!(found.catalog_id, "amazfit-balance-2");

        db.set_device_model_override("A1B2C3", None).unwrap();
        assert!(db.device_model_override(&["A1B2C3"]).unwrap().is_none());
    }

    #[test]
    fn only_changed_device_assignments_invalidate_workout_replay_stamps() {
        let db = db();
        db.insert_raw_record(&crate::models::RawRecord {
            stream: "workouts".into(),
            source_key: "sport_history:run:assignment".into(),
            source_scope: crate::models::SourceScope::Device,
            device_id: None,
            start_utc: Utc::now(),
            end_utc: None,
            payload: serde_json::json!({"data": []}),
            capability: crate::models::CapabilityStatus::Verified,
        })
        .unwrap();
        db.reprocess_raw_records().unwrap();
        assert!(db.pending_replay_plan().unwrap().is_none());
        db.set_device_model_override("AABB", None).unwrap();
        assert!(db.pending_replay_plan().unwrap().is_none());
        db.set_device_model_override("AABB", Some("amazfit-balance-2"))
            .unwrap();
        assert!(db.pending_replay_plan().unwrap().is_some());
        db.reprocess_raw_records_if_needed().unwrap();
        db.set_device_model_override("aabb", Some("amazfit-balance-2"))
            .unwrap();
        assert!(db.pending_replay_plan().unwrap().is_none());
        db.set_device_model_override("aabb", None).unwrap();
        assert!(db.pending_replay_plan().unwrap().is_some());
        assert!(db.device_model_override(&["AABB"]).unwrap().is_none());
    }
}
