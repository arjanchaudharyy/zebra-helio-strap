//! Write tools for the local AI: add and edit history (injuries, life events, races, shoes), workout feel, and type correction.
//!
//! Rules (`guide.md` also tells the AI):
//!
//! - Write only the local library. Validation uses the same save functions as the desktop app; MCP has no copy of its own;
//! - Every write goes into the change ledger `mcp_writes`. The user undoes each one in ZeppBridge "Settings -> Local AI -> Recent AI changes";
//! - **No delete**: "healed" and "retired" are fields (injury `endDate`, shoe `retiredOn`);
//! - When you edit an existing entry, send only the fields to change. The rest keep their current values;
//! - Denied as a whole in task scope (`whole_library`).

use super::*;
use zeppbridge_core::storage::mcp_writes::McpChange;
use zeppbridge_core::training_plan::standalone;

/// Names of the write tools. `build_request` marks all of them `whole_library`.
pub(super) const WRITE_TOOLS: [&str; 6] = [
    "save_injury",
    "save_life_event",
    "save_race",
    "save_shoe",
    "annotate_workout",
    "set_workout_type",
];

const UNDO_NOTE: &str = "Written to the local library and recorded in the change ledger: the user can undo this write in ZeppBridge \"Settings -> Local AI -> Recent AI changes\". You have no delete tool.";

/// Argument name (camelCase) -> key in the storage struct.
type FieldMap = &'static [(&'static str, &'static str)];

const INJURY_FIELDS: FieldMap = &[
    ("title", "title"),
    ("bodyPart", "body_part"),
    ("severity", "severity"),
    ("startDate", "start_date"),
    ("endDate", "end_date"),
    ("notes", "notes"),
];
const LIFE_EVENT_FIELDS: FieldMap = &[
    ("title", "title"),
    ("category", "category"),
    ("startDate", "startDate"),
    ("endDate", "endDate"),
    ("notes", "notes"),
    ("epoch", "epoch"),
];
const RACE_FIELDS: FieldMap = &[
    ("name", "name"),
    ("raceDate", "race_date"),
    ("sport", "sport"),
    ("distanceM", "distance_m"),
    ("goalSeconds", "goal_seconds"),
    ("priority", "priority"),
    ("resultSeconds", "result_seconds"),
    ("workoutId", "workout_id"),
    ("notes", "notes"),
];
const SHOE_FIELDS: FieldMap = &[
    ("name", "name"),
    ("sport", "sport"),
    ("isDefault", "is_default"),
    ("startKm", "start_km"),
    ("retireKm", "retire_km"),
    ("startedOn", "started_on"),
    ("retiredOn", "retired_on"),
    ("notes", "notes"),
];

fn invalid(message: impl Into<String>) -> zeppbridge_core::models::ZeppBridgeError {
    zeppbridge_core::models::ZeppBridgeError::ConfigError(message.into())
}

/// Lays the arguments over a base (defaults for a new entry, the current state for an edit), then hands it to the storage struct to deserialize.
fn overlay(mut base: Value, args: &Value, fields: FieldMap) -> Value {
    for (arg, key) in fields {
        if let Some(value) = args.get(*arg) {
            base[*key] = value.clone();
        }
    }
    base
}

fn record_id(args: &Value) -> Result<Option<i64>, CallFailure> {
    match args.get("id") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .filter(|id| *id > 0)
            .map(Some)
            .ok_or_else(|| CallFailure::plain("id must be a positive integer (omit it for a new entry)")),
    }
}

/// This crate does not depend on serde directly, so generic bounds cannot name `Deserialize` / `Serialize`. A macro does the job.
macro_rules! parse {
    ($value:expr, $what:expr) => {
        serde_json::from_value($value)
            .map_err(|error| invalid(format!("{} has wrong fields: {error}", $what)))
    };
}

macro_rules! to_json {
    ($value:expr) => {
        serde_json::to_value($value).map_err(|error| invalid(error.to_string()))
    };
}

fn dir(db: &Database) -> Result<std::path::PathBuf, CallFailure> {
    db.data_dir()
        .ok_or_else(|| CallFailure::plain("Cannot find the local data directory, so nothing can be written."))
}

fn db_failure(error: zeppbridge_core::models::ZeppBridgeError) -> CallFailure {
    CallFailure::coded(error.code(), error.user_message())
}

/// Shared flow for the four history ledgers: base -> lay arguments -> save -> record.
fn save_book(
    db: &Database,
    tool: &'static str,
    args: &Value,
    fields: FieldMap,
    defaults: Value,
    current: impl Fn(&Database, i64) -> zeppbridge_core::models::error::Result<Option<Value>>,
    build: impl Fn(Value) -> zeppbridge_core::models::error::Result<McpChange>,
) -> Result<Value, CallFailure> {
    let id = record_id(args)?;
    let client = protocol::client_name();
    let applied = standalone::write_once(&dir(db)?, |db| {
        let base = match id {
            Some(id) => {
                let mut base = current(db, id)?
                    .ok_or_else(|| invalid(format!("No entry with id {id} exists")))?;
                base["id"] = json!(id);
                base
            }
            None => defaults.clone(),
        };
        let change = build(overlay(base, args, fields))?;
        db.apply_mcp_change(tool, change, client.as_deref())
    })
    .map_err(db_failure)?;
    Ok(json!({
        "applied": applied,
        "note": UNDO_NOTE,
        "missingValues": contract::MISSING_VALUE_CONVENTION,
    }))
}

