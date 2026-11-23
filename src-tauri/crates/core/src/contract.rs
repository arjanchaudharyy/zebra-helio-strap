//! Read-only data contract.
//!
//! The GUI, Local API, CLI and MCP are four outputs for the same data. If each
//! output explains "what unit is this number" and "how is a missing sample shown"
//! on its own, sooner or later they give four different answers. Users and
//! external models cannot tell which one is correct. So units, time zones,
//! sources and missing values are defined here once, and all four adapters
//! read them from here.
//!
//! This contract is part of the public promise. A change to it changes the
//! meaning that external tools see, so treat it as a breaking change.

/// Contract version. Bump it when the meaning of a field changes. Adding an optional field does not need a bump.
pub const CONTRACT_VERSION: &str = "1";

/// How many days one incremental sync fetches back.
///
/// This value lives in the contract, not in scattered code, because it appears
/// in three places: the window the backend really requests, the UI text
/// "syncing the last N days", and two architecture documents. When it changed
/// from 7 to 30, only the backend changed. The UI and the docs still said 7.
/// The number the user saw was not what the program did, and no test caught
/// this drift.
///
/// **Changing this value = changing the contract.** The UI reads it from
/// `AppStatus.incremental_sync_days`. Do not hard-code a number in the frontend again.
pub const INCREMENTAL_SYNC_DAYS: i64 = 30;

/// The quiet scheduled auto sync fetches only this many recent days again.
///
/// Auto sync runs every 15 minutes. A full 30-day window each time means about
/// 80 to 90 requests per run and close to ten thousand per day. For an
/// unofficial API this wastes requests and invites rate limits, and 27 of those
/// days almost always come back unchanged. Data that the watch uploads to the
/// cloud a few days late is covered by the "full window at least once a day" rule below.
///
/// Manual sync, startup sync, tray sync and the CLI still use the full window. The UI also reads this value from `AppStatus`.
pub const QUICK_SYNC_DAYS: i64 = 3;

/// If the last **successful** full-window sync is older than this many hours, the scheduled sync fetches the full window again.
pub const FULL_WINDOW_REFRESH_HOURS: i64 = 24;

/// How time is expressed.
pub const TIME_CONVENTION: &str = "All timestamps are RFC 3339 with a time zone offset. The cloud fetch time (synced_at / fetched_at) and the time a health sample happened (start_time / timestamp) are two different things. One never replaces the other.";

/// How missing values are expressed. This is the most important public promise of this project.
pub const MISSING_VALUE_CONVENTION: &str = "No sample means missing: the field is null or the whole entry is absent. Gaps are never filled with 0, with the previous value or with an estimate. If a series has fewer points than its time span, those days really have no data. Nothing was lost.";

/// How the source is expressed.
pub const SOURCE_CONVENTION: &str = "source_scope tells which layer a record comes from: device means one watch reported it, user_fused means the Zepp cloud merged it across devices, unknown means the source cannot be determined. unknown is never merged into device.";

/// Privacy boundary. The CLI `--help` and the MCP server instructions both use this text.
pub const PRIVACY_NOTE: &str =
    "Reads only the local SQLite database. It does not go online, does not listen on any port, and does not return credentials or absolute local paths. All data stays on this computer.";

/// The public definition of one metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricContract {
    pub metric: &'static str,
    pub unit: &'static str,
    pub description: &'static str,
}

/// The JSON that `zeppbridge-cli contract` prints. The CLI and the push (the PA `contract.json`)
/// both use it, so the two sides do not each build their own copy.
pub fn contract_json() -> serde_json::Value {
    serde_json::json!({
        "contractVersion": CONTRACT_VERSION,
        "time": TIME_CONVENTION,
        "missingValues": MISSING_VALUE_CONVENTION,
        "source": SOURCE_CONVENTION,
        "privacy": PRIVACY_NOTE,
        "metrics": METRICS.iter().map(|item| serde_json::json!({
            "metric": item.metric,
            "unit": item.unit,
            "description": item.description,
        })).collect::<Vec<_>>(),
    })
}

