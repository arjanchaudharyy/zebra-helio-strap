//! Training plan writes: send a plan to the user's Zepp App and watch.
//!
//! The official endpoint is `POST /users/-/workouts?version=2.0.0` (tested on 2026-09-30). It has
//! four quirks that set the shape of this module:
//!
//! 1. **The 7-day window cannot be split**: `endDate = startDate + 6`. Each push replaces
//!    these 7 days under ZeppBridge as a whole. A change to one day also sends the full window. See [`window`].
//! 2. **There is no read endpoint** (GET returns 405). Only our own publish ledger knows what is on the watch
//!    (`storage::training_plan`). So each push **writes the ledger first, then sends**.
//! 3. **The response is not a validation**: V2 returns `{"code":1,"message":"success"}` for any body
//!    (also `{}`). [`validate`] does all validation before the send.
//! 4. **`workouts: []` clears the server's 7 days from now**, whatever dates we give. Revoking access
//!    deletes synced plans automatically. So an empty array is only for a clear the user confirmed, never on revoke.
//!
//! The plan writing format ([`PlanDocument`]) is for people and AI: `"20min"`, `"5km"`,
//! `"hr 135-150"`, `"pace 5:30-5:50"`. It is easier to write than the V2 number fields. [`parse`] reads it into
//! a structure, and [`v2`] turns it into the official payload.
//!
//! **Do not send a form that was not verified on a watch.** After tests with a real account, watch and phone
//! Zepp App (results in `docs/capabilities/official-api-matrix.json`), distance, pace,
//! no target and two workouts on one day are verified, and [`VERIFIED`] enables all of them. A new form
//! stays off here until a test verifies it.
//!
//! Two more hard limits found in tests:
//!
//! - V2 `sport` accepts only 4 main types. Other values (WALKING, HIKING, ...) are **silently dropped one by one**,
//!   and the server still returns success. So walking, hiking and strength parse and display, but validation gives
//!   the publish-blocking `sport_not_deliverable`.
//! - On the watch, a third-party plan only has the main type. The watch asks the user to pick a sub-type again (outdoor run / treadmill, ...).
//!   We can only write the sub-type in the first line of the description (see [`v2`]), and the UI says so.
//!
//! V1 (`version=1.0.0`) is **not used**: targets show as garbled text, and the phone Zepp App crashes on the detail page.

pub mod parse;
pub mod publish;
pub mod standalone;
pub mod v2;
pub mod validate;
pub mod window;

#[cfg(test)]
mod tests;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

pub use validate::{check_plan, HeldWorkout, PlanCheck, PlanContext, PlanIssue, Severity};

/// The forms already verified on a real account and the Zepp App.
///
/// Only a form set to `true` here can be sent. A change here needs a test record (in
/// `docs/capabilities/official-api-matrix.json`).
pub struct Verified {
    /// Distance steps (`"5km"`). The official docs say "unit: seconds"; tests show meters.
    pub distance_steps: bool,
    /// Pace target. The official docs give no unit; tests show speed (m/s).
    pub pace_target: bool,
    /// Steps with no target (`OPEN`, low = high = 0).
    pub open_target: bool,
    /// Two workouts on one day.
    pub same_day_workouts: bool,
}

/// Test records (real account, watch and phone Zepp App; screenshots stay on the user's machine, not in the repo):
///
/// - 2026-09-30: time steps with a heart rate range target display correctly (30 minutes, 130-150). A push to the same window
///   replaces the old entries and does not touch plans from other sources.
/// - 2026-10-06 round 3: `DISTANCE` 1000 shows "1.00 km" (the unit is meters). `OPEN`
///   shows as no target. Two workouts on one day both show. `POWER_LAP` 150-180 shows "150-180 W".
///   A repeat group x3 shows "repeat x3" with the correct total time.
/// - 2026-10-06 rounds 3 and 4: the `PACE_LAP` value is **speed (m/s)** and accepts decimals: 3.03-3.33
///   shows 5'00"-5'30"/km. Swapped low and high values are corrected automatically.
pub const VERIFIED: Verified = Verified {
    distance_steps: true,
    pace_target: true,
    open_target: true,
    same_day_workouts: true,
};