pub(super) fn run_write(db: &Database, name: &str, args: &Value) -> Result<Value, CallFailure> {
    match name {
        "save_injury" => save_book(
            db,
            "save_injury",
            args,
            INJURY_FIELDS,
            json!({ "id": null, "notes": "" }),
            |db, id| {
                db.list_injuries()?
                    .into_iter()
                    .find(|row| row.input.id == Some(id))
                    .map(|row| to_json!(&row.input))
                    .transpose()
            },
            |value| Ok(McpChange::Injury(parse!(value, "injury")?)),
        ),
        "save_life_event" => save_book(
            db,
            "save_life_event",
            args,
            LIFE_EVENT_FIELDS,
            json!({ "id": null, "notes": "", "endDate": null, "epoch": false }),
            |db, id| {
                db.list_life_events(None, None)?
                    .into_iter()
                    .find(|row| row.input.id == Some(id))
                    .map(|row| to_json!(&row.input))
                    .transpose()
            },
            |value| Ok(McpChange::LifeEvent(parse!(value, "life event")?)),
        ),
        "save_race" => save_book(
            db,
            "save_race",
            args,
            RACE_FIELDS,
            json!({ "id": null, "notes": "", "priority": "b" }),
            |db, id| {
                db.list_races()?
                    .into_iter()
                    .find(|row| row.input.id == Some(id))
                    .map(|row| to_json!(&row.input))
                    .transpose()
            },
            |value| Ok(McpChange::Race(parse!(value, "race")?)),
        ),
        "save_shoe" => save_book(
            db,
            "save_shoe",
            args,
            SHOE_FIELDS,
            json!({
                "id": null, "notes": "", "is_default": false, "start_km": 0.0,
                "retire_km": zeppbridge_core::storage::logbook::shoes::DEFAULT_RETIRE_KM,
            }),
            |db, id| {
                db.list_shoe_rows()?
                    .into_iter()
                    .find(|row| row.input.id == Some(id))
                    .map(|row| to_json!(&row.input))
                    .transpose()
            },
            |value| Ok(McpChange::Shoe(parse!(value, "shoe")?)),
        ),
        "annotate_workout" => {
            let workout_id = workout_arg(args)?;
            let rpe = match args.get("rpe") {
                None | Some(Value::Null) => None,
                Some(value) => Some(
                    value
                        .as_u64()
                        .filter(|rpe| (1..=10).contains(rpe))
                        .ok_or_else(|| CallFailure::plain("rpe must be an integer 1-10, or null to clear it"))?
                        as u8,
                ),
            };
            let soreness: Vec<String> = match args.get("soreness") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| item.as_str().map(str::to_string))
                    .collect::<Option<_>>()
                    .ok_or_else(|| CallFailure::plain("soreness is an array of body part codes"))?,
                Some(_) => return Err(CallFailure::plain("soreness is an array of body part codes")),
            };
            write_change(
                db,
                "annotate_workout",
                McpChange::WorkoutFeel {
                    workout_id,
                    rpe,
                    soreness,
                },
            )
        }
        "set_workout_type" => {
            let workout_id = workout_arg(args)?;
            let workout_type = match args.get("type") {
                Some(Value::String(kind)) if !kind.trim().is_empty() => Some(kind.clone()),
                Some(Value::Null) => None,
                _ => return Err(CallFailure::plain(
                    "type must be a workout type code (for example run, trail_running, treadmill), or null to remove the correction",
                )),
            };
            write_change(
                db,
                "set_workout_type",
                McpChange::WorkoutType {
                    workout_id,
                    workout_type,
                },
            )
        }
        other => Err(CallFailure::plain(format!("There is no tool named {other}."))),
    }
}

fn workout_arg(args: &Value) -> Result<String, CallFailure> {
    args.get("workoutId")
        .and_then(Value::as_str)
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| CallFailure::plain("missing workoutId"))
}

fn write_change(db: &Database, tool: &str, change: McpChange) -> Result<Value, CallFailure> {
    let client = protocol::client_name();
    let applied = standalone::write_once(&dir(db)?, |db| {
        db.apply_mcp_change(tool, change, client.as_deref())
    })
    .map_err(db_failure)?;
    Ok(json!({
        "applied": applied,
        "note": UNDO_NOTE,
        "missingValues": contract::MISSING_VALUE_CONVENTION,
    }))
}

/// Coach profile notes are saved at once (no longer through the inbox), recorded the same way, and can be undone.
pub(super) fn run_remember_note(db: &Database, args: &Value) -> Result<Value, CallFailure> {
    let text = args
        .get("note")
        .and_then(Value::as_str)
        .ok_or_else(|| CallFailure::plain("missing note"))?
        .to_string();
    let client = protocol::client_name();
    let applied = standalone::write_once(&dir(db)?, |db| {
        db.apply_mcp_change(
            "propose_coach_note",
            McpChange::CoachNote(text),
            client.as_deref(),
        )
    })
    .map_err(db_failure)?;
    let next = match applied.note {
        Some(zeppbridge_core::storage::coach_notes::Remembered::Remembered { .. }) => {
            "Remembered: it shows up in coachNotes of get_athlete_profile right away. The user can undo or rewrite it in ZeppBridge."
        }
        Some(zeppbridge_core::storage::coach_notes::Remembered::AlreadyKnown { .. }) => {
            "The user already has this note. Do not write it again."
        }
        _ => "The user rejected or undid this note before. Do not write it again, and do not write it in other words.",
    };
    Ok(json!({
        "applied": applied,
        "next": next,
        "missingValues": contract::MISSING_VALUE_CONVENTION,
    }))
}
