//! The full track attached when the export goes to an AI (user feedback: "Detailed" + "Exact route" did not give all GPS points).
//!
//! To fit the AI context, the `.md` samples the track at 10 / 30 / 60 seconds and drops it whole when over budget. That is correct,
//! but a person who wants the AI to see the full route had no option. So with "Exact route" on, a GPX file is written next to the `.md`:
//! every decoded point is in it (same writer as "Export GPX", WGS-84, split at pauses). It goes on the clipboard with the `.md`.
//! It is not written when exact route is off, the level is not "Detailed", or there are no points.

use crate::export_formats::to_gpx;
use serde_json::{json, Value};

/// File name of the attached track: same name as the `.md` plus `-route`. The command layer uses this to find it next to the `.md`.
pub fn route_file_name(md_name: &str) -> String {
    format!(
        "{}-route.gpx",
        md_name.strip_suffix(".md").unwrap_or(md_name)
    )
}

/// Builds GPX from `workouts[].series.route` in the task document. Returns `(XML, point count)`. `None` when there are no points.
pub(super) fn route_gpx(document: &Value) -> Option<(String, usize)> {
    let workouts: Vec<Value> = document
        .get("workouts")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|workout| {
            let series = workout.get("series")?;
            let route = series.get("route").and_then(Value::as_array)?;
            if route.is_empty() {
                return None;
            }
            Some(json!({
                "workout_id": workout.get("workout_id"),
                "effective_type": workout.get("effective_type").or_else(|| workout.get("workout_type")),
                "start_time": workout.get("start_time"),
                "route": route,
                "samples": series.get("samples"),
                "pauses": series.get("pauses"),
            }))
        })
        .collect();
    if workouts.is_empty() {
        return None;
    }
    let export = json!({
        "generated_at": document.get("generated_at"),
        "data": { "workouts": workouts },
    });
    to_gpx(&export).ok()
}

/// The last line of the `.md`: tells the AI which attachment holds the full track. The section title is not translated, like other machine names in the `.md`.
pub(super) fn route_note(file_name: &str, points: usize) -> String {
    format!(
        "\n## Full route\n\n_All {points} GPS points of the route(s) above are in the attached file `{file_name}` (GPX 1.1, WGS-84). The route table above is thinned to fit the size budget._\n"
    )
}

#[cfg(test)]
#[path = "route_gpx_tests.rs"]
mod tests;