/// Metrics that can be read as a daily series.
///
/// Units are always short machine-readable strings (`bpm`, `ml/kg/min`). The
/// human text goes in `description`, so the units in the MCP schema do not change with the language.
pub const METRICS: [MetricContract; 41] = [
    MetricContract {
        metric: "blood_pressure_systolic",
        unit: "mmHg",
        description: "Systolic blood pressure, an explicit numeric field of the candidate API. Not yet checked against an account with real measurements",
    },
    MetricContract {
        metric: "blood_pressure_diastolic",
        unit: "mmHg",
        description: "Diastolic blood pressure, stored as a pair with systolic, with the measurement time kept",
    },
    MetricContract {
        metric: "blood_pressure_pulse",
        unit: "bpm",
        description: "Pulse during the blood pressure measurement. Not written if missing, and not mixed into all-day heart rate",
    },
    // Weight and body composition. From `/users/{id}/members/-1/weightRecords`,
    // not the events surface. See `ZeppConnector::fetch_weight_records`.
    MetricContract {
        metric: "weight",
        unit: "kg",
        description: "Body weight, one entry per weigh-in, not aggregated by day",
    },
    MetricContract {
        metric: "bmi",
        unit: "kg/m2",
        description: "Body mass index, given by the cloud together with weight",
    },
    MetricContract {
        metric: "height",
        unit: "cm",
        description: "Height. A profile value, not a measurement of that weigh-in, returned with each weight record",
    },
    MetricContract {
        metric: "body_fat_rate",
        unit: "%",
        description: "Body fat rate. Needs a body composition scale. The field name is not yet checked on a real scale. Readings outside a plausible range are dropped",
    },
    MetricContract {
        metric: "body_water_rate",
        unit: "%",
        description: "Body water rate. Same as above",
    },
    MetricContract {
        metric: "muscle_mass",
        unit: "kg",
        description: "Muscle mass. Same as above",
    },
    MetricContract {
        metric: "bone_mass",
        unit: "kg",
        description: "Bone mass. Same as above",
    },
    MetricContract {
        metric: "protein_rate",
        unit: "%",
        description: "Protein rate. Same as above",
    },
    MetricContract {
        metric: "visceral_fat",
        unit: "grade",
        description: "Visceral fat grade, not a percentage. Same as above",
    },
    MetricContract {
        metric: "bmr",
        unit: "kcal/day",
        description: "Basal metabolic rate (resting energy use), estimated by the scale from body composition. Same as above. It is included when daily activity is selected for an AI handoff. Without a scale there is no value. It is never derived from total or active energy",
    },
    MetricContract {
        metric: "body_balance_score",
        unit: "score",
        description: "Body balance score, 0-100, given by the scale during a one-leg measurement",
    },
    // Food log. From `/v2/users/me/events?eventType=Food` (no subType).
    // Field names are not yet checked on real food data. Readings outside a plausible range are dropped.
    MetricContract {
        metric: "intake_calories",
        unit: "kcal",
        description: "Energy intake from food for the day. Needs the food log feature of the Zepp app (the mainland China version does not have it)",
    },
    MetricContract {
        metric: "intake_protein_g",
        unit: "g",
        description: "Protein intake for the day. Same as above",
    },
    MetricContract {
        metric: "intake_fat_g",
        unit: "g",
        description: "Fat intake for the day. Same as above",
    },
    MetricContract {
        metric: "intake_carbs_g",
        unit: "g",
        description: "Carbohydrate intake for the day. Same as above",
    },
    MetricContract {
        metric: "readiness",
        unit: "score",
        description: "Overall readiness score",
    },
    MetricContract {
        metric: "physical_readiness",
        unit: "score",
        description: "Physical readiness score",
    },
    MetricContract {
        metric: "mental_readiness",
        unit: "score",
        description: "Mental readiness score",
    },
    MetricContract {
        metric: "hybrid_charge",
        unit: "score",
        description: "Overall energy score",
    },
    MetricContract {
        metric: "physical_charge",
        unit: "score",
        description: "Physical energy score",
    },
    MetricContract {
        metric: "mental_charge",
        unit: "score",
        description: "Mental energy score",
    },
    MetricContract {
        metric: "stress",
        unit: "score",
        description: "Stress score, with the measured minimum and maximum of the day",
    },
    MetricContract {
        metric: "respiratory_rate",
        unit: "breaths/min",
        description: "Respiratory rate, with the measured minimum and maximum of the day",
    },
    MetricContract {
        metric: "resting_hr",
        unit: "bpm",
        // An AI read this as "sleep heart rate". It is the rhr field of the band's sleep summary.
        // It is not the average heart rate during sleep, even when the two values are equal.
        description: "Resting heart rate: the rhr field of the band's sleep summary, not the average heart rate during sleep",
    },
    MetricContract {
        metric: "spo2_odi",
        unit: "events/h",
        description: "Overnight oxygen desaturation index",
    },
    MetricContract {
        metric: "spo2_night_score",
        unit: "score",
        description: "Overnight blood oxygen score",
    },
    MetricContract {
        metric: "spo2_measured_minutes",
        unit: "min",
        description: "Measured duration of overnight blood oxygen",
    },
    MetricContract {
        metric: "spo2",
        unit: "%",
        description: "Single blood oxygen saturation readings, folded by day",
    },
    MetricContract {
        metric: "training_load",
        unit: "load",
        description: "Training load",
    },
    MetricContract {
        metric: "vo2max",
        unit: "ml/kg/min",
        description: "Maximal oxygen uptake",
    },
    MetricContract {
        metric: "lactate_threshold_hr",
        unit: "bpm",
        description: "Lactate threshold heart rate",
    },
    MetricContract {
        metric: "lactate_threshold_pace",
        unit: "s/km",
        description: "Lactate threshold pace",
    },
    MetricContract {
        metric: "pai_daily",
        unit: "pai",
        description: "PAI for the day",
    },
    MetricContract {
        metric: "pai_low_zone",
        unit: "pai",
        description: "PAI in the low intensity zone",
    },
    MetricContract {
        metric: "pai_medium_zone",
        unit: "pai",
        description: "PAI in the medium intensity zone",
    },
    MetricContract {
        metric: "pai_high_zone",
        unit: "pai",
        description: "PAI in the high intensity zone",
    },
    MetricContract {
        metric: "pai_total",
        unit: "pai",
        description: "Total PAI over seven days",
    },
    MetricContract {
        metric: "sleep_score",
        unit: "score",
        description: "Sleep score",
    },
];

