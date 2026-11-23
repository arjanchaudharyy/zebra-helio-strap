//! Validated workout -> official Workout V2 payload.
//!
//! Field names and enum values are copied word for word from `devopen.zepp.com/docs/data-apis/workout-v2-schema`.
//! Units follow the 2026-10-06 tests (see [`super::VERIFIED`]): distance is meters, pace is speed (m/s).
//!
//! The first line of the description (`description`) is "purpose · sub-type". The notes start on the second line,
//! split by newlines (R1 tests show correct line breaks on phone and watch). On the watch, a third-party plan only has the main type
//! and asks the user to pick a sub-type again; this line tells the user which one. This is **content** sent to the watch, not UI text,
//! so it uses the UI language at publish time ([`WatchLocale`]) and has no code. The Chinese labels stay as Unicode escapes.

use super::window::Window;
use super::{Intensity, Sport, Step, StepLength, StepNode, Target, Variant, Workout};
use serde_json::{json, Value};

/// The language for the sub-type text in the first description line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WatchLocale {
    #[default]
    Zh,
    En,
    Es,
}

impl WatchLocale {
    /// UI language tag (`zh-CN`, `en`, `es-ES`, ...) -> one of these three; other languages use English.
    pub fn from_tag(tag: &str) -> Self {
        let tag = tag.trim().to_ascii_lowercase();
        if tag.starts_with("zh") {
            Self::Zh
        } else if tag.starts_with("es") {
            Self::Es
        } else {
            Self::En
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
            Self::Es => "es",
        }
    }
}

/// The sub-type name in the watch description, as close as possible to the item the watch asks the user to pick.
pub fn variant_label(sport: Sport, variant: Variant, locale: WatchLocale) -> Option<&'static str> {
    use WatchLocale::{En, Es, Zh};
    Some(match (sport, variant, locale) {
        (Sport::Running, Variant::Outdoor, Zh) => "\u{6237}\u{5916}\u{8dd1}", // "Outdoor run"
        (Sport::Running, Variant::Outdoor, En) => "Outdoor run",
        (Sport::Running, Variant::Outdoor, Es) => "Carrera al aire libre",
        (Sport::Running, Variant::Treadmill, Zh) => "\u{8dd1}\u{6b65}\u{673a}", // "Treadmill"
        (Sport::Running, Variant::Treadmill, En) => "Treadmill",
        (Sport::Running, Variant::Treadmill, Es) => "Cinta de correr",
        (Sport::Running, Variant::Track, Zh) => "\u{64cd}\u{573a}\u{8dd1}\u{6b65}", // "Track run"
        (Sport::Running, Variant::Track, En) => "Track run",
        (Sport::Running, Variant::Track, Es) => "Pista",
        (Sport::Cycling, Variant::Outdoor, Zh) => "\u{6237}\u{5916}\u{9a91}\u{884c}", // "Outdoor cycling"
        (Sport::Cycling, Variant::Outdoor, En) => "Outdoor cycling",
        (Sport::Cycling, Variant::Outdoor, Es) => "Ciclismo al aire libre",
        (Sport::Cycling, Variant::Indoor, Zh) => "\u{5ba4}\u{5185}\u{9a91}\u{884c}", // "Indoor cycling"
        (Sport::Cycling, Variant::Indoor, En) => "Indoor cycling",
        (Sport::Cycling, Variant::Indoor, Es) => "Ciclismo en interior",
        // Third-party plans on the watch have no "trail run" choice, so it can only start as a run (#143). The label gives the main type first,
        // so nobody looks for a sub-type that does not exist on the watch.
        (Sport::Running, Variant::Trail, Zh) => "\u{8dd1}\u{6b65} · \u{8d8a}\u{91ce}\u{8def}\u{7ebf}", // "Run · trail route"
        (Sport::Running, Variant::Trail, En) => "Run · trail",
        (Sport::Running, Variant::Trail, Es) => "Carrera · montaña",
        (Sport::Cycling, Variant::Mountain, Zh) => "\u{5c71}\u{5730}\u{9a91}\u{884c}", // "Mountain cycling"
        (Sport::Cycling, Variant::Mountain, En) => "Mountain biking",
        (Sport::Cycling, Variant::Mountain, Es) => "Ciclismo de montaña",
        _ => return None,
    })
}

