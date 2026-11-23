//! Zebra strap tools: data that comes straight from the Helio Strap over Bluetooth and is kept
//! in `<data dir>/strap/` by the Zebra app (not in the database).

use super::*;

fn strap_dir(db: &Database) -> Result<std::path::PathBuf, CallFailure> {
    db.data_dir()
        .map(|d| d.join("strap"))
        .ok_or_else(|| CallFailure::plain("Could not find the local data folder."))
}

/// Heart rate saved from Zebra's always-on Bluetooth link, about one reading a second,
/// returned as one-minute averages.
pub(super) fn run_strap_heart_rate(db: &Database, args: &Value) -> Result<Value, CallFailure> {
    let minutes = args.get("minutes").and_then(Value::as_i64).unwrap_or(60).clamp(1, 10_080);
    let path = strap_dir(db)?.join("live-hr.jsonl");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let cut = chrono::Utc::now().timestamp_millis() - minutes * 60_000;
    let mut buckets: BTreeMap<i64, (i64, i64, i64, i64)> = BTreeMap::new();
    let mut count = 0_i64;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let (Some(t), Some(bpm)) = (v.get("t").and_then(Value::as_i64), v.get("bpm").and_then(Value::as_i64)) else { continue };
        if t < cut || !(25..=230).contains(&bpm) {
            continue;
        }
        count += 1;
        let e = buckets.entry(t / 60_000 * 60_000).or_insert((0, 0, i64::MAX, i64::MIN));
        e.0 += bpm;
        e.1 += 1;
        e.2 = e.2.min(bpm);
        e.3 = e.3.max(bpm);
    }
    let points: Vec<Value> = buckets
        .iter()
        .map(|(t, (sum, n, lo, hi))| {
            let at = chrono::DateTime::from_timestamp_millis(*t)
                .map(|d| d.with_timezone(&Local).format("%Y-%m-%dT%H:%M").to_string())
                .unwrap_or_default();
            json!({ "minute": at, "avgBpm": (*sum as f64 / *n as f64 * 10.0).round() / 10.0, "minBpm": lo, "maxBpm": hi, "readings": n })
        })
        .collect();
    let all: Vec<i64> = buckets.values().flat_map(|(_, _, lo, hi)| [*lo, *hi]).collect();
    Ok(json!({
        "source": "Helio Strap, live Bluetooth heart rate saved by the Zebra app",
        "windowMinutes": minutes,
        "readings": count,
        "minutesCovered": points.len(),
        "lowestBpm": all.iter().min(),
        "highestBpm": all.iter().max(),
        "perMinute": points,
        "note": if count == 0 { "No live readings in this window. Zebra saves them only while the app is open and the strap is in range." } else { "Gaps mean the app was closed or the strap was out of range." },
    }))
}

/// Health-watch alerts and the illness early-warning state written by Zebra's hourly check.
pub(super) fn run_strap_alerts(db: &Database) -> Result<Value, CallFailure> {
    let path = strap_dir(db)?.join("alerts.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| CallFailure::plain(format!("alerts.json is unreadable: {e}"))),
        Err(_) => Ok(json!({ "alerts": [], "illness": null, "note": "No health-watch results yet. Set up the strap client and let Zebra sync." })),
    }
}

pub(super) fn strap_tool_definitions() -> Vec<Value> {
    let missing = contract::MISSING_VALUE_CONVENTION;
    vec![
        json!({
            "name": "get_strap_heart_rate",
            "description": format!("Heart rate straight from the Helio Strap over Bluetooth, saved by the Zebra app about once a second while it runs. Returns one-minute averages in bpm (with min, max and reading count) plus the lowest and highest values. Much finer than the cloud heart rate. Use it for 'what was my heart rate this afternoon' or to see effort during a session. {missing}"),
            "inputSchema": {
                "type": "object",
                "properties": {
                    "minutes": { "type": "integer", "minimum": 1, "maximum": 10080, "default": 60, "description": "How many minutes back from now (up to 7 days)." }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "get_strap_alerts",
            "description": format!("Zebra's health watch: alerts raised from the strap's data (unusual resting heart rate in bpm, HRV in ms, skin temperature in °C, breathing in breaths a minute, blood oxygen in %) and the illness early-warning state with its baseline. Checked every hour. Not a diagnosis. {missing}"),
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
    ]
}
