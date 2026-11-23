//! Kilometer splits and laps: accumulate from point series, split by the watch's per-km times, check against the summary (split out of decoder/workout_detail.rs, logic unchanged).

use super::*;

/// Accumulator for the split currently being filled.
#[derive(Debug)]
pub(super) struct SplitBuilder {
    pub(super) index: i32,
    pub(super) start_time: DateTime<Utc>,
    pub(super) start_distance_m: f64,
    pub(super) hr_sum: i64,
    pub(super) hr_count: i64,
    pub(super) max_hr: Option<i32>,
    pub(super) committed_altitude: Option<f64>,
    pub(super) gain: f64,
    pub(super) loss: f64,
    pub(super) saw_altitude: bool,
}

impl SplitBuilder {
    pub(super) fn new(index: i32, start_time: DateTime<Utc>, start_distance_m: f64) -> Self {
        Self {
            index,
            start_time,
            start_distance_m,
            hr_sum: 0,
            hr_count: 0,
            max_hr: None,
            committed_altitude: None,
            gain: 0.0,
            loss: 0.0,
            saw_altitude: false,
        }
    }

    pub(super) fn observe(&mut self, sample: &WorkoutSample) {
        if let Some(heart_rate) = sample.heart_rate {
            self.hr_sum += i64::from(heart_rate);
            self.hr_count += 1;
            self.max_hr = Some(self.max_hr.map_or(heart_rate, |best| best.max(heart_rate)));
        }
        if let Some(altitude) = sample.altitude_m {
            self.saw_altitude = true;
            match self.committed_altitude {
                None => self.committed_altitude = Some(altitude),
                Some(previous) => {
                    let change = altitude - previous;
                    if change >= ELEVATION_NOISE_FLOOR_M {
                        self.gain += change;
                        self.committed_altitude = Some(altitude);
                    } else if change <= -ELEVATION_NOISE_FLOOR_M {
                        self.loss += -change;
                        self.committed_altitude = Some(altitude);
                    }
                }
            }
        }
    }

    pub(super) fn finish(
        self,
        end_time: DateTime<Utc>,
        end_distance_m: f64,
        partial: bool,
    ) -> WorkoutSplit {
        let distance_m = (end_distance_m - self.start_distance_m).max(0.0);
        let duration_seconds = (end_time - self.start_time).num_seconds().max(0);
        let pace_min_per_km = (distance_m > 0.0 && duration_seconds > 0)
            .then(|| (duration_seconds as f64 / 60.0) / (distance_m / 1000.0));
        WorkoutSplit {
            index: self.index,
            start_time: self.start_time,
            end_time,
            distance_m,
            duration_seconds,
            pace_min_per_km,
            avg_hr: (self.hr_count > 0).then(|| (self.hr_sum / self.hr_count) as i32),
            max_hr: self.max_hr,
            // A workout with no altitude readings reports nothing rather than a
            // confident zero.
            elevation_gain_m: self.saw_altitude.then_some(self.gain),
            elevation_loss_m: self.saw_altitude.then_some(self.loss),
            partial,
        }
    }
}

/// Laps from the `lap` field of the workout detail.
///
/// Row layout, confirmed against **all 32** workouts in a real library that
/// carry this field — rows come 60, 62 or 66 columns wide and the first six are
/// the same in every one:
///
/// ```text
/// 0,300,570,s00000000000,135,301,...
/// │ │   │   │             │   └ elapsed seconds at the end of this lap
/// │ │   │   │             └ average heart rate
/// │ │   │   └ geohash placeholder, always zeroed in these payloads
/// │ │   └ distance in metres
/// │ └ duration in seconds
/// └ 0-based lap number
/// ```
///
/// The check is not the column widths — those vary, and on `kilo_pace` the same
/// width was seen with the columns in different orders. It is that the numbers
/// have to agree with the workout: sequential indices, a non-decreasing elapsed
/// column, and heart rates in a range a heart can occupy. The caller adds the
/// two strongest checks, which need the summary: the lap distances must sum to
/// the workout distance and the last elapsed value must match its duration.
/// On the 32 real workouts all five hold every time.
///
/// Column `[1]` is **moving** time and can be shorter than the gap between two
/// elapsed values when the watch was paused, so lap boundaries are placed from
/// the elapsed column and `[1]` is not used for timing.
pub(super) fn parse_laps(value: Option<&Value>, start: DateTime<Utc>) -> Vec<WorkoutLap> {
    let Some(text) = value.and_then(Value::as_str).map(str::trim) else {
        return Vec::new();
    };
    if text.is_empty() {
        return Vec::new();
    }
    let mut laps = Vec::new();
    let mut previous_elapsed = 0i64;
    for (position, row) in text.split(';').filter(|row| !row.is_empty()).enumerate() {
        let columns: Vec<&str> = row.split(',').collect();
        if columns.len() < 6 {
            return Vec::new();
        }
        let (Ok(index), Ok(distance_m), Ok(heart_rate), Ok(elapsed)) = (
            columns[0].trim().parse::<i64>(),
            columns[2].trim().parse::<f64>(),
            columns[4].trim().parse::<i64>(),
            columns[5].trim().parse::<i64>(),
        ) else {
            return Vec::new();
        };
        if index != position as i64
            || !distance_m.is_finite()
            || distance_m < 0.0
            || elapsed < previous_elapsed
            || elapsed > MAX_ACTIVITY_SECONDS
            || !(0..=250).contains(&heart_rate)
        {
            return Vec::new();
        }
        let (Some(lap_start), Some(lap_end)) = (
            start.checked_add_signed(chrono::Duration::seconds(previous_elapsed)),
            start.checked_add_signed(chrono::Duration::seconds(elapsed)),
        ) else {
            return Vec::new();
        };
        laps.push(WorkoutLap {
            index: position as i32 + 1,
            start_time: lap_start,
            end_time: lap_end,
            distance_m,
            duration_seconds: elapsed - previous_elapsed,
            // A 0 from the watch means "no heart rate for this lap", not "heart rate is 0".
            avg_hr: (heart_rate > 0).then_some(heart_rate as i32),
            max_hr: None,
        });
        previous_elapsed = elapsed;
    }
    laps
}

