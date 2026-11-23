//! Strength training (Zepp `type = 52`): the 125 standard exercise dictionary, decoding of the 62-column per-set `lap`, and summary aggregation.
//!
//! Produces only stable English codes and numbers, never natural language text. The frontend uses `exercise_key` / `muscle_group`
//! to pick the current UI language. Off-table codes such as `1030` return `(None, None)`, and the frontend shows `#1030` as is.
//! Per-set rest time and the five-part exercise quality score are never invented: the raw payload has no per-set rest field, and the order of the score parts is not confirmed.

use crate::models::{StrengthExerciseGroup, StrengthSetRow, StrengthWorkoutDetail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Exact factor for international pounds to kilograms.
pub const LB_TO_KG: f64 = 0.45359237;

/// Allowlist of the nine stable muscle group keys (tests use it to keep the dictionary aligned with the frontend language tables).
pub const STRENGTH_MUSCLE_GROUPS: [&str; 9] = [
    "chest",
    "back",
    "shoulders",
    "biceps",
    "triceps",
    "forearms",
    "core",
    "glutes",
    "legs",
];

/// Static table from all 125 standard Zepp `/sport/config/muscle` exercise codes (0..=130, with 6 unused gaps)
/// to `(exercise_key, muscle_group)`, sorted strictly by `action_code` for binary search.
pub const STRENGTH_ACTION_CATALOG: [(i32, &str, Option<&str>); 125] = [
    (0, "free_exercise", None),
    (1, "bodyweight_squats", Some("legs")),
    (2, "straight_arm_front_squats", Some("legs")),
    (3, "barbell_squats", Some("legs")),
    (4, "triceps_pushdowns", Some("triceps")),
    (5, "bent_over_row", Some("back")),
    (6, "bench_press", Some("chest")),
    (7, "sit_up_holds", Some("core")),
    (8, "knee_touch_sit_ups", Some("core")),
    (9, "dumbbell_flys", Some("chest")),
    (10, "burpee_jumps", Some("legs")),
    (11, "burpees", Some("legs")),
    (12, "overhead_tricep_extensions", Some("triceps")),
    (13, "lying_tricep_extensions", Some("triceps")),
    (14, "lateral_raises", Some("shoulders")),
    (15, "jumping_jacks", Some("legs")),
    (16, "incline_dumbbell_row", Some("back")),
    (17, "bent_over_dumbbell_row", Some("back")),
    (18, "bent_over_barbell_row", Some("back")),
    (19, "single_arm_dumbbell_row_left", Some("back")),
    (20, "single_arm_dumbbell_row_right", Some("back")),
    (21, "dumbbell_supine_triceps_extensions", Some("triceps")),
    (22, "supine_triceps_extensions", Some("triceps")),
    (
        23,
        "dumbbell_single_arm_triceps_extensions_left",
        Some("triceps"),
    ),
    (
        24,
        "seated_bent_over_dumbbell_triceps_extensions",
        Some("triceps"),
    ),
    (25, "seated_triceps_press", Some("triceps")),
    (26, "smith_machine_squats", Some("legs")),
    (30, "dumbbell_bench_press", Some("chest")),
    (32, "smith_machine_bench_press", Some("chest")),
    (
        33,
        "dumbbell_single_arm_triceps_extensions_right",
        Some("triceps"),
    ),
    (34, "incline_dumbbell_flys", Some("chest")),
    (35, "decline_dumbbell_flys", Some("chest")),
    (37, "v_bar_tricep_pushdowns", Some("triceps")),
    (38, "rope_triceps_pushdowns", Some("triceps")),
    (39, "elastic_band_lateral_raise", Some("shoulders")),
    (40, "dumbbell_lateral_raise", Some("shoulders")),
    (41, "dumbbell_front_raise", Some("shoulders")),
    (42, "pectoral_muscles", Some("chest")),
    (43, "rectus_abdominis", Some("core")),
    (44, "external_oblique", Some("core")),
    (45, "upper_back_muscles", Some("back")),
    (46, "lower_back_muscles", Some("back")),
    (47, "lateral_back_muscles", Some("back")),
    (48, "shoulder_muscles", Some("shoulders")),
    (49, "neck_muscles", Some("shoulders")),
    (50, "upper_arm_front_muscles", Some("biceps")),
    (51, "upper_arm_back_muscles", Some("triceps")),
    (52, "front_forearm_muscles", Some("forearms")),
    (53, "back_forearm_muscles", Some("forearms")),
    (54, "hip_muscles", Some("glutes")),
    (55, "quadriceps", Some("legs")),
    (56, "hamstrings", Some("legs")),
    (57, "front_calf_muscles", Some("legs")),
    (58, "back_calf_muscles", Some("legs")),
    (60, "lat_pulldowns", Some("back")),
    (61, "deadlift", Some("legs")),
    (62, "push_up", Some("chest")),
    (63, "shoulder_press", Some("shoulders")),
    (64, "pull_up", Some("back")),
    (65, "biceps_curl", Some("biceps")),
    (66, "seated_cable_row", Some("back")),
    (67, "upright_row", Some("shoulders")),
    (68, "straight_arm_pulldown", Some("back")),
    (69, "russian_twist", Some("core")),
    (70, "barbell_deadlift", Some("legs")),
    (71, "assisted_push_up", Some("chest")),
    (72, "dumbbell_push_press", Some("shoulders")),
    (73, "dumbbell_upright_row", Some("shoulders")),
    (74, "neck_pulldown", Some("back")),
    (75, "chin_up", Some("back")),
    (76, "dumbbell_biceps_curl", Some("biceps")),
    (77, "t_bar_row", Some("back")),
    (78, "kettlebell_deadlift", Some("legs")),
    (79, "kneeling_push_up", Some("chest")),
    (80, "rotational_dumbbell_push_press", Some("shoulders")),
    (81, "incline_barbell_upright_row", Some("shoulders")),
    (82, "rope_straight_arm_pulldown", Some("back")),
    (83, "standard_pull_up", Some("back")),
    (84, "hammer_curl_to_press", Some("biceps")),
    (85, "seated_row_machine", Some("back")),
    (86, "chest_flyes", Some("chest")),
    (87, "single_arm_kettlebell_swing_right", Some("back")),
    (88, "barbell_push_press", Some("shoulders")),
    (89, "dumbbell_compound_push_press", Some("shoulders")),
    (90, "barbell_biceps_curl", Some("biceps")),
    (91, "single_arm_kettlebell_swing_left", Some("back")),
    (92, "v_grip_cable_row", Some("back")),
    (93, "dumbbell_front_raise_left", Some("shoulders")),
    (94, "dumbbell_front_raise_right", Some("shoulders")),
    (95, "bent_over_lateral_raise", Some("shoulders")),
    (96, "front_raise", Some("shoulders")),
    (97, "plank", Some("core")),
    (98, "barbell_hip_thrust", Some("glutes")),
    (99, "barbell_snatch", Some("legs")),
    (100, "standing_calf_raise", Some("legs")),
    (101, "barbell_shrug", Some("back")),
    (102, "dumbbell_shrug", Some("back")),
    (103, "farmers_walk_lunge", Some("legs")),
    (104, "v_up", Some("core")),
    (105, "lateral_slide", Some("legs")),
    (106, "battle_rope", Some("shoulders")),
    (107, "kettlebell_swing", Some("glutes")),
    (108, "walking_recovery", None),
    (109, "incline_dumbbell_bench_press", Some("chest")),
    (110, "incline_barbell_bench_press", Some("chest")),
    (111, "decline_dumbbell_bench_press", Some("chest")),
    (112, "decline_barbell_bench_press", Some("chest")),
    (113, "pec_deck_fly", Some("chest")),
    (114, "reverse_pec_deck_fly", Some("shoulders")),
    (115, "trx_triceps_press", Some("triceps")),
    (116, "trx_reverse_flys", Some("shoulders")),
    (117, "three_point_chest_push", Some("chest")),
    (118, "incline_cable_flys", Some("chest")),
    (119, "incline_cable_chest_press", Some("chest")),
    (120, "low_cable_crossover", Some("chest")),
    (121, "cable_crossover", Some("chest")),
    (122, "reverse_grip_triceps_pushdown", Some("triceps")),
    (123, "cable_crunch", Some("core")),
    (124, "smith_machine_incline_bench_press", Some("chest")),
    (125, "smith_machine_decline_bench_press", Some("chest")),
    (126, "chest_press_machine", Some("chest")),
    (127, "leverage_incline_chest_press", Some("chest")),
    (128, "leverage_decline_chest_press", Some("chest")),
    (129, "leverage_chest_press", Some("chest")),
    (130, "machine_shoulder_press", Some("shoulders")),
];

/// Looks up the standard exercise and primary muscle group for an `actionType`. Off-table codes (such as the new firmware code `1030`) return `(None, None)`.
pub fn resolve_strength_action(action_code: i32) -> (Option<&'static str>, Option<&'static str>) {
    match STRENGTH_ACTION_CATALOG.binary_search_by_key(&action_code, |&(code, _, _)| code) {
        Ok(idx) => {
            let (_, key, muscle) = STRENGTH_ACTION_CATALOG[idx];
            (Some(key), muscle)
        }
        Err(_) => (None, None),
    }
}

/// The stored form of one record from the summary stream `strength_training_group`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredStrengthGroupEntry {
    pub action_code: i32,
    pub reps: i32,
}

