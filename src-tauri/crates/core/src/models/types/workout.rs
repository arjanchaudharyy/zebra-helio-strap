//! Workout records and per-point series (split from models/types.rs by domain; shapes unchanged).

use super::*;

/// A `Default` for tests only.
///
/// `Workout` has more than twenty fields, and more than ten are optional summary values
/// (set when the cloud sends them, else None). A test cares about one or two, but must
/// write all of them. Each new field then changes many tests with no assertion value.
///
/// It exists only in tests. Production code must write every `Workout` field, so a timestamp
/// that defaults to the UNIX epoch cannot get into real data.
#[cfg(test)]
impl Default for Workout {
    fn default() -> Self {
        Self {
            workout_id: String::new(),
            workout_type: String::new(),
            normalized_type: String::new(),
            type_source: "missing".to_string(),
            user_override: None,
            effective_type: String::new(),
            custom_label: None,
            start_time: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch time is valid"),
            end_time: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch time is valid"),
            distance_meters: None,
            calories: None,
            avg_hr: None,
            max_hr: None,
            training_load: None,
            vo2max: None,
            min_hr: None,
            total_steps: None,
            moving_seconds: None,
            elevation_gain_m: None,
            elevation_loss_m: None,
            max_altitude_m: None,
            min_altitude_m: None,
            training_effect: None,
            anaerobic_training_effect: None,
            rpe: None,
            avg_cadence_spm: None,
            max_cadence_spm: None,
            avg_stride_cm: None,
            gear_id: None,
            hr_zones: Vec::new(),
            source_scope: SourceScope::Unknown,
            device_id: None,
            synced_at: None,
            gps_available: false,
            sample_count: 0,
            zepp_source: None,
            zepp_type: None,
        }
    }
}