/// Today as a "health day": computed in the device time zone (the IANA name saved from the
/// device info during sync). If the device time zone is unknown or the name is not
/// recognized, fall back to this computer's local time zone, not to UTC.
///
/// Daily metrics (`daily_metrics.date`) use the device local day. Before, "today's steps"
/// compared against the computer's local day. When the computer and the watch were in
/// different time zones (travel, remote desktop), today's steps were empty or showed
/// yesterday near midnight.
pub fn health_today(
    device_time_zone: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> chrono::NaiveDate {
    if let Some(name) = device_time_zone
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        if let (Ok(zone), Ok(timestamp)) = (
            jiff::tz::TimeZone::get(name),
            jiff::Timestamp::from_second(now.timestamp()),
        ) {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(
                &timestamp.to_zoned(zone).date().to_string(),
                "%Y-%m-%d",
            ) {
                return date;
            }
        }
    }
    now.with_timezone(&chrono::Local).date_naive()
}

/// List of metric names, for the enum in the MCP schema.
pub fn metric_names() -> Vec<&'static str> {
    METRICS.iter().map(|item| item.metric).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_metric_in_the_contract_is_uniquely_named() {
        let mut names = metric_names();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "the contract must not have duplicate metric names");
    }

    #[test]
    fn the_missing_value_promise_stays_explicit() {
        // This promise is a hard product rule, not copy text. Changing it changes the
        // public meaning. It must be a deliberate breaking change, not a casual rewording.
        assert!(MISSING_VALUE_CONVENTION.contains("never filled with 0"));
        assert!(!PRIVACY_NOTE.is_empty());
    }
}
