//! Device profiles and identity hints (split from models/types.rs by domain; shapes unchanged).

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceMatchStatus {
    Exact,
    Alias,
    /// The user told us which model this is, because the account's device
    /// response carried no product name for ZeppBridge to match on. It is a
    /// correction, not a recognition, and the UI must say so.
    UserAssigned,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct DeviceProfile {
    /// Existing display name is retained for backwards compatibility. It may
    /// be a user nickname; `canonical_name` is the official catalog value.
    pub name: Option<String>,
    #[serde(default)]
    pub canonical_name: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    /// With no specific device (a fused cloud source, or the device is unknown), this holds a
    /// code and `name` / `display_name` stay empty. The UI looks up its own text by code:
    /// `ui.device.fused`, `ui.device.undetermined`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_code: Option<String>,
    #[serde(default)]
    pub catalog_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub image_key: Option<String>,
    #[serde(default)]
    pub match_status: DeviceMatchStatus,
    #[serde(default)]
    pub has_local_data: bool,
    #[serde(default)]
    pub last_data_at: Option<String>,
    /// The user hid it from the device list on this machine (`device_local_state`). This only
    /// affects the list; the data stays. A single-device lookup (sleep page, workout detail)
    /// still returns it with this flag.
    #[serde(default)]
    pub hidden: bool,
    pub firmware: Option<String>,
    pub serial: Option<String>,
    pub device_id: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceCacheMetadata {
    pub status: String,
    #[serde(default)]
    pub cached_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub age_seconds: Option<i64>,
    #[serde(default)]
    pub refreshed: bool,
    #[serde(default)]
    pub refresh_error: Option<String>,
    /// The stable code for the `refresh_error` text. The UI looks up its own text by it.
    #[serde(default)]
    pub refresh_error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceProfilesResult {
    /// The devices shown in the list, without hidden ones.
    pub profiles: Vec<DeviceProfile>,
    /// The devices the user hid. Settings > Devices > "N hidden" restores them from here.
    #[serde(default)]
    pub hidden_profiles: Vec<DeviceProfile>,
    pub cache: DeviceCacheMetadata,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceIdentityHint {
    pub aliases: Vec<String>,
    pub name: Option<String>,
    pub firmware: Option<String>,
    pub serial: Option<String>,
    pub device_id: Option<String>,
    pub timezone: Option<String>,
}