/// Strength summary taken from one record of the `workouts` (history) stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryStrengthSummary {
    pub total_sets: Option<i32>,
    pub total_reps: Option<i32>,
    pub rest_seconds: Option<i64>,
    pub groups_json: Option<String>,
}

/// Checks whether `lap` in the detail object is the per-set strength layout, and parses it into `StrengthSetRow`.
///
/// Running / cycling laps put the distance in `col[2]` and increasing elapsed seconds in `col[5]`.
/// Strength training rows have 62 columns (we must read at least up to `col[28]`), with `col[2] == 0`, `col[5] == 0`,
/// `col[21]` weight, `col[22]` reps, `col[23]` unit (0 = kg, 1 = lb), `col[28]` exercise code,
/// `col[43]` duration in ms (column meanings in #163).
///
/// The layout check (column count, zero distance and elapsed seconds) covers the whole payload: if one row does not look like strength, treat it as running laps and drop all of it.
/// When the payload has its own strength marker (`strengthAssess` / `strengthSets` / `type == 52`), distance and elapsed seconds are not checked.
/// Content is tolerant per row: bodyweight exercises write `-1` for weight and unit, timed exercises write `-1` for reps, and that only leaves that cell empty.
/// Rows with no exercise code, reps or weight (rest between sets) are skipped and do not count as a set.
pub fn parse_strength_sets(data: &serde_json::Map<String, Value>) -> Vec<StrengthSetRow> {
    let Some(text) = data.get("lap").and_then(Value::as_str).map(str::trim) else {
        return Vec::new();
    };
    if text.is_empty() {
        return Vec::new();
    }
    let has_strength_envelope = non_empty_strength_marker(data.get("strengthAssess"))
        || non_empty_strength_marker(data.get("strengthSets"))
        || data
            .get("type")
            .and_then(Value::as_i64)
            .is_some_and(|code| code == 52);

    let mut sets = Vec::new();
    let mut saw_positive_signal = has_strength_envelope;
    // If the payload has a strength marker but no set parses, log one reason, so diagnostic logs can locate new payload shapes.
    // Log only the reason, row number and column count. Never log values such as weight or reps.
    let reject = |reason: &str, row: usize, columns: usize| -> Vec<StrengthSetRow> {
        if has_strength_envelope {
            tracing::warn!("strength per-set data not parsed: {reason} (row {row}, {columns} columns)");
        }
        Vec::new()
    };
    let int = |cols: &[&str], at: usize| cols.get(at).and_then(|v| v.trim().parse::<i64>().ok());
    let float = |cols: &[&str], at: usize| {
        cols.get(at)
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite())
    };

    for (position, row) in text
        .split(';')
        .filter(|row| !row.trim().is_empty())
        .enumerate()
    {
        let cols: Vec<&str> = row.split(',').collect();
        if cols.len() < 29 {
            return reject("too few columns", position, cols.len());
        }
        if int(&cols, 0).is_none() {
            return reject("row number is not a number", position, cols.len());
        }
        if !has_strength_envelope {
            let distance_m = float(&cols, 2);
            let elapsed = int(&cols, 5);
            if distance_m.is_none_or(|d| d.abs() > 1e-6) || elapsed != Some(0) {
                return reject("distance or elapsed time is not zero", position, cols.len());
            }
        }

        let action_code = int(&cols, 28)
            .filter(|code| (0..=100_000).contains(code))
            .map(|code| code as i32);
        let reps = int(&cols, 22)
            .filter(|reps| (0..=10_000).contains(reps))
            .map(|reps| reps as i32);
        let unit_code = int(&cols, 23).filter(|unit| matches!(unit, 0 | 1));
        let weight_raw = float(&cols, 21)
            .filter(|w| *w > 0.0 && *w <= 2_000.0)
            .filter(|_| unit_code.is_some());
        let ms_raw = int(&cols, 43).unwrap_or(-1);
        let sec_int = int(&cols, 1).unwrap_or(-1);

        // Rest between sets: no exercise, no reps, no weight. Not a set.
        if action_code.is_none() && reps.unwrap_or(0) == 0 && weight_raw.is_none() {
            continue;
        }
        let action_code = action_code.unwrap_or(-1);
        let reps = reps.unwrap_or(0);
        if action_code > 0 || reps > 0 || weight_raw.is_some() || unit_code == Some(1) || ms_raw > 0
        {
            saw_positive_signal = true;
        }

        let weight_unit = if unit_code == Some(1) { "lb" } else { "kg" }.to_string();
        let (weight_value, weight_kg) = match weight_raw {
            Some(raw) => {
                let kg = if unit_code == Some(1) {
                    round3(raw * LB_TO_KG)
                } else {
                    round3(raw)
                };
                (Some(round2(raw)), Some(kg))
            }
            None => (None, None),
        };

        let duration_ms = if ms_raw > 0 && ms_raw <= 3_600_000 {
            Some(ms_raw)
        } else if sec_int > 0 && sec_int <= 3_600 {
            Some(sec_int * 1000)
        } else {
            None
        };

        let avg_hr = int(&cols, 4)
            .filter(|hr| (1..=250).contains(hr))
            .map(|hr| hr as i32);
        let calories_kcal = float(&cols, 15)
            .filter(|val| *val >= 0.0 && *val <= 5_000.0)
            .map(round2);

        let (exercise_key, muscle_group) = resolve_strength_action(action_code);
        sets.push(StrengthSetRow {
            set_index: sets.len() as i32 + 1,
            action_code,
            exercise_key: exercise_key.map(str::to_string),
            muscle_group: muscle_group.map(str::to_string),
            reps,
            weight_value,
            weight_unit,
            weight_kg,
            duration_ms,
            avg_hr,
            calories_kcal,
            exercise_name: None,
            secondary_muscle_groups: Vec::new(),
        });
    }

    if sets.is_empty() && has_strength_envelope {
        tracing::warn!("strength per-set data not parsed: every row is an empty rest between sets");
    }
    if saw_positive_signal {
        sets
    } else {
        Vec::new()
    }
}

