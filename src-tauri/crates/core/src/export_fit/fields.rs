//! Field encoding and conversion: HR zones, pauses, sport type mapping, time and coordinates (split out of export_fit.rs, logic unchanged).

use super::*;

/// Time in each heart rate zone, as the FIT `time_in_hr_zone` array.
///
/// `hr_zones` in the database is `{index, upper_bound_bpm, seconds}`, with `index` from 0,
/// and zone 0 is "below the lower bound of zone 1". This matches the FIT array convention. Even so,
/// **place each value by `index`**, not by array order: if any zone is missing, flat order shifts every
/// later number one zone forward, and after the shift they still look fully plausible.
///
/// The field is `Vec<u32>`, scale 1000, unit seconds, so the stored value is milliseconds.
/// Zones the watch did not measure stay 0: that means "no time in this zone this time", not missing data.
pub(super) fn hr_zone_field(workout: &Value) -> Option<Vec<u32>> {
    let zones = workout.get("hr_zones").and_then(Value::as_array)?;
    let mut buckets: Vec<u32> = Vec::new();
    let mut wrote_any = false;
    for zone in zones {
        let Some(index) = zone.get("index").and_then(Value::as_i64) else {
            continue;
        };
        let Some(seconds) = zone.get("seconds").and_then(Value::as_f64) else {
            continue;
        };
        // The limit of 8 leaves room for zone numbers (the watch uses 1-5). Drop an out-of-range index
        // so that one bad number cannot grow the array to tens of thousands of elements.
        if !(0..=8).contains(&index) || !seconds.is_finite() || seconds < 0.0 {
            continue;
        }
        let slot = index as usize;
        if buckets.len() <= slot {
            buckets.resize(slot + 1, 0);
        }
        let milliseconds = (seconds * 1000.0).round();
        if !milliseconds.is_finite() || milliseconds > f64::from(u32::MAX) {
            continue;
        }
        buckets[slot] = milliseconds as u32;
        wrote_any = true;
    }
    wrote_any.then_some(buckets)
}

