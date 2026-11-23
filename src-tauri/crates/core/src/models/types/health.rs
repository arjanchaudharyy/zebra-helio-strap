//! Source scope, auth metadata, samples, daily metrics, sleep, overview and food (split from models/types.rs by domain; shapes unchanged).

use super::*;

/// The data source scope
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SourceScope {
    UserFused, // Zepp user-level fused result
    Device,    // Explicit device-level data
    Unknown,   // Unknown source
}

impl SourceScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserFused => "user_fused",
            Self::Device => "device",
            Self::Unknown => "unknown",
        }
    }
}

/// Auth information
///
/// **Does not derive `Debug`.** `app_token` holds all rights to this account. Any
/// `tracing::debug!("{auth:?}")`, any `.unwrap()` panic message, or any log a user pastes
/// would leak it as is. No production code prints a full `AuthInfo` today, but the type
/// must enforce that, not memory.
#[derive(Clone, Serialize, Deserialize)]
pub struct AuthInfo {
    pub app_token: String,
    pub user_id: String,
    pub region_host: String,
}

impl std::fmt::Debug for AuthInfo {
    /// Always redacts `app_token`.
    ///
    /// Not even the length is shown: a token length can be a fingerprint, and debugging
    /// only needs to know if a token exists. `user_id` and `region_host` stay. Region
    /// probe debugging needs them, and they are not credentials.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthInfo")
            .field(
                "app_token",
                &if self.app_token.is_empty() {
                    "<empty>"
                } else {
                    "<redacted>"
                },
            )
            .field("user_id", &self.user_id)
            .field("region_host", &self.region_host)
            .finish()
    }
}

/// A metric sample (time series such as heart rate and HRV)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub metric: String,
    pub timestamp: DateTime<Utc>,
    pub value: f64,
    pub unit: String,
    pub source_scope: SourceScope,
    pub device_id: Option<String>,
}

/// A daily metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyMetric {
    pub date: String, // YYYY-MM-DD
    pub metric: String,
    pub value: f64,
    pub unit: String,
    pub source_scope: SourceScope,
    pub device_id: Option<String>,
}

/// A real sleep stage slice. The order must come from the cloud `stage[]`; never build it from totals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SleepStageSlice {
    /// `deep` / `light` / `rem` / `awake`, or `unknown` when the mode is not recognized.
    ///
    /// **`unknown` is not `awake`.** Before, unknown modes became awake to avoid gaps in the
    /// stage bar. The program then claimed the user was awake. An unknown slice is better.
    pub stage: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    /// The raw mode value from the cloud.
    ///
    /// It is kept for the next new Zepp stage mode. "One slice is unknown" gives no lead;
    /// "the unknown mode is 13" does. A wrong match without the raw code has no evidence.
    /// Old rows have `NULL`.
    #[serde(default)]
    pub raw_mode: Option<i64>,
}

/// A sleep session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepSession {
    pub sleep_id: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub score: Option<i32>,
    pub duration_minutes: i32,
    pub deep_minutes: Option<i32>,
    pub light_minutes: Option<i32>,
    pub rem_minutes: Option<i32>,
    pub awake_minutes: Option<i32>,
    pub source_scope: SourceScope,
    pub device_id: Option<String>,
    #[serde(default)]
    pub synced_at: Option<DateTime<Utc>>,
    /// Set only when the cloud gives a separate time-in-bed field. Zepp `ebt`/`obt` are not reliable now, so it is always None.
    #[serde(default)]
    pub time_in_bed_minutes: Option<i32>,
    #[serde(default)]
    pub stages: Vec<SleepStageSlice>,
    /// Times the sleeper woke during the night (`wc`). Distinct from
    /// `awake_minutes`: ten one-minute wakings and one ten-minute waking are
    /// the same duration but not the same night.
    #[serde(default)]
    pub wake_count: Option<i32>,
}

/// One heart rate zone: seconds spent below `upper_bound_bpm` (and above the previous zone's limit).
///
/// It comes directly from the cloud `heart_range`; we do not compute it. The cloud uses the
/// zone limits the user set on the watch. We do not have them, so our own split would differ.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeartRateZoneBucket {
    /// The zone index, from 0.
    pub index: i32,
    /// The upper heart rate limit of this zone.
    pub upper_bound_bpm: i32,
    pub seconds: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartRatePoint {
    pub timestamp: String,
    pub value: f64,
}

/// One reading on the all-day stress curve.
///
/// It has the same shape as a heart rate point but stays a separate type. The two curves
/// have different units, ranges and empty-value meanings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressPoint {
    pub timestamp: String,
    pub value: f64,
}

/// The steps in one hour of a local day (official Zepp `activities?interval=hourly`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HourlySteps {
    /// The local calendar day, YYYY-MM-DD.
    pub date: String,
    /// The local hour, 0-23.
    pub hour: i64,
    pub steps: f64,
}

/// A raw response retained before any normalization.  It deliberately contains
/// no credentials and is suitable for passing to `Database::insert_raw_record`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawRecord {
    pub stream: String,
    pub source_key: String,
    pub source_scope: SourceScope,
    pub device_id: Option<String>,
    pub start_utc: DateTime<Utc>,
    pub end_utc: Option<DateTime<Utc>>,
    pub payload: serde_json::Value,
    pub capability: CapabilityStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataStatus {
    pub stream: String,
    pub status: String,
    pub last_sync: Option<DateTime<Utc>>,
    pub records_written: i64,
    pub capability: String,
    pub needs_reauth: bool,
    pub message: Option<String>,
}

/// The health data overview
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coverage {
    pub start: String,
    pub end: String,
    pub days: i64,
    pub streams: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthOverview {
    pub current_hr: Option<i32>,
    pub resting_hr: Option<i32>,
    pub hrv: Option<f64>,
    pub last_sleep_score: Option<i32>,
    pub readiness: Option<f64>,
    pub bio_charge: Option<f64>,
    pub hybrid_charge: Option<f64>,
    pub training_load: Option<f64>,
    pub vo2max: Option<f64>,
    pub steps_today: Option<i32>,
    pub active_calories_today: Option<i32>,
    pub latest_heart_rate_at: Option<String>,
    pub last_updated: Option<String>,
    pub coverage: Option<Coverage>,
    pub source_scope: Option<String>,
}

/// The extremes and sample count of one day's raw heart rate samples.
///
/// `samples` is not optional. With only 12 samples in a day, `max` is the highest of those
/// 12 points, not of the day. Without the count, the UI must show it as a full-day maximum,
/// and that is a false fact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DailyHeartRateExtreme {
    /// The date in the local time zone, `YYYY-MM-DD`.
    pub date: String,
    pub max: i32,
    pub min: i32,
    pub average: i32,
    /// How many raw heart rate samples this machine stored for this day.
    pub samples: i64,
}

/// Allowlisted food-log details, never the raw account response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FoodEntry {
    pub date: String,
    pub food_log_id: Option<String>,
    pub food_name: Option<String>,
    pub food_text: Option<String>,
    pub meal_type: Option<String>,
    pub mealtime: Option<String>,
    /// Original reported weight; the source does not establish a unit.
    pub measure_weight: Option<f64>,
    pub nutrients: std::collections::BTreeMap<String, f64>,
}