fn non_empty_strength_marker(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            !trimmed.is_empty() && trimmed != "[]" && trimmed != "null"
        }
        _ => false,
    }
}

/// Parses `strength_training_group` from history (accepts a JSON string or a JSON array).
pub fn parse_history_strength_groups(value: Option<&Value>) -> Vec<StoredStrengthGroupEntry> {
    let parsed_array: Option<Vec<Value>> = match value {
        Some(Value::Array(items)) => Some(items.clone()),
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                serde_json::from_str::<Vec<Value>>(trimmed).ok()
            }
        }
        _ => None,
    };
    let Some(items) = parsed_array else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let Some(action_code) = obj
            .get("actionType")
            .or_else(|| obj.get("action_type"))
            .and_then(as_i32)
            .filter(|code| (0..=100_000).contains(code))
        else {
            continue;
        };
        let reps = obj
            .get("count")
            .or_else(|| obj.get("reps"))
            .and_then(as_i32)
            .unwrap_or(0)
            .max(0);
        out.push(StoredStrengthGroupEntry { action_code, reps });
    }
    out
}

fn as_i32(value: &Value) -> Option<i32> {
    value
        .as_i64()
        .and_then(|v| i32::try_from(v).ok())
        .or_else(|| {
            value
                .as_f64()
                .filter(|v| v.is_finite())
                .map(|v| v.round() as i32)
        })
        .or_else(|| value.as_str().and_then(|s| s.trim().parse::<i32>().ok()))
}

