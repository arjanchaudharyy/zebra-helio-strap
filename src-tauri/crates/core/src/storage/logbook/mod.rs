//! The subjective layer ("keep the data in ZeppBridge"): shoes, how each workout felt, injuries, races, PBs.
//!
//! Body data comes from the cloud and we cannot keep it. This layer stores **judgments the user makes**: which shoes, how hard this felt, what hurts,
//! which day is the race, what result they ran. The user writes all of it (PBs are computed from local samples). Like scores and the coach profile:
//!
//! - No foreign keys to `workouts`: replay deletes and reinserts workout rows, and user data must not go with them.
//! - Retention cleanup and replay never touch these tables.
//! - The caller takes the cross-process write lock (commands use `with_write`; MCP write tools use `training_plan::standalone::write_once`,
//!   and record into the `mcp_writes` change ledger).
//! - A failed check always returns an error. We never "fix" the value into something else for the user.
//!
//! One file per domain: [`shoes`] shoes and mileage, [`annotations`] RPE / soreness / shoes for one workout,
//! [`injuries`] injury log, [`races`] race calendar and medal wall, [`records`] PB wall. [`profile`] puts these together into one package for the AI.

pub mod annotations;
pub mod injuries;
pub mod profile;
pub mod races;
pub mod records;
pub mod shoes;

use crate::models::{error::Result, ZeppBridgeError};
use chrono::NaiveDate;

/// A failed check: the caller (command layer) reports it to the UI with its own code. The text here is only for logs and the CLI.
pub(crate) fn invalid(message: &str) -> ZeppBridgeError {
    ZeppBridgeError::ConfigError(message.into())
}

/// `YYYY-MM-DD`, and a day that really exists.
pub(crate) fn valid_date(value: &str) -> bool {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .is_ok_and(|date| date.format("%Y-%m-%d").to_string() == value)
}

/// Trim surrounding whitespace. Reject empty values and values longer than `max` characters.
pub(crate) fn clean_text(value: &str, max: usize, what: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max {
        return Err(invalid(what));
    }
    Ok(value.to_string())
}

/// Running types: PBs count only these.
pub const RUN_TYPES: [&str; 3] = ["run", "trail_running", "treadmill"];

/// Which kind of shoe a workout needs. Workouts without shoes (cycling, swimming, ...) return `None`, and the UI does not ask about shoes.
pub fn footwear_family(effective_type: &str) -> Option<&'static str> {
    match effective_type {
        "run" | "treadmill" => Some("running"),
        "trail_running" => Some("trail"),
        "walking" => Some("walking"),
        "hiking" => Some("hiking"),
        _ => None,
    }
}

/// Body parts (the areas you can tap on the body figure). Left and right are separate. Injuries and soreness share this list.
pub const BODY_PARTS: [&str; 40] = [
    "neck",
    "shoulder_l",
    "shoulder_r",
    "upper_back",
    "lower_back",
    "chest",
    "abdomen",
    "hip_l",
    "hip_r",
    "glute_l",
    "glute_r",
    "quad_l",
    "quad_r",
    "hamstring_l",
    "hamstring_r",
    "knee_l",
    "knee_r",
    "shin_l",
    "shin_r",
    "calf_l",
    "calf_r",
    "achilles_l",
    "achilles_r",
    "ankle_l",
    "ankle_r",
    "foot_l",
    "foot_r",
    "it_band_l",
    "it_band_r",
    "head",
    "upper_arm_l",
    "upper_arm_r",
    "elbow_l",
    "elbow_r",
    "forearm_l",
    "forearm_r",
    "wrist_l",
    "wrist_r",
    "hand_l",
    "hand_r",
];

pub(crate) fn valid_body_part(part: &str) -> bool {
    BODY_PARTS.contains(&part)
}
