//! Declarations of the write tools and the two actions. They match the tool tables in `docs/reference/cli-and-mcp*.md`
//! and `guide.md` one to one (tests guard this).

use super::*;

const UNDO: &str = "Every write is recorded in the change ledger, and the user can undo each one in ZeppBridge \"Settings -> Local AI -> Recent AI changes\". There is no delete.";

fn writer() -> Value {
    json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false })
}

fn id_field(what: &str) -> Value {
    json!({ "type": "integer", "minimum": 1, "description": format!("The id of the {what} to edit (listed in get_athlete_profile). Omit it for a new entry. When editing, send only the fields to change; the rest keep their current values.") })
}

fn date(description: &str) -> Value {
    json!({ "type": ["string", "null"], "format": "date", "description": description })
}

pub(super) fn write_tool_definitions() -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "save_injury",
            "description": format!(
                "Add or edit one injury (history). When it heals, set endDate to the day it healed; do not delete it. Use body part codes for the location \
                 (knee_l, knee_r, calf_l, lower_back ... left and right are separate). severity: 1 mild / 2 moderate / 3 severe (affects training). \
                 Record only injuries the user told you about; do not infer them from data. {UNDO}"
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": id_field("injury"),
                    "title": { "type": "string", "maxLength": 120 },
                    "bodyPart": { "type": ["string", "null"] },
                    "severity": { "type": ["integer", "null"], "minimum": 1, "maximum": 3 },
                    "startDate": date("Local start day, YYYY-MM-DD."),
                    "endDate": date("Local day it healed; null if not healed yet."),
                    "notes": { "type": "string", "maxLength": 2000 }
                },
                "additionalProperties": false
            },
            "annotations": writer()
        }),
        json!({
            "name": "save_life_event",
            "description": format!(
                "Add or edit one life event (business trip, illness, move, race build-up ...). Charts and the context handed to the AI mark it. \
                 category must be one of health / travel / routine / training / other; epoch=true means this is a named stretch of days (an epoch). \
                 endDate null means it is still ongoing; for a one-day event, endDate equals startDate. {UNDO}"
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": id_field("life event"),
                    "title": { "type": "string", "maxLength": 120 },
                    "category": { "type": "string", "enum": ["health", "travel", "routine", "training", "other"] },
                    "startDate": date("Local start day, YYYY-MM-DD."),
                    "endDate": date("Local end day; null if still ongoing."),
                    "notes": { "type": "string", "maxLength": 4000 },
                    "epoch": { "type": "boolean" }
                },
                "additionalProperties": false
            },
            "annotations": writer()
        }),
        json!({
            "name": "save_race",
            "description": format!(
                "Add or edit one race: one the user signed up for (date, distance, goal) or one already finished (fill resultSeconds for the medal wall). \
                 sport is running / trail / cycling / swimming / triathlon / other; priority is a (main goal) / b / c (practice race). \
                 Distance in meters, time in seconds. The result is the official result; do not guess it from workout records. {UNDO}"
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": id_field("race"),
                    "name": { "type": "string" },
                    "raceDate": date("Local race day, YYYY-MM-DD."),
                    "sport": { "type": "string", "enum": ["running", "trail", "cycling", "swimming", "triathlon", "other"] },
                    "distanceM": { "type": ["number", "null"], "description": "Meters." },
                    "goalSeconds": { "type": ["integer", "null"], "description": "Goal time, seconds." },
                    "priority": { "type": "string", "enum": ["a", "b", "c"] },
                    "resultSeconds": { "type": ["integer", "null"], "description": "Official result, seconds." },
                    "workoutId": { "type": ["string", "null"], "description": "The workout recognized as this race." },
                    "notes": { "type": "string" }
                },
                "additionalProperties": false
            },
            "annotations": writer()
        }),
        json!({
            "name": "save_shoe",
            "description": format!(
                "Add or edit one pair of shoes. sport is running / trail / walking / hiking; retireKm is the retirement reminder distance (default 700 km), \
                 startKm is the kilometers already worn before tracking. When retired, set retiredOn to that day; do not delete it. {UNDO}"
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": id_field("shoe"),
                    "name": { "type": "string" },
                    "sport": { "type": "string", "enum": ["running", "trail", "walking", "hiking"] },
                    "isDefault": { "type": "boolean", "description": "Make this the default shoe for this sport." },
                    "startKm": { "type": "number", "minimum": 0, "description": "Kilometers." },
                    "retireKm": { "type": "number", "description": "Kilometers." },
                    "startedOn": date("Local day the shoe was first worn, YYYY-MM-DD."),
                    "retiredOn": date("Local retirement day; null if still in use."),
                    "notes": { "type": "string" }
                },
                "additionalProperties": false
            },
            "annotations": writer()
        }),
        json!({
            "name": "annotate_workout",
            "description": format!(
                "Record the user's own feel for one workout: perceived exertion rpe (1-10) and sore body parts soreness (array of body part codes). \
                 It replaces these two fields as a whole (send null / [] to clear them); the RPE the watch recorded is not affected, and the shoe choice does not change. \
                 Record only the feel the user told you. {UNDO}"
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workoutId": { "type": "string" },
                    "rpe": { "type": ["integer", "null"], "minimum": 1, "maximum": 10 },
                    "soreness": { "type": ["array", "null"], "items": { "type": "string" }, "maxItems": 20 }
                },
                "required": ["workoutId"],
                "additionalProperties": false
            },
            "annotations": writer()
        }),
        json!({
            "name": "set_workout_type",
            "description": format!(
                "Correct the type of one workout (for example, the watch recorded a run but it was a trail run). It uses the same path as \"Change type\" on the desktop, \
                 and the original cloud record does not change. type is a workout type code (run, trail_running, treadmill, walking, hiking ...); \
                 null removes the correction and goes back to the type the watch recorded. {UNDO}"
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workoutId": { "type": "string" },
                    "type": { "type": ["string", "null"] }
                },
                "required": ["workoutId", "type"],
                "additionalProperties": false
            },
            "annotations": writer()
        }),
        json!({
            "name": "sync_now",
            "description": "Make ZeppBridge pull new data from the Zepp cloud once (the same path as zeppbridge-cli sync in scheduled tasks). \
                 state: done / running (still running in the background; check later with get_data_health) / deferred (this computer is organizing data; not a failure) / \
                 busy (the desktop app is syncing) / not_configured / cloud_error / incomplete / failed. \
                 It is normal that the cloud has nothing new: the watch must send its data to the Zepp App first. Do not start it again and again. It does not change records the user wrote and does not go into the change ledger.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": true }
        }),
        json!({
            "name": "export_data",
            "description": format!(
                "Export a range of days (at most 366 days) to a JSON file in the exports folder of the ZeppBridge data directory on the user's computer, \
                 and return the file name (relative to the data directory) and the counts. Summary only, no workout tracks. types defaults to workouts, daily_activity, sleep; \
                 options: {}. Use it only when the user wants a file; to look at data, use the read tools directly. It does not go into the change ledger.",
                zeppbridge_core::storage::EXPORT_DATA_TYPES.join(", ")
            ),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "startDate": { "type": "string", "format": "date" },
                    "endDate": { "type": "string", "format": "date" },
                    "types": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["startDate", "endDate"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false }
        }),
    ];
    // Records read back also have missing values: state the rule the same way as the read tools.
    for tool in &mut tools {
        let text = format!(
            "{} {}",
            tool["description"].as_str().unwrap_or_default(),
            contract::MISSING_VALUE_CONVENTION
        );
        tool["description"] = json!(text);
    }
    tools
}
