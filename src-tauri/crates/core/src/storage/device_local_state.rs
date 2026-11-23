//! Local device state: today this is only "hidden" (3.1.1).
//!
//! Like manual assignment (`corrections.rs`), it stays local: it is not sent to the cloud and replay ignores it. Hiding **only hides the list entry**:
//! the device lists in settings, overview and device details no longer show it, but its samples, charts and exports stay the same.
//! Filtering every sample in the database by device_id costs far more than it gives. The user only wants "stop listing this device".
//!
//! The key is the same as for manual assignment: device_id, else the serial number (`deviceKey` in the UI `useDevices.ts`), compared in lowercase.

use std::collections::HashSet;

use chrono::Utc;
use rusqlite::params;

use super::Database;
use crate::models::{error::Result, ZeppBridgeError};

impl Database {
    /// Hide or restore a device. Restore deletes the row, so the table only holds hidden devices.
    pub fn set_device_hidden(&self, device_key: &str, hidden: bool) -> Result<()> {
        let device_key = device_key.trim();
        if device_key.is_empty() {
            return Err(ZeppBridgeError::ConfigError("Device identifier must not be empty".into()));
        }
        let tx = self.conn.unchecked_transaction()?;
        // Keep one row for the same key in different case, as manual assignment does.
        self.conn.execute(
            "DELETE FROM device_local_state WHERE lower(device_key) = lower(?1)",
            [device_key],
        )?;
        if hidden {
            self.conn.execute(
                "INSERT INTO device_local_state(device_key, hidden, updated_at) VALUES(?1, 1, ?2)",
                params![device_key, Utc::now().to_rfc3339()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Keys of hidden devices, all lowercase.
    pub fn hidden_device_keys(&self) -> Result<HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT lower(device_key) FROM device_local_state WHERE hidden = 1")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut keys = HashSet::new();
        for row in rows {
            keys.insert(row?);
        }
        Ok(keys)
    }
}

/// Whether any identifier of this device (device_id, serial number) is in the hidden set.
///
/// Check both: a device such as Helio has two device_ids. The user hides the key of the list row,
/// and the profile built from the other path can carry only the serial number.
pub fn is_hidden_device(
    hidden: &HashSet<String>,
    device_id: Option<&str>,
    serial: Option<&str>,
) -> bool {
    if hidden.is_empty() {
        return false;
    }
    [device_id, serial]
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_lowercase())
        .any(|value| !value.is_empty() && hidden.contains(&value))
}