/// When the watch has no such sub-type to pick, tell the user how to start it on the watch (#143).
fn variant_hint(variant: Variant, locale: WatchLocale) -> Option<&'static str> {
    use WatchLocale::{En, Es, Zh};
    Some(match (variant, locale) {
        (Variant::Trail, Zh) => "\u{624b}\u{8868}\u{4e0a}\u{9009}\u{8dd1}\u{6b65}\u{5f00}\u{59cb}", // "Pick run on the watch to start"
        (Variant::Trail, En) => "start it as a run on the watch",
        (Variant::Trail, Es) => "en el reloj, empieza una carrera",
        _ => return None,
    })
}

/// The watch description: first line "purpose · sub-type" (a missing part is left out), notes from the second line.
/// For a sub-type the watch cannot pick (trail run), the first line adds a half sentence on how to start.
/// Workouts in the ledger from before 2026-10 have no purpose or sub-type, so they only have notes.
pub fn watch_description(workout: &Workout, locale: WatchLocale) -> Option<String> {
    let variant = workout.variant.and_then(|variant| {
        let label = variant_label(workout.sport, variant, locale)?;
        Some(match variant_hint(variant, locale) {
            Some(hint) if locale == WatchLocale::Zh => format!("{label}\u{ff0c}{hint}"), // full-width comma
            Some(hint) => format!("{label}, {hint}"),
            None => label.to_string(),
        })
    });
    let head = match (workout.focus.as_deref(), variant) {
        (Some(focus), Some(variant)) => Some(format!("{focus} · {variant}")),
        (Some(focus), None) => Some(focus.to_string()),
        (None, Some(variant)) => Some(variant.to_string()),
        (None, None) => None,
    };
    match (head, workout.description.as_deref()) {
        (Some(head), Some(body)) => Some(format!("{head}\n{body}")),
        (Some(head), None) => Some(head),
        (None, Some(body)) => Some(body.to_string()),
        (None, None) => None,
    }
}

/// Pace (seconds per km) -> speed (m/s), rounded to two decimals. Tested in R3 and R4: the `PACE_LAP` value is
/// speed; 3.03-3.33 shows 5'00"-5'30"/km.
pub fn pace_to_speed(seconds_per_km: u16) -> f64 {
    (1000.0 / f64::from(seconds_per_km.max(1)) * 100.0).round() / 100.0
}

/// A workout that already has a number in the ledger. `id` is the V2 `workoutId` (it must be unique on the third-party platform).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberedWorkout {
    pub id: i64,
    pub workout: Workout,
}

/// The request body for a full window. It only holds workouts inside the window, sorted by date and number.
///
/// Do **not** send it when `workouts` is empty: to the official API an empty array means "clear the server's 7 days from now".
/// Only a clear the user confirmed can be sent that way (see `publish`).
pub fn window_body(window: Window, workouts: &[NumberedWorkout], locale: WatchLocale) -> Value {
    let mut inside: Vec<&NumberedWorkout> = workouts
        .iter()
        .filter(|item| window.contains(item.workout.date))
        .collect();
    inside.sort_by_key(|item| (item.workout.date, item.id));
    json!({
        "startDate": window.start.to_string(),
        "endDate": window.end().to_string(),
        "workouts": inside
            .into_iter()
            .map(|item| workout_json(item, locale))
            .collect::<Vec<_>>(),
    })
}