/// The watch list cuts a workout name after about 14 Chinese characters (tested 2026-10-06). A longer name only warns and does not block.
pub const WATCH_NAME_CHARS: usize = 14;

/// The number of days one push covers (both ends included).
pub const WINDOW_DAYS: i64 = 7;

/// How many days after today a plan can reach. A plan further out usually changes before then.
pub const MAX_DAYS_AHEAD: i64 = 56;

// ---------------------------------------------------------------------------
// Writing format: the one people and AI write
// ---------------------------------------------------------------------------

/// A plan. `from` / `to` give **the days this plan controls**: a day in that range with no workout is a rest day,
/// and publishing clears the old plan on those days. If not given, the range is the earliest to the latest workout date.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rest: Vec<PlanRest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub workouts: Vec<PlanWorkout>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRest {
    pub date: String,
    pub bedtime: Option<String>,
    pub sleep_target: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestDay {
    pub date: NaiveDate,
    pub bedtime_minutes: Option<u16>,
    pub sleep_target_seconds: Option<u32>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanWorkout {
    /// `YYYY-MM-DD`, local date.
    pub date: String,
    /// `running` / `cycling` / `pool_swim` / `open_water_swim`. Walking, hiking, strength and others
    /// also parse, but cannot be sent to the watch (see [`Activity`]).
    pub sport: String,
    /// Sub-type: running `outdoor` / `treadmill` / `track`, cycling `outdoor` / `indoor`.
    /// The watch only has the main type, so the sub-type only goes in the first description line as a reminder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    pub name: String,
    /// Workout purpose (a short phrase, for example "build endurance"). Required from zeppbridge-plan/3. The old format still reads,
    /// but validation reports `missing_focus`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    /// Workout notes. Required (`missing_description`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub steps: Vec<PlanStep>,
}

/// One step, or a group that repeats N times. A repeat group cannot hold another repeat group (the V2 example has one level only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PlanStep {
    Repeat {
        repeat: u32,
        steps: Vec<PlanStep>,
    },
    Single {
        /// `warmup` / `active` / `interval` / `recovery` / `rest` / `cooldown`.
        kind: String,
        /// `"20min"`, `"90s"`, `"1h"`, `"400m"`, `"5km"`.
        duration: String,
        /// `"hr 135-150"`, `"pace 5:30-5:50"`, `"power 200-220"`. If not given, there is no target.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Parsed structure: validation, V2 conversion and preview all use it
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sport {
    Running,
    Cycling,
    PoolSwim,
    OpenWaterSwim,
}

impl Sport {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim().to_ascii_lowercase().as_str() {
            // Trail running and mountain biking are sub-types of a main type (#138). The watch only takes the main type;
            // the sub-type goes in the description and the user picks it again on the watch. As a sport value they send as the main type.
            "running" | "run" | "trail_running" | "trail_run" => Self::Running,
            "cycling" | "ride" | "bike" | "mountain_biking" | "mountain_bike" | "mtb" => {
                Self::Cycling
            }
            "pool_swim" | "lap_swimming" | "pool_swimming" => Self::PoolSwim,
            "open_water_swim" | "open_water_swimming" => Self::OpenWaterSwim,
            _ => return None,
        })
    }

    /// The sub-types this main type allows. Swimming has none.
    pub fn variants(self) -> &'static [Variant] {
        match self {
            Self::Running => &[
                Variant::Outdoor,
                Variant::Treadmill,
                Variant::Track,
                Variant::Trail,
            ],
            Self::Cycling => &[Variant::Outdoor, Variant::Indoor, Variant::Mountain],
            Self::PoolSwim | Self::OpenWaterSwim => &[],
        }
    }
}

/// Sub-type. On the watch, a third-party plan only has the main type (tested in R1), so this goes in the description, not in `sport`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    Outdoor,
    Indoor,
    Treadmill,
    Track,
    /// Trail running (#138).
    Trail,
    /// Mountain biking (#138).
    Mountain,
}

impl Variant {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim().to_ascii_lowercase().as_str() {
            "outdoor" | "road" | "outside" => Self::Outdoor,
            "indoor" | "trainer" | "turbo" => Self::Indoor,
            "treadmill" => Self::Treadmill,
            "track" => Self::Track,
            "trail" | "trail_running" | "trail_run" => Self::Trail,
            "mountain" | "mountain_biking" | "mountain_bike" | "mtb" => Self::Mountain,
            _ => return None,
        })
    }
}