/// A workout record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workout {
    pub workout_id: String,
    /// Backwards-compatible alias for `normalized_type`. Request-path names
    /// are never allowed to populate this field.
    pub workout_type: String,
    /// ZeppBridge's interpretation of the record's own type evidence.
    pub normalized_type: String,
    /// `numeric_mapped`, `unknown_code`, `string_field`, or `missing`.
    pub type_source: String,
    /// Optional local correction. This never overwrites Zepp's raw type or the
    /// normalizer result and therefore survives a raw-record replay.
    #[serde(default)]
    pub user_override: Option<String>,
    /// The type consumers should display: override first, otherwise normalized.
    pub effective_type: String,
    /// The name the user gave this Zepp type code, when the bundled catalog
    /// cannot resolve it. Zepp's custom training templates arrive as numbers
    /// with no name attached, and guessing what code 226 means would be
    /// inventing data - so the user names it once and every record with that
    /// code uses it. Never set for codes the catalog already knows.
    #[serde(default)]
    pub custom_label: Option<String>,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub distance_meters: Option<f64>,
    pub calories: Option<i32>,
    pub avg_hr: Option<i32>,
    pub max_hr: Option<i32>,
    pub training_load: Option<f64>,
    pub vo2max: Option<f64>,
    /// The minimum heart rate of this workout. The cloud always sent it; we did not read it before.
    #[serde(default)]
    pub min_hr: Option<i32>,
    /// The step count. Sports with no steps, such as cycling, have `None`, not 0. "No steps"
    /// and "zero steps" are different.
    #[serde(default)]
    pub total_steps: Option<i32>,
    /// The workout duration in seconds, from the cloud `run_time`. It differs from
    /// `end_time - start_time`, which includes pauses.
    #[serde(default)]
    pub moving_seconds: Option<i64>,
    /// Total ascent / descent (meters).
    ///
    /// The cloud value comes first, because the user sees it in the Zepp App. Only when the
    /// cloud gives none does it fall back to the parser value from the altitude series with a
    /// 1 m noise floor. The two can differ (one walk: cloud 59 m, ours 37 m), and users report
    /// "does not match the App" as a bug.
    #[serde(default)]
    pub elevation_gain_m: Option<f64>,
    #[serde(default)]
    pub elevation_loss_m: Option<f64>,
    /// Maximum / minimum altitude (meters).
    #[serde(default)]
    pub max_altitude_m: Option<f64>,
    #[serde(default)]
    pub min_altitude_m: Option<f64>,
    /// Training effect, aerobic and anaerobic. The cloud stores ten times the value as an integer (22 means 2.2).
    #[serde(default)]
    pub training_effect: Option<f64>,
    #[serde(default)]
    pub anaerobic_training_effect: Option<f64>,
    /// Rate of perceived exertion (RPE), which the user picks on the watch.
    #[serde(default)]
    pub rpe: Option<i32>,
    /// Average / maximum cadence, in steps per minute.
    ///
    /// The unit was checked against the cloud summary; see the table above
    /// `export_fit::steps_per_minute_to_fit_cadence`.
    #[serde(default)]
    pub avg_cadence_spm: Option<f64>,
    #[serde(default)]
    pub max_cadence_spm: Option<f64>,
    /// Average stride length (centimeters).
    #[serde(default)]
    pub avg_stride_cm: Option<f64>,
    /// The Zepp ID of the gear (shoes) for this workout, the summary `gearId` (#143).
    /// Only the ID: the cloud gives no shoe name, and the UI and export do not invent one.
    #[serde(default)]
    pub gear_id: Option<String>,
    /// The heart rate zone distribution the cloud computed.
    ///
    /// This is the Zepp `heart_range`: six parts of `seconds,zone upper limit`. Before, it was
    /// dropped, so the UI showed nothing for time in each zone although the answer existed.
    #[serde(default)]
    pub hr_zones: Vec<HeartRateZoneBucket>,
    pub source_scope: SourceScope,
    pub device_id: Option<String>,
    #[serde(default)]
    pub synced_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub gps_available: bool,
    #[serde(default)]
    pub sample_count: i64,
    /// History `source` query value required by `/v1/sport/run/detail.json`.
    #[serde(default)]
    pub zepp_source: Option<String>,
    /// Zepp history `type` integer. Running is `1`.
    #[serde(default)]
    pub zepp_type: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkoutRoutePoint {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub segment_start: bool,
    pub timestamp: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_m: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WorkoutSeriesSample {
    pub timestamp: String,
    pub heart_rate: Option<i32>,
    pub speed: Option<f64>,
    pub pace: Option<f64>,
    pub cadence: Option<f64>,
    pub stride_cm: Option<f64>,
    pub altitude_m: Option<f64>,
    /// Running power in watts (`power_meter`), verified against the workout
    /// summary's `average_power` / `max_power`.
    pub power_watts: Option<f64>,
    /// Ground contact time in milliseconds (`runPosture` field 1), verified
    /// against `averageGct` / `minGct`.
    pub ground_contact_ms: Option<f64>,
    /// Vertical oscillation in millimetres (`runPosture` field 2), verified
    /// against `averageVo` / `maxVo`.
    pub vertical_oscillation_mm: Option<f64>,
    /// Vertical stride ratio in percent (`runPosture` field 3), verified
    /// against `avgVertStrideRatio`.
    pub vertical_ratio_pct: Option<f64>,
    /// Grade-adjusted equivalent pace in seconds per kilometre (`equivPace`),
    /// verified against `bestEquivPace` and `avgEquivPace`.
    pub equivalent_pace_s_per_km: Option<f64>,
    /// The watch's own cumulative distance in metres (`currentDistance`), only
    /// on seconds that carried a reading. FIT `record.distance` (#139).
    #[serde(default)]
    pub distance_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkoutPause {
    pub start_time: String,
    pub end_time: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WorkoutSeriesSummary {
    pub average_pace: Option<f64>,
    pub average_cadence: Option<f64>,
    pub max_cadence: Option<f64>,
    pub average_stride_cm: Option<f64>,
    pub elevation_gain_m: Option<f64>,
    pub elevation_loss_m: Option<f64>,
    pub average_power_watts: Option<f64>,
    pub max_power_watts: Option<f64>,
    pub average_ground_contact_ms: Option<f64>,
    pub average_vertical_oscillation_mm: Option<f64>,
    pub average_vertical_ratio_pct: Option<f64>,
    /// The fastest equivalent pace in the series, in seconds per kilometre.
    pub best_equivalent_pace_s_per_km: Option<f64>,
}

/// One kilometre of a workout, as stored.
///
/// Times are RFC3339 strings to match the rest of the series shapes crossing
/// the IPC boundary.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkoutSplitRow {
    pub index: i32,
    pub start_time: String,
    pub end_time: String,
    pub distance_m: f64,
    pub duration_seconds: i64,
    pub pace_min_per_km: Option<f64>,
    pub avg_hr: Option<i32>,
    pub max_hr: Option<i32>,
    pub elevation_gain_m: Option<f64>,
    pub elevation_loss_m: Option<f64>,
    pub partial: bool,
    /// The average per-point cadence (steps/min) in this split, from readings above 0 only.
    /// With no per-point cadence it is `None`. It does not use the workout average or 0.
    #[serde(default)]
    pub avg_cadence_spm: Option<f64>,
}

/// A lap the watch recorded, in the shape read from the database.
///
/// It sits next to [`WorkoutSplitRow`] and does not replace it. We cut splits per kilometer.
/// The watch records laps during the workout (lap button, auto distance laps, interval steps).
/// One run can have both, with different meanings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkoutLapRow {
    pub index: i32,
    pub start_time: String,
    pub end_time: String,
    pub distance_m: f64,
    pub duration_seconds: i64,
    pub avg_hr: Option<i32>,
    pub max_hr: Option<i32>,
    /// Same as [`WorkoutSplitRow::avg_cadence_spm`]. Interval step cadence uses it.
    #[serde(default)]
    pub avg_cadence_spm: Option<f64>,
}

/// One set of a strength workout (from the 62 columns of the cloud detail `lap`).
///
/// Only stable English codes and numbers, no natural-language names (per the UI language contract).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrengthSetRow {
    /// The set number, from 1.
    pub set_index: i32,
    /// The raw Zepp `actionType` number (0 = no exercise picked; 1..=130 standard library; >130 firmware number outside the table).
    pub action_code: i32,
    /// The exercise key from the standard library (such as `"biceps_curl"`); `None` for numbers outside the table. Never guess a name.
    pub exercise_key: Option<String>,
    /// The primary muscle group key from the standard library (such as `"biceps"`); `None` with no exercise or a number outside the table.
    pub muscle_group: Option<String>,
    pub reps: i32,
    /// The raw positive weight entered on the watch; `None` for bodyweight exercises or no weight.
    pub weight_value: Option<f64>,
    /// `"kg"` or `"lb"` (set by each set's own `col[23]`, not one value for the workout).
    pub weight_unit: String,
    /// The weight in kilograms (`lb * 0.45359237`), for the total volume.
    pub weight_kg: Option<f64>,
    /// The set duration (milliseconds): `col[43]` first, else `col[1] * 1000`.
    pub duration_ms: Option<i64>,
    /// The average heart rate in the set (bpm, set only when `> 0`).
    pub avg_hr: Option<i32>,
    /// The kilocalories burned in the set (set only when `>= 0`).
    pub calories_kcal: Option<f64>,
    /// The English exercise name from the Zepp exercise catalog (read from the catalog cache, not stored); the UI picks its own language.
    #[serde(default)]
    pub exercise_name: Option<String>,
    /// The secondary muscle groups of this exercise in the Zepp catalog (the nine group keys, without the primary group).
    #[serde(default)]
    pub secondary_muscle_groups: Vec<String>,
}