/// `(pause start, resume)` pairs of second timestamps, sorted by start time.
pub(super) fn pause_intervals(workout: &Value, from: i64, to: i64) -> Vec<(i64, i64)> {
    let mut intervals: Vec<(i64, i64)> = array(workout, "pauses")
        .iter()
        .filter_map(|pause| {
            let start = parse_unix(text(pause.get("start_time")))?;
            let end = parse_unix(text(pause.get("end_time")))?;
            let start = start.max(from);
            let end = end.min(to);
            (end > start).then_some((start, end))
        })
        .collect();
    intervals.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (start, end) in intervals {
        if let Some(last) = merged.last_mut().filter(|last| start <= last.1) {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

pub(super) fn paused_seconds(workout: &Value, start: i64, end: i64) -> f64 {
    pause_intervals(workout, start, end)
        .iter()
        .map(|(from, to)| (to - from) as f64)
        .sum()
}

pub(super) fn timer_seconds(workout: &Value, start: i64, end: i64) -> f64 {
    let elapsed = (end - start).max(0) as f64;
    workout
        .get("moving_seconds")
        .and_then(Value::as_f64)
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0 && *seconds <= elapsed)
        .unwrap_or_else(|| (elapsed - paused_seconds(workout, start, end)).max(0.0))
}

pub(super) fn timer_event(unix: i64, event_type: typedef::EventType) -> Message {
    Message {
        num: typedef::MesgNum::EVENT,
        fields: vec![
            u32_field(mesgdef::Event::TIMESTAMP, fit_timestamp(unix)),
            enum_field(mesgdef::Event::EVENT, typedef::Event::TIMER.0),
            enum_field(mesgdef::Event::EVENT_TYPE, event_type.0),
        ],
        ..Default::default()
    }
}

/// Sport type -> FIT `sport` / `sub_sport`.
///
/// Map only types that really have a FIT match. Unknown types all go to `GENERIC`, instead of
/// a forced near match. Tai chi recorded as "running" is worse than "generic".
pub(super) fn map_sport(key: &str) -> (typedef::Sport, typedef::SubSport) {
    use typedef::{Sport, SubSport};
    match key {
        "run" => (Sport::RUNNING, SubSport::STREET),
        "trail_running" => (Sport::RUNNING, SubSport::TRAIL),
        "treadmill" => (Sport::RUNNING, SubSport::TREADMILL),
        "race_walking" => (Sport::WALKING, SubSport::SPEED_WALKING),
        "walking" | "dog_walking" => (Sport::WALKING, SubSport::GENERIC),
        "indoor_walking" => (Sport::WALKING, SubSport::INDOOR_WALKING),
        "hiking" => (Sport::HIKING, SubSport::GENERIC),
        "ride" => (Sport::CYCLING, SubSport::GENERIC),
        "road_cycling" => (Sport::CYCLING, SubSport::ROAD),
        // Alias fallback: text types in old databases that did not go through the alias table (#157).
        "mountain_biking" | "mountain_bike" | "mtb" => (Sport::CYCLING, SubSport::MOUNTAIN),
        "indoor_cycling" => (Sport::CYCLING, SubSport::INDOOR_CYCLING),
        "spinning" => (Sport::CYCLING, SubSport::SPIN),
        "bmx" => (Sport::CYCLING, SubSport::BMX),
        "e_bike" => (Sport::E_BIKING, SubSport::GENERIC),
        "pool_swimming" => (Sport::SWIMMING, SubSport::LAP_SWIMMING),
        "open_water_swimming" => (Sport::SWIMMING, SubSport::OPEN_WATER),
        "finswimming" | "artistic_swimming" => (Sport::SWIMMING, SubSport::GENERIC),
        "snorkeling" => (Sport::SNORKELING, SubSport::GENERIC),
        "rowing" | "water_rowing" => (Sport::ROWING, SubSport::GENERIC),
        "cross_country_skiing" => (Sport::CROSS_COUNTRY_SKIING, SubSport::GENERIC),
        "alpine_skiing" => (Sport::ALPINE_SKIING, SubSport::GENERIC),
        "kayaking" => (Sport::KAYAKING, SubSport::GENERIC),
        "sailing" => (Sport::SAILING, SubSport::GENERIC),
        "strength" => (Sport::TRAINING, SubSport::STRENGTH_TRAINING),
        "core_training" | "cross_training" | "free_training" | "indoor_fitness" => {
            (Sport::TRAINING, SubSport::CARDIO_TRAINING)
        }
        "flexibility" | "stretching" => (Sport::TRAINING, SubSport::FLEXIBILITY_TRAINING),
        "yoga" => (Sport::TRAINING, SubSport::YOGA),
        "pilates" => (Sport::TRAINING, SubSport::PILATES),
        "hiit" => (Sport::HIIT, SubSport::GENERIC),
        "jump_rope" => (Sport::JUMP_ROPE, SubSport::GENERIC),
        "stair_climber" => (Sport::FITNESS_EQUIPMENT, SubSport::STAIR_CLIMBING),
        "stair_climbing" => (Sport::FLOOR_CLIMBING, SubSport::GENERIC),
        "stepper" | "air_walker" => (Sport::FITNESS_EQUIPMENT, SubSport::ELLIPTICAL),
        "rock_climbing" | "bouldering" => (Sport::ROCK_CLIMBING, SubSport::GENERIC),
        "boxing" => (Sport::BOXING, SubSport::GENERIC),
        "kickboxing" | "muay_thai" | "martial_arts" | "judo" | "jujitsu" | "karate"
        | "taekwondo" | "kendo" | "wrestling" => (Sport::MIXED_MARTIAL_ARTS, SubSport::GENERIC),
        "tennis" => (Sport::TENNIS, SubSport::GENERIC),
        "badminton" | "squash" | "table_tennis" | "racket" => (Sport::RACKET, SubSport::GENERIC),
        "padel" => (Sport::RACKET, SubSport::PADEL),
        "basketball" => (Sport::BASKETBALL, SubSport::GENERIC),
        "soccer" | "futsal" => (Sport::SOCCER, SubSport::GENERIC),
        "american_football" => (Sport::AMERICAN_FOOTBALL, SubSport::GENERIC),
        "volleyball" | "beach_volleyball" => (Sport::VOLLEYBALL, SubSport::GENERIC),
        "baseball" | "softball" => (Sport::BASEBALL, SubSport::GENERIC),
        "cricket" => (Sport::CRICKET, SubSport::GENERIC),
        "ice_hockey" | "floorball" => (Sport::HOCKEY, SubSport::GENERIC),
        "handball" => (Sport::TEAM_SPORT, SubSport::GENERIC),
        "golf" => (Sport::GOLF, SubSport::GENERIC),
        "archery" => (Sport::ARCHERY, SubSport::GENERIC),
        "horse_riding" => (Sport::HORSEBACK_RIDING, SubSport::GENERIC),
        "ice_skating" | "indoor_ice_skating" => (Sport::ICE_SKATING, SubSport::GENERIC),
        "roller_skating" => (Sport::INLINE_SKATING, SubSport::GENERIC),
        "skateboarding" => (Sport::WINTER_SPORT, SubSport::GENERIC),
        "dance" | "ballet" | "ballroom_dance" | "belly_dance" | "breaking" | "folk_dance"
        | "hip_hop" | "jazz_dance" | "latin_dance" | "modern_dance" | "pole_dance"
        | "square_dance" | "street_dance" | "zumba" => (Sport::DANCE, SubSport::GENERIC),
        "esports" | "somatosensory_game" => (Sport::VIDEO_GAMING, SubSport::GENERIC),
        "fishing" => (Sport::FISHING, SubSport::GENERIC),
        "driving" => (Sport::DRIVING, SubSport::GENERIC),
        "tai_chi" => (Sport::MEDITATION, SubSport::GENERIC),
        _ => (Sport::GENERIC, SubSport::GENERIC),
    }
}

/// The file name carries the sport type and start time, so a batch export comes out sorted.
pub(super) fn file_name_for(workout: &Value) -> String {
    let workout_type = text(
        workout
            .get("effective_type")
            .or_else(|| workout.get("workout_type")),
    );
    let kind = if workout_type.is_empty() {
        "workout"
    } else {
        workout_type
    };
    let stamp = parse_time(text(workout.get("start_time")))
        .map(|time| time.format("%Y%m%d-%H%M%S").to_string())
        .unwrap_or_else(|| "unknown-time".to_string());
    let safe: String = kind
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{stamp}-{safe}.fit")
}

pub(super) fn fit_timestamp(unix: i64) -> u32 {
    (unix - FIT_EPOCH_OFFSET).clamp(0, i64::from(u32::MAX)) as u32
}

/// Convert "steps/minute" to FIT `cadence`.
///
/// # The unit was checked, not guessed
///
/// `samples[].cadence` comes from the fourth component of Zepp `gait`. Whether it is steps/min or
/// revolutions per minute can be checked against this workout's own cloud summary. Those fields are in the raw payload,
/// but the parser did not extract them, so before we wrongly thought "it cannot be checked":
///
/// | Summary field (walk trackid 1787901817) | Our series |
/// |---|---|
/// | `max_frequency` = 141 | Series max 141.0 |
/// | `avg_frequency` = 99.0 | Series mean 100.6 |
/// | `avg_stride_length` = 70 cm | Stride derived from steps/min 0.70 m |
/// | `total_step` = 8998 | Steps derived from steps/min ≈ 9060 |
///
/// Read as "revolutions per minute", the stride is 0.35 m and the steps 18121, off by exactly two times. So the unit of this series
/// is **steps/minute**. The `avg_frequency` of a ride is 0.0, which also matches our all-zero
/// series for it.
///
/// # Divide by two when writing FIT?
///
/// The unit of FIT `cadence` is rpm, "full cycles per minute". For running and
/// walking one cycle is one full stride (two footfalls), so the spec value is
/// steps/min / 2, and readers multiply by two for display. FIT files from Garmin running watches are
/// in the 80-95 range, not 160-190. In cycling one cycle is one crank turn, which is already
/// rpm, so do not divide.
///
/// A wrong division is always off by two and hard to see, so handle it per sport type, not the same for all.
/// m/s -> FIT speed encoding (scale 1000, u16). Out of range means not written, not clamped.
pub(super) fn encode_speed(mps: f64) -> Option<u16> {
    if !mps.is_finite() || mps < 0.0 {
        return None;
    }
    let scaled = (mps * 1000.0).round();
    if scaled > f64::from(u16::MAX) {
        return None;
    }
    Some(scaled as u16)
}

/// Max measured speed in `[start, end]`. Returns `None` without samples.
pub(super) fn max_speed_between(points: &[(i64, Point)], start: i64, end: i64) -> Option<f64> {
    points
        .iter()
        .filter(|(unix, _)| *unix >= start && *unix <= end)
        .filter_map(|(_, point)| point.speed_mps)
        .filter(|value| value.is_finite() && *value >= 0.0)
        .fold(None, |best: Option<f64>, value| {
            Some(best.map_or(value, |best| best.max(value)))
        })
}

/// Total ascent / descent.
///
/// Prefer the cloud's own `elevation_gain_m` / `elevation_loss_m`: that is the number the user sees in
/// the Zepp App, and an export that matches it is not reported as a bug. Only when the cloud gives none,
/// fall back to the sum of per-segment ascent in `splits`. That is a measured value the parser cut from the altitude series with a 1 m noise floor
/// (see `ELEVATION_NOISE_FLOOR_M` in `decoder/workout_detail.rs`).
///
/// The two can differ: one measured 6.37 km walk had 59 m from the cloud and 37 m as the sum of splits. Neither is wrong,
/// but only one can appear in the export.
///
/// `distance_ascend` in the cloud summary is "horizontal distance covered while climbing", not climb
/// height, so it cannot stand in for it.
pub(super) fn total_elevation(workout: &Value, summary_key: &str, split_key: &str) -> Option<f64> {
    if let Some(value) = workout
        .get(summary_key)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
    {
        return Some(value);
    }
    let mut total = 0.0;
    let mut seen = false;
    for split in array(workout, "splits") {
        if let Some(value) = split.get(split_key).and_then(Value::as_f64) {
            if value.is_finite() && value >= 0.0 {
                total += value;
                seen = true;
            }
        }
    }
    seen.then_some(total)
}

pub(super) fn steps_per_minute_to_fit_cadence(value: f64, sport: typedef::Sport) -> Option<u8> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    let cycles_per_minute = if is_foot_sport(sport) {
        value / 2.0
    } else {
        value
    };
    let rounded = cycles_per_minute.round();
    if rounded < 1.0 || rounded > f64::from(u8::MAX) {
        return None;
    }
    Some(rounded as u8)
}

/// Sports where one cycle is one full stride. See `steps_per_minute_to_fit_cadence`.
pub(super) fn is_foot_sport(sport: typedef::Sport) -> bool {
    matches!(
        sport.0,
        value if value == typedef::Sport::RUNNING.0
            || value == typedef::Sport::WALKING.0
            || value == typedef::Sport::HIKING.0
    )
}

/// Coordinate range: latitude ±90, longitude ±180, and both must be finite.
/// This is the last boundary of the export, separate from clamping on the decode side. Data that arrives here can
/// come directly from built export input, without the decoder.
pub(super) fn coordinates_in_domain(latitude: f64, longitude: f64) -> bool {
    latitude.is_finite()
        && longitude.is_finite()
        && latitude.abs() <= 90.0
        && longitude.abs() <= 180.0
}

pub(super) fn semicircles(degrees: f64) -> Option<i32> {
    if !degrees.is_finite() || degrees.abs() > 180.0 {
        return None;
    }
    // At longitude exactly 180.0 degrees (date line), `180 x 2^31/180` is exactly 2^31, which is i32::MAX
    // plus 1. Before, this returned None, so the position fields of that point were skipped entirely. A real
    // coordinate disappeared because of one lowest bit. The saturating clamp error is 2^-31 semicircles,
    // about 8x10^-8 degrees, three orders of magnitude smaller than GPS accuracy.
    let scaled = (degrees * SEMICIRCLES_PER_DEGREE).round();
    Some(scaled.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32)
}

/// Offset in seconds of this workout's time zone from UTC.
///
/// UTC time in the database has lost the original time zone. A zero offset and a parse failure both return `None`.
/// A non-zero RFC3339 offset can still be kept. When it is unknown, the caller omits the local timestamp.
pub(super) fn local_offset_seconds(workout: &Value) -> Option<i32> {
    parse_time(text(workout.get("start_time")))
        .map(|time| time.offset().local_minus_utc())
        .filter(|offset| *offset != 0)
}

pub(super) fn encode_altitude(metres: f64) -> Option<u16> {
    if !metres.is_finite() {
        return None;
    }
    let scaled = ((metres + ALTITUDE_OFFSET_M) * ALTITUDE_SCALE).round();
    if scaled < 0.0 || scaled > f64::from(u16::MAX) {
        return None;
    }
    Some(scaled as u16)
}

pub(super) fn parse_unix(value: &str) -> Option<i64> {
    parse_time(value).map(|time| time.timestamp())
}

pub(super) fn parse_time(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

pub(super) fn array<'a>(parent: &'a Value, key: &str) -> &'a [Value] {
    parent
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub(super) fn text(value: Option<&Value>) -> &str {
    value.and_then(Value::as_str).unwrap_or("")
}

pub(super) fn enum_field(num: u8, value: u8) -> Field {
    Field {
        num,
        base_type: typedef::FitBaseType::ENUM,
        value: FitValue::Uint8(value),
        is_expanded: false,
    }
}

pub(super) fn u8_field(num: u8, value: u8) -> Field {
    Field {
        num,
        base_type: typedef::FitBaseType::UINT8,
        value: FitValue::Uint8(value),
        is_expanded: false,
    }
}

pub(super) fn u16_field(num: u8, value: u16) -> Field {
    Field {
        num,
        base_type: typedef::FitBaseType::UINT16,
        value: FitValue::Uint16(value),
        is_expanded: false,
    }
}

pub(super) fn u32_field(num: u8, value: u32) -> Field {
    Field {
        num,
        base_type: typedef::FitBaseType::UINT32,
        value: FitValue::Uint32(value),
        is_expanded: false,
    }
}

pub(super) fn i32_field(num: u8, value: i32) -> Field {
    Field {
        num,
        base_type: typedef::FitBaseType::SINT32,
        value: FitValue::Int32(value),
        is_expanded: false,
    }
}

/// A `u32` array field.
///
/// `time_in_hr_zone` is the only array field in this export: one element per heart rate zone.
/// All other fields are scalars, so this builder has only this one user. Without it, the time-in-zone
/// distribution would have to split into several invented field names that no platform can read.
pub(super) fn u32_array_field(num: u8, values: Vec<u32>) -> Field {
    Field {
        num,
        base_type: typedef::FitBaseType::UINT32,
        value: FitValue::VecUint32(values),
        is_expanded: false,
    }
}

pub(super) fn string_field(num: u8, value: &str) -> Field {
    Field {
        num,
        base_type: typedef::FitBaseType::STRING,
        value: FitValue::String(value.to_string()),
        is_expanded: false,
    }
}