/// An activity we can read but the watch cannot take (tested in R1: V2 silently drops these `sport` values one by one).
///
/// Parsing recognizes it so it **displays as written** (a walk stays a walk, never a rest day). Validation gives
/// the publish-blocking `sport_not_deliverable`, so the user deletes it or changes it to a main type that can be sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Walking,
    Hiking,
    Strength,
    Yoga,
    Rowing,
    Elliptical,
    /// Inline skating (#138): V2 has no matching main type.
    InlineSkating,
    /// Cross-country skiing (#138): same as above.
    CrossCountrySkiing,
}

impl Activity {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim().to_ascii_lowercase().as_str() {
            "walking" | "walk" | "long_walk" | "brisk_walk" => Self::Walking,
            "hiking" | "hike" | "trekking" => Self::Hiking,
            "strength" | "strength_training" | "weights" | "gym" => Self::Strength,
            "yoga" | "pilates" | "mobility" | "stretching" => Self::Yoga,
            "rowing" | "row" => Self::Rowing,
            "elliptical" => Self::Elliptical,
            "inline_skating" | "skating" | "roller_skating" => Self::InlineSkating,
            "cross_country_skiing" | "xc_skiing" | "nordic_skiing" | "skiing" => {
                Self::CrossCountrySkiing
            }
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Intensity {
    Warmup,
    Active,
    Interval,
    Recovery,
    Rest,
    Cooldown,
}

impl Intensity {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim().to_ascii_lowercase().as_str() {
            "warmup" | "warm_up" => Self::Warmup,
            "active" | "main" => Self::Active,
            "interval" | "work" => Self::Interval,
            "recovery" => Self::Recovery,
            "rest" => Self::Rest,
            "cooldown" | "cool_down" => Self::Cooldown,
            _ => return None,
        })
    }
}

/// How long a step is: by time or by distance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum StepLength {
    Time { seconds: u32 },
    Distance { meters: u32 },
}

/// The target of a step. Pace is stored as seconds per km; `fast` is the smaller value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Target {
    Open,
    HeartRate { low: u16, high: u16 },
    Pace { fast: u16, slow: u16 },
    Power { low: u16, high: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub intensity: Intensity,
    pub length: StepLength,
    pub target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum StepNode {
    Step(Step),
    Repeat { times: u32, steps: Vec<Step> },
}

impl StepNode {
    /// The total seconds of this section when expanded. A distance step has no duration, so it returns `None`.
    pub fn seconds(&self) -> Option<u64> {
        let step_seconds = |step: &Step| match step.length {
            StepLength::Time { seconds } => Some(u64::from(seconds)),
            StepLength::Distance { .. } => None,
        };
        match self {
            Self::Step(step) => step_seconds(step),
            Self::Repeat { times, steps } => steps
                .iter()
                .map(step_seconds)
                .sum::<Option<u64>>()
                .map(|total| total * u64::from(*times)),
        }
    }
}

/// A validated workout.
///
/// `variant` / `focus` were added in 2026-10. Older workouts in the ledger do not have them, so they read as `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workout {
    pub date: NaiveDate,
    pub sport: Sport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<Variant>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub steps: Vec<StepNode>,
}

impl Workout {
    /// The total time when every step is by time. If any step is by distance, this is `None` (no estimate).
    pub fn total_seconds(&self) -> Option<u64> {
        self.steps.iter().map(StepNode::seconds).sum()
    }

    /// The total meters when every step is by distance. If any step is by time, this is `None` (also no estimate).
    pub fn total_meters(&self) -> Option<u64> {
        let meters = |step: &Step| match step.length {
            StepLength::Distance { meters } => Some(u64::from(meters)),
            StepLength::Time { .. } => None,
        };
        self.steps
            .iter()
            .map(|node| match node {
                StepNode::Step(step) => meters(step),
                StepNode::Repeat { times, steps } => steps
                    .iter()
                    .map(meters)
                    .sum::<Option<u64>>()
                    .map(|total| total * u64::from(*times)),
            })
            .sum()
    }
}