/// Set count and total reps per exercise (from per-set details when present, else from the summary stream).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StrengthExerciseGroup {
    pub action_code: i32,
    pub exercise_key: Option<String>,
    pub muscle_group: Option<String>,
    pub sets: i32,
    pub total_reps: i32,
    #[serde(default)]
    pub exercise_name: Option<String>,
    #[serde(default)]
    pub secondary_muscle_groups: Vec<String>,
}

/// The summary and per-set details of one strength workout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrengthWorkoutDetail {
    pub total_sets: i32,
    pub total_reps: i32,
    /// The total volume (`sum of reps * weight_kg`); `None` for all-bodyweight workouts or with no per-set weights yet.
    pub total_volume_kg: Option<f64>,
    /// The total rest between sets (seconds, from the summary stream `restTimeBetweenGroupsWithMillis / 1000` or `rope_skipping_rest_time`).
    pub rest_seconds: Option<i64>,
    /// The per-set details; an empty array before the detail is fetched, and the frontend then shows `exercise_groups`.
    pub sets: Vec<StrengthSetRow>,
    /// Set count and total reps per exercise.
    pub exercise_groups: Vec<StrengthExerciseGroup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkoutSeries {
    pub workout_id: String,
    pub samples: Vec<WorkoutSeriesSample>,
    pub route: Vec<WorkoutRoutePoint>,
    pub pauses: Vec<WorkoutPause>,
    pub splits: Vec<WorkoutSplitRow>,
    /// The laps the watch recorded. An empty array means no laps, which is true for most workouts.
    #[serde(default)]
    pub laps: Vec<WorkoutLapRow>,
    pub summary: WorkoutSeriesSummary,
    /// The main climb / descent segments. `None` = no altitude samples or an unreliable distance
    /// (no card); an empty list = altitude exists but no clear climb.
    #[serde(default)]
    pub climbs: Option<crate::workout_climbs::WorkoutClimbs>,
    /// The strength summary and per-set details; `None` for non-strength workouts or with no strength fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strength: Option<StrengthWorkoutDetail>,
}

#[derive(Debug, Clone)]
pub struct PendingWorkoutDetail {
    pub workout_id: String,
    pub source: String,
}