/// Do the laps agree with the workout they belong to?
///
/// The two checks that need the summary, and the two that carry the most weight:
/// lap distances have to add up to the workout distance, and the last lap has to
/// end when the workout ended. A column read as the wrong field passes neither.
/// Either summary value being absent skips its own check rather than failing —
/// an indoor workout has no distance, and that is not evidence against the laps.
pub(super) fn laps_agree_with_summary(
    laps: &[WorkoutLap],
    summary_distance_m: Option<f64>,
    duration_seconds: i64,
) -> bool {
    if laps.is_empty() {
        return false;
    }
    if let Some(distance) = summary_distance_m.filter(|value| value.is_finite() && *value > 0.0) {
        let total: f64 = laps.iter().map(|lap| lap.distance_m).sum();
        if (total - distance).abs() / distance > LAP_SUMMARY_TOLERANCE {
            return false;
        }
    }
    if duration_seconds > 0 {
        let last = laps
            .last()
            .map(|lap| (lap.end_time - laps[0].start_time).num_seconds())
            .unwrap_or_default();
        let drift = (last - duration_seconds).abs() as f64 / duration_seconds as f64;
        if drift > LAP_SUMMARY_TOLERANCE {
            return false;
        }
    }
    true
}

/// How far the total lap distance / duration may differ from the summary.
///
/// 5% is not arbitrary: the largest gap in 32 real records is 2% (the watch has a few seconds
/// between timer stop and lap record), while a misread column is off by a multiple, not a few percent.
pub(super) const LAP_SUMMARY_TOLERANCE: f64 = 0.05;

/// Kilometre boundaries from the server's own `kilo_pace` series.
///
/// Splits normally come from `currentDistance`, but on this library **130 of
/// 336 stored workout details have an empty `currentDistance` and a populated
/// `kilo_pace`** — those workouts get no splits at all today, which is what a
/// .fit comparison on GitHub surfaced as "run.fit: 1 lap, Zepp: 8 laps".
///
/// The row layout is **not stable**. Rows come 6, 14 or 15 fields wide, and at
/// the same width the columns still move: on one workout field 4 is the average
/// heart rate and field 13 is something else, on another it is the reverse. So
/// only three columns are read here, and the decode has to prove itself before
/// it is used:
///
/// * `[0]` the 0-based kilometre number, which must equal its position;
/// * `[1]` that kilometre's duration in seconds, which must be plausible;
/// * `[5]` the elapsed total, which must equal the running sum of `[1]` —
///   or, on watches that count it in wall-clock time (#139: a Balance 2 trail
///   run with 17 pauses), must grow by at least `[1]` every row. In that case
///   `[5]` places the boundaries directly, pauses included.
///
/// If any row fails, the whole series is refused and the workout keeps the
/// single whole-activity lap it has today. On 266 real workouts this accepts
/// 263 and rejects 3 — and the three it rejects are ones whose columns really
/// do not line up.
///
/// Heart rate and elevation are **not** read out of `kilo_pace`. They are
/// accumulated from our own per-second samples over each kilometre's time
/// window, exactly as the `currentDistance` path does, because that needs no
/// guess about which column means what.
/// Per-km times read from `kilo_pace`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct KilometreTiming {
    /// **Moving** time per km (`[1]`), in seconds.
    pub(super) moving: Vec<i64>,
    /// **Wall clock** seconds from workout start to the end of each km (`[5]`). Present only when `[5]` includes pauses.
    /// Without it, the boundaries convert moving time to wall clock with the pause ranges.
    pub(super) elapsed_end: Option<Vec<i64>>,
}

