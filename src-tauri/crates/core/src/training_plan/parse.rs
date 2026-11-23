//! Two short texts in the writing format: duration (`"20min"`) and target (`"hr 135-150"`).
//!
//! Only clear forms are accepted. `"5m"` is 5 meters, not 5 minutes: 400m and 800m are the most common forms
//! in running plans, so minutes are always written `min`. An unknown form is an error. We do not guess.

use super::{StepLength, Target};

pub fn parse_bedtime(text: &str) -> Option<u16> {
    if text.len() != 5
        || text.as_bytes()[2] != b':'
        || !text
            .as_bytes()
            .iter()
            .enumerate()
            .all(|(i, b)| i == 2 || b.is_ascii_digit())
    {
        return None;
    }
    let (h, m) = text.split_once(':')?;
    let h: u16 = h.parse().ok()?;
    let m: u16 = m.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

pub fn parse_sleep_target(text: &str) -> Option<u32> {
    let text = text.trim();
    let seconds =
        if let Some((hours, minutes)) = text.split_once('h').filter(|(_, m)| !m.is_empty()) {
            let h: u32 = hours.parse().ok()?;
            let m: u32 = minutes.strip_suffix('m')?.parse().ok()?;
            if m >= 60 {
                return None;
            }
            h.checked_mul(3600)?.checked_add(m.checked_mul(60)?)?
        } else {
            match parse_duration(text)? {
                StepLength::Time { seconds } => seconds,
                _ => return None,
            }
        };
    (seconds > 0 && seconds <= 24 * 3600).then_some(seconds)
}

/// Duration: `<number><unit>`, with an optional space between them.
///
/// Time units `s` / `sec` / Chinese "seconds", `min` / Chinese "minutes", `h` / Chinese "hours". Distance units `m` / Chinese "meters",
/// `km` / Chinese "km". Chinese unit names stay as Unicode escapes. Decimals are allowed (`"1.5h"`, `"2.5km"`); the result rounds to whole seconds / meters.
pub fn parse_duration(text: &str) -> Option<StepLength> {
    let text = text.trim().to_ascii_lowercase();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let value: f64 = number.parse().ok().filter(|v: &f64| v.is_finite())?;
    if value <= 0.0 {
        return None;
    }
    let rounded = |scale: f64| {
        let scaled = (value * scale).round();
        (scaled >= 1.0 && scaled <= f64::from(u32::MAX)).then_some(scaled as u32)
    };
    Some(match unit.trim() {
        "s" | "sec" | "\u{79d2}" => StepLength::Time {
            seconds: rounded(1.0)?,
        },
        "min" | "\u{5206}\u{949f}" | "\u{5206}" => StepLength::Time {
            seconds: rounded(60.0)?,
        },
        "h" | "hr" | "\u{5c0f}\u{65f6}" => StepLength::Time {
            seconds: rounded(3600.0)?,
        },
        "m" | "\u{7c73}" => StepLength::Distance {
            meters: rounded(1.0)?,
        },
        "km" | "\u{516c}\u{91cc}" => StepLength::Distance {
            meters: rounded(1000.0)?,
        },
        _ => return None,
    })
}

/// Target: `hr 135-150`, `pace 5:30-5:50` (per km), `power 200-220`.
/// An empty string or `open` means no target. The two ends of a range can be swapped; this puts them in order.
pub fn parse_target(text: Option<&str>) -> Option<Target> {
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return Some(Target::Open);
    };
    let text = text.to_ascii_lowercase();
    if text == "open" {
        return Some(Target::Open);
    }
    let (kind, range) = text.split_once(char::is_whitespace)?;
    let (low, high) = range.trim().split_once('-')?;
    match kind {
        "hr" | "heart_rate" => {
            let (low, high) = ordered(low.trim().parse().ok()?, high.trim().parse().ok()?);
            Some(Target::HeartRate { low, high })
        }
        "power" => {
            let (low, high) = ordered(low.trim().parse().ok()?, high.trim().parse().ok()?);
            Some(Target::Power { low, high })
        }
        "pace" => {
            let (fast, slow) = ordered(pace_seconds(low)?, pace_seconds(high)?);
            Some(Target::Pace { fast, slow })
        }
        _ => None,
    }
}

/// `"5:30"` -> 330 seconds. The seconds part has at most two digits and is below 60.
fn pace_seconds(text: &str) -> Option<u16> {
    let (minutes, seconds) = text.trim().split_once(':')?;
    let minutes: u16 = minutes.parse().ok()?;
    let seconds: u16 = seconds.parse().ok().filter(|s| *s < 60)?;
    minutes.checked_mul(60)?.checked_add(seconds)
}

fn ordered(a: u16, b: u16) -> (u16, u16) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}