/// The payload that clears a **future** window: the window is written as usual, and `workouts` holds only one placeholder
/// workout dated the day before the window.
///
/// Warning: this depends on **undefined server behavior**. The official docs call a workout date outside `startDate`-`endDate`
/// an invalid request. The server does not check it now and still replaces the whole window (tested 2026-10-06 in R5:
/// the window's workouts were cleared, then a push of the previous window overwrote the placeholder, and nothing was left). Do not send
/// `[]` for a future window: an empty array always clears only "7 days from today". When the official API gives a proper method, change only this place.
/// The issue is item 7 in the desktop Zepp training plan API feedback file.
///
/// After this send, **push the previous window again at once** (the placeholder lands there). `storage::training_plan`
/// keeps the order. `placeholder_id` must not collide with a real workout number.
pub fn clear_future_window(window: Window, placeholder_id: i64) -> Value {
    let date = window.start - chrono::Duration::days(1);
    json!({
        "startDate": window.start.to_string(),
        "endDate": window.end().to_string(),
        "workouts": [{
            "workoutId": placeholder_id,
            "workoutDate": date.to_string(),
            "sport": "RUNNING",
            "workoutName": "ZeppBridge",
            "steps": [{
                "type": "WorkoutStep",
                "stepOrder": 1,
                "intensity": "ACTIVE",
                "durationType": "TIME",
                "durationValue": 600,
                "targetType": "OPEN",
                "targetValueLow": 0,
                "targetValueHigh": 0,
            }],
        }],
    })
}

fn workout_json(item: &NumberedWorkout, locale: WatchLocale) -> Value {
    let workout = &item.workout;
    let mut body = json!({
        "workoutId": item.id,
        "workoutDate": workout.date.to_string(),
        "sport": sport(workout.sport),
        "workoutName": workout.name,
        "steps": workout
            .steps
            .iter()
            .enumerate()
            .map(|(index, node)| node_json(index + 1, node))
            .collect::<Vec<_>>(),
    });
    if let Some(description) = watch_description(workout, locale) {
        body["description"] = json!(description);
    }
    body
}

fn node_json(order: usize, node: &StepNode) -> Value {
    match node {
        StepNode::Step(step) => step_json(order, step),
        StepNode::Repeat { times, steps } => json!({
            "type": "WorkoutRepeatStep",
            "stepOrder": order,
            "repeatType": "REPEAT_UNTIL_STEPS_CMPLT",
            "repeatValue": times,
            "steps": steps
                .iter()
                .enumerate()
                .map(|(index, step)| step_json(index + 1, step))
                .collect::<Vec<_>>(),
        }),
    }
}

fn step_json(order: usize, step: &Step) -> Value {
    let (duration_type, duration_value) = match step.length {
        StepLength::Time { seconds } => ("TIME", seconds),
        // The docs say "seconds"; R3 tests show meters: 1000 shows "1.00 km".
        StepLength::Distance { meters } => ("DISTANCE", meters),
    };
    let (target_type, low, high) = match step.target {
        // R3 test: low = high = 0 shows as no target.
        Target::Open => ("OPEN", json!(0), json!(0)),
        Target::HeartRate { low, high } => ("HEART_RATE_LAP", json!(low), json!(high)),
        Target::Power { low, high } => ("POWER_LAP", json!(low), json!(high)),
        // Speed (m/s): a slow pace is a low speed, sent in order (the server also corrects a swap, but we do not rely on it).
        Target::Pace { fast, slow } => (
            "PACE_LAP",
            json!(pace_to_speed(slow)),
            json!(pace_to_speed(fast)),
        ),
    };
    let mut body = json!({
        "type": "WorkoutStep",
        "stepOrder": order,
        "intensity": intensity(step.intensity),
        "durationType": duration_type,
        "durationValue": duration_value,
        "targetType": target_type,
        "targetValueLow": low,
        "targetValueHigh": high,
    });
    if let Some(note) = step.note.as_deref() {
        body["description"] = json!(note);
    }
    body
}

fn sport(sport: Sport) -> &'static str {
    match sport {
        Sport::Running => "RUNNING",
        Sport::Cycling => "CYCLING",
        Sport::PoolSwim => "LAP_SWIMMING",
        Sport::OpenWaterSwim => "OPEN_WATER_SWIMMING",
    }
}

fn intensity(intensity: Intensity) -> &'static str {
    match intensity {
        Intensity::Warmup => "WARMUP",
        Intensity::Active => "ACTIVE",
        Intensity::Interval => "INTERVAL",
        Intensity::Recovery => "RECOVERY",
        Intensity::Rest => "REST",
        Intensity::Cooldown => "COOLDOWN",
    }
}