pub(super) fn kilometre_seconds(
    value: Option<&Value>,
    duration_seconds: i64,
    pauses: &[PauseInterval],
) -> Option<KilometreTiming> {
    let text = value.and_then(Value::as_str)?.trim();
    if text.is_empty() {
        return None;
    }
    let mut seconds = Vec::new();
    let mut cumulative_column = Vec::new();
    for (position, row) in text.split(';').filter(|row| !row.is_empty()).enumerate() {
        let columns: Vec<&str> = row.split(',').collect();
        if columns.len() < 6 {
            return None;
        }
        let index: i64 = columns[0].trim().parse().ok()?;
        let duration: i64 = columns[1].trim().parse().ok()?;
        let cumulative: i64 = columns[5].trim().parse().ok()?;
        if index != position as i64 {
            return None;
        }
        // More than an hour for one km is not a pace. The column was misread.
        if !(1..=3600).contains(&duration) {
            return None;
        }
        seconds.push(duration);
        cumulative_column.push(cumulative);
    }
    if seconds.is_empty() {
        return None;
    }
    // Case one: `[5]` is the running sum of `[1]` (all 263 rows in our data).
    // Compare the increment per km, allow 2 seconds of rounding per km, and do not accumulate. In some records this column
    // is rounded from milliseconds. With 1 extra second per km as in #159, the total error passed
    // 2 seconds by the third km, and the old code rejected the whole record with no km splits.
    let mut previous = 0i64;
    let is_moving_sum = seconds
        .iter()
        .zip(&cumulative_column)
        .all(|(duration, cumulative)| {
            let step = cumulative - previous;
            previous = *cumulative;
            (step - duration).abs() <= 2
        });
    if is_moving_sum {
        return Some(KilometreTiming {
            moving: seconds,
            elapsed_end: None,
        });
    }
    // Case two: `[5]` is cumulative wall clock including pauses. All three checks must hold, or the columns were misread
    // and the whole record is rejected:
    // * each row covers at least the moving time of that km;
    // * the last km ends no later than the workout end;
    // * the extra time of `[5]` over moving time is at most the total pause time (with 5 seconds plus 2% slack:
    //   the watch and the server round pause start and end differently).
    let mut previous = 0i64;
    for (duration, cumulative) in seconds.iter().zip(&cumulative_column) {
        let step = cumulative - previous;
        if step < duration - 2 {
            return None;
        }
        previous = *cumulative;
    }
    let last_elapsed = *cumulative_column.last()?;
    if last_elapsed > duration_seconds + 2 {
        return None;
    }
    // If extra time appears with no pauses, it is not pause time and `[5]` cannot place the boundaries.
    // But the per-km times in `[1]` already passed validation, so fall back to them alone (place boundaries by moving
    // time as in case one). Do not reject the whole record.
    // Pauses after the last km are not in `[5]`, so only require that the gap is at most the total pause time.
    if pauses.is_empty() {
        return Some(KilometreTiming {
            moving: seconds,
            elapsed_end: None,
        });
    }
    let gap = last_elapsed - seconds.iter().sum::<i64>();
    let paused: i64 = pauses
        .iter()
        .map(|pause| (pause.end_time - pause.start_time).num_seconds())
        .sum();
    if gap > paused + 5 + paused / 50 {
        return None;
    }
    Some(KilometreTiming {
        moving: seconds,
        elapsed_end: Some(cumulative_column),
    })
}

/// Converts "`moving` seconds of movement after the start" to a wall clock time: each pause that starts before that point
/// pushes the time later. The pauses are sorted by start.
pub(super) fn wall_clock_after(
    start: DateTime<Utc>,
    moving: i64,
    pauses: &[PauseInterval],
) -> Option<DateTime<Utc>> {
    let mut wall = add_seconds(start, moving)?;
    for pause in pauses {
        if pause.start_time < wall {
            wall = wall.checked_add_signed(pause.end_time - pause.start_time)?;
        } else {
            break;
        }
    }
    Some(wall)
}