/// Builds the public `StrengthWorkoutDetail`.
///
/// When per-set `sets` exist, they are the source of truth for total sets, total reps, total volume and per-exercise aggregates.
/// When `detail` is not fetched yet, fall back to the summary fields in the `workouts` table. Never invent weight or per-set duration.
pub fn build_strength_workout_detail(
    sets: Vec<StrengthSetRow>,
    is_strength_workout: bool,
    history_total_sets: Option<i32>,
    history_total_reps: Option<i32>,
    rest_seconds: Option<i64>,
    history_groups_json: Option<&str>,
) -> Option<StrengthWorkoutDetail> {
    if !sets.is_empty() {
        let total_sets = sets.len() as i32;
        let total_reps: i32 = sets.iter().map(|s| s.reps.max(0)).sum();
        let mut volume_sum = 0.0f64;
        let mut has_weighted = false;
        for set in &sets {
            if set.reps > 0 {
                if let Some(kg) = set.weight_kg.filter(|kg| *kg > 0.0) {
                    volume_sum += (set.reps as f64) * kg;
                    has_weighted = true;
                }
            }
        }
        let exercise_groups = aggregate_groups(sets.iter().map(|s| (s.action_code, s.reps)));
        return Some(StrengthWorkoutDetail {
            total_sets,
            total_reps,
            total_volume_kg: has_weighted.then(|| round2(volume_sum)),
            rest_seconds: rest_seconds.filter(|s| *s >= 0),
            sets,
            exercise_groups,
        });
    }

    if !is_strength_workout {
        return None;
    }

    let stored_groups: Vec<StoredStrengthGroupEntry> = history_groups_json
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_default();
    let exercise_groups = aggregate_groups(stored_groups.iter().map(|g| (g.action_code, g.reps)));
    let derived_sets = (!stored_groups.is_empty()).then_some(stored_groups.len() as i32);
    let derived_reps =
        (!stored_groups.is_empty()).then(|| stored_groups.iter().map(|g| g.reps.max(0)).sum());

    let total_sets = history_total_sets
        .filter(|n| *n > 0)
        .or(derived_sets)
        .unwrap_or(0);
    let total_reps = history_total_reps
        .filter(|n| *n >= 0)
        .or(derived_reps)
        .unwrap_or(0);
    let rest_seconds = rest_seconds.filter(|s| *s >= 0);

    if total_sets == 0 && exercise_groups.is_empty() && rest_seconds.is_none() {
        return None;
    }

    Some(StrengthWorkoutDetail {
        total_sets,
        total_reps,
        total_volume_kg: None,
        rest_seconds,
        sets: Vec::new(),
        exercise_groups,
    })
}

fn aggregate_groups(items: impl IntoIterator<Item = (i32, i32)>) -> Vec<StrengthExerciseGroup> {
    let mut out: Vec<StrengthExerciseGroup> = Vec::new();
    for (action_code, reps) in items {
        if let Some(existing) = out.iter_mut().find(|g| g.action_code == action_code) {
            existing.sets += 1;
            existing.total_reps += reps.max(0);
        } else {
            let (exercise_key, muscle_group) = resolve_strength_action(action_code);
            out.push(StrengthExerciseGroup {
                action_code,
                exercise_key: exercise_key.map(str::to_string),
                muscle_group: muscle_group.map(str::to_string),
                sets: 1,
                total_reps: reps.max(0),
                exercise_name: None,
                secondary_muscle_groups: Vec::new(),
            });
        }
    }
    out
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

#[cfg(test)]
#[path = "strength_tests.rs"]
mod tests;