/// Build kilometre splits from `kilo_pace` durations plus our own samples.
///
/// `kilo_pace` carries exactly `floor(distance / 1000)` rows — the trailing
/// partial kilometre is not one of them — so the remainder is added here from
/// the summary distance, keeping the same `partial` flag the other path sets.
/// Without it a 4975 m run would report 4 km and quietly lose 975 m.
pub(super) fn splits_from_kilometre_seconds(
    samples: &[WorkoutSample],
    timing: &KilometreTiming,
    pauses: &[PauseInterval],
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    total_distance_m: Option<f64>,
) -> Vec<WorkoutSplit> {
    let sample_by_second: std::collections::BTreeMap<i64, &WorkoutSample> = samples
        .iter()
        .map(|sample| (sample.timestamp.timestamp(), sample))
        .collect();

    let mut splits = Vec::new();
    let mut cursor = start;
    let mut travelled = 0.0f64;
    let mut moving = 0i64;
    for (position, duration) in timing.moving.iter().enumerate() {
        moving += duration;
        // Place boundaries on the wall clock: samples, pauses and FIT laps all use the wall clock. The old code used
        // "start + cumulative moving time", so after a trail run with a 24-minute stop, every boundary in the second half
        // was minutes early.
        let boundary = match &timing.elapsed_end {
            Some(elapsed) => add_seconds(start, elapsed[position]),
            None => wall_clock_after(start, moving, pauses),
        };
        let Some(end) = boundary.filter(|end| *end > cursor) else {
            break;
        };
        let mut builder = SplitBuilder::new(position as i32 + 1, cursor, travelled);
        for sample in sample_by_second
            .range(cursor.timestamp()..=end.timestamp())
            .map(|(_, item)| *item)
        {
            builder.observe(sample);
        }
        travelled += 1000.0;
        splits.push(builder.finish(end, travelled, false));
        cursor = end;
    }

    // The last partial km. `kilo_pace` gives only whole kilometers, so its duration can come only from the workout
    // end time. Do not use "the last sample": on records where sampling stops before the timer, that drops the whole partial
    // segment, and that is exactly the 975 m of a 4975 m run.
    if let Some(total) = total_distance_m.filter(|value| value.is_finite()) {
        if total - travelled > 1.0 && end > cursor {
            let index = splits.len() as i32 + 1;
            let mut builder = SplitBuilder::new(index, cursor, travelled);
            for sample in sample_by_second
                .range(cursor.timestamp()..=end.timestamp())
                .map(|(_, item)| *item)
            {
                builder.observe(sample);
            }
            splits.push(builder.finish(end, total, true));
        }
    }
    splits
}

/// Cut a workout into kilometres along the server's cumulative distance.
///
/// The distance series drives the walk, not the sample series: a workout's
/// distance readings can run past its last per-second sample, and driving from
/// the samples silently dropped that tail — a 578 m walk came out 1.75% short.
/// Samples are folded in for heart rate and altitude wherever they line up.
pub(super) fn compute_splits(
    samples: &[WorkoutSample],
    distance_by_second: &std::collections::BTreeMap<i64, f64>,
) -> Vec<WorkoutSplit> {
    let Some((&first_ts, _)) = distance_by_second.iter().next() else {
        return Vec::new();
    };
    let Some(first_time) = Utc.timestamp_opt(first_ts, 0).single() else {
        return Vec::new();
    };
    let sample_by_second: std::collections::BTreeMap<i64, &WorkoutSample> = samples
        .iter()
        .map(|sample| (sample.timestamp.timestamp(), sample))
        .collect();

    let mut splits = Vec::new();
    let mut boundary = 1000.0f64;
    let mut builder = SplitBuilder::new(1, first_time, 0.0);
    let mut previous_ts: Option<i64> = None;
    let mut travelled = 0.0f64;
    let mut last_time = first_time;

    for (&unix_ts, distance) in distance_by_second {
        let Some(moment) = Utc.timestamp_opt(unix_ts, 0).single() else {
            continue;
        };
        // Every sample since the previous distance reading belongs to this
        // split, so a distance series coarser than one second still averages
        // heart rate over the whole kilometre.
        let lower = previous_ts.map_or(unix_ts, |previous| previous.saturating_add(1));
        for sample in sample_by_second
            .range(lower..=unix_ts)
            .map(|(_, item)| *item)
        {
            builder.observe(sample);
        }
        previous_ts = Some(unix_ts);
        travelled = *distance;
        last_time = moment;

        // A single reading can only span more than one boundary in corrupt
        // data; looping keeps the indices contiguous if it ever happens.
        while travelled >= boundary {
            let index = builder.index;
            splits.push(builder.finish(moment, boundary, false));
            builder = SplitBuilder::new(index + 1, moment, boundary);
            boundary += 1000.0;
        }
    }

    if travelled > builder.start_distance_m {
        splits.push(builder.finish(last_time, travelled, true));
    }
    splits
}
