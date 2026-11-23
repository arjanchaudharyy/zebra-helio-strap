//! Single-run insights: comparable runs, baseline and heart rate drift.

use super::*;

impl Database {
    /// A deterministic insight for one workout.
    pub fn workout_insight(&self, workout_id: &str) -> Result<WorkoutInsight> {
        let Some(workout) = self.get_workout_detail(workout_id)? else {
            return Err(crate::models::ZeppBridgeError::DataUnavailable(
                "This workout record is not in the local database".into(),
            ));
        };
        let workout_type = workout.effective_type.clone();
        if !SUPPORTED_WORKOUT_TYPES.contains(&workout_type.as_str()) {
            return Ok(WorkoutInsight {
                workout_id: workout_id.to_string(),
                workout_type,
                supported: false,
                unsupported_reason: Some(
                    "Insights for this workout type are not supported yet. The first version only covers running verified with real data. Other workouts can still be viewed, corrected and exported as usual."
                        .into(),
                ),
                unsupported_code: Some("unsupported_workout_type".into()),
                // The first/second half comparison also covers running only. Walking and cycling have enough per-point samples,
                // but their thresholds are not verified with real data. That conflicts with the first rule
                // at the top of this module, so this stays empty.
                heart_rate_drift: None,
                heart_rate_drift_unavailable: Some("unsupported_workout_type".into()),
                facts: Vec::new(),
                baseline_included: Vec::new(),
                baseline_excluded: Vec::new(),
            });
        }

        let target = self.run_row(workout_id)?.ok_or_else(|| {
            crate::models::ZeppBridgeError::DataUnavailable("This workout record is not in the local database".into())
        })?;
        let (included, excluded) = self.comparable_runs(&target)?;

        let window = BaselineWindow {
            kind: "comparable_runs".into(),
            days: baseline::WINDOW_DAYS,
            min_samples: baseline::MIN_SAMPLES as i64,
            max_samples: baseline::MAX_SAMPLES as i64,
            distance_tolerance_percent: Some(baseline::DISTANCE_TOLERANCE * 100.0),
        };

        let facts = vec![
            run_fact(
                "run.distance",
                "distance",
                "m",
                target.distance_meters,
                &included,
                |row| row.distance_meters,
                &window,
                &target.source_scope,
            ),
            run_fact(
                "run.duration",
                "duration",
                "s",
                target.duration_seconds(),
                &included,
                RunRow::duration_seconds,
                &window,
                &target.source_scope,
            ),
            run_fact(
                "run.pace",
                "pace",
                "s/km",
                target.pace_seconds_per_km(),
                &included,
                RunRow::pace_seconds_per_km,
                &window,
                &target.source_scope,
            ),
            run_fact(
                "run.avg_hr",
                "avg_hr",
                "bpm",
                target.avg_hr.map(f64::from),
                &included,
                |row| row.avg_hr.map(f64::from),
                &window,
                &target.source_scope,
            ),
            run_fact(
                "run.training_load",
                "training_load",
                "load",
                target.training_load,
                &included,
                |row| row.training_load,
                &window,
                &target.source_scope,
            ),
        ];

        // The first/second half comparison is separate from the baseline comparison above. It only uses the
        // per-point samples of this workout and needs no history. So even when there are too few comparable runs and all of the above is
        // `insufficient`, this one can still give a finding.
        let (heart_rate_drift, heart_rate_drift_unavailable) =
            match self.heart_rate_drift(workout_id)? {
                Ok(drift) => (Some(drift), None),
                Err(code) => (None, Some(code)),
            };

        Ok(WorkoutInsight {
            workout_id: workout_id.to_string(),
            workout_type,
            supported: true,
            unsupported_reason: None,
            unsupported_code: None,
            heart_rate_drift,
            heart_rate_drift_unavailable,
            facts,
            baseline_included: included
                .iter()
                .map(|row| BaselineEntry {
                    workout_id: row.workout_id.clone(),
                    start_time: row.start_time.to_rfc3339(),
                    distance_meters: row.distance_meters.unwrap_or_default(),
                })
                .collect(),
            baseline_excluded: excluded,
        })
    }

    /// The "pace x heart rate" comparison of the first and second half.
    ///
    /// `Err` only means the database read failed. `Ok(Err(code))` means this workout does not meet the conditions.
    /// It carries a stable reason code for the UI to translate.
    pub fn heart_rate_drift(
        &self,
        workout_id: &str,
    ) -> Result<std::result::Result<HeartRateDrift, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT timestamp, heart_rate, speed
             FROM workout_samples
             WHERE workout_id = ?1 AND heart_rate IS NOT NULL AND speed IS NOT NULL
             ORDER BY timestamp",
        )?;
        let rows = stmt.query_map(rusqlite::params![workout_id], |row| {
            let timestamp: String = row.get(0)?;
            let heart_rate: f64 = row.get(1)?;
            let speed: f64 = row.get(2)?;
            Ok((timestamp, heart_rate, speed))
        })?;

        let mut samples: Vec<DriftSample> = Vec::new();
        for row in rows {
            let (timestamp, heart_rate, speed) = row?;
            // A poor fit drops the heart rate to single digits, and a stop drops the speed to 0. Neither is
            // "the real intensity of this second". In an average they skew both halves.
            if !(heart_rate.is_finite() && heart_rate >= drift::MIN_PLAUSIBLE_HR) {
                continue;
            }
            if !(speed.is_finite() && speed >= drift::MIN_PLAUSIBLE_SPEED_MPS) {
                continue;
            }
            let Ok(parsed) = DateTime::parse_from_rfc3339(&timestamp) else {
                continue;
            };
            samples.push(DriftSample {
                unix: parsed.timestamp(),
                heart_rate,
                speed_mps: speed,
            });
        }

        if samples.len() < drift::MIN_SAMPLES_PER_HALF * 2 {
            return Ok(Err("not_enough_samples".into()));
        }

        let start = samples[0].unix;
        let end = samples[samples.len() - 1].unix;
        if end - start < drift::MIN_DURATION_SECONDS {
            return Ok(Err("too_short".into()));
        }

        // Split at the midpoint in **time**, not by sample count. When samples are missing in the middle, a split by count
        // gives the two halves very different durations, and the comparison means nothing.
        let midpoint = start + (end - start) / 2;
        let (first, second): (Vec<&DriftSample>, Vec<&DriftSample>) =
            samples.iter().partition(|sample| sample.unix < midpoint);

        if first.len() < drift::MIN_SAMPLES_PER_HALF || second.len() < drift::MIN_SAMPLES_PER_HALF {
            return Ok(Err("not_enough_samples".into()));
        }

        let speeds: Vec<f64> = samples.iter().map(|sample| sample.speed_mps).collect();
        let speed_mean = speeds.iter().sum::<f64>() / speeds.len() as f64;
        let variance = speeds
            .iter()
            .map(|value| (value - speed_mean).powi(2))
            .sum::<f64>()
            / speeds.len() as f64;
        let speed_cv = if speed_mean > 0.0 {
            variance.sqrt() / speed_mean
        } else {
            f64::INFINITY
        };
        if !(speed_cv.is_finite() && speed_cv <= drift::MAX_SPEED_CV) {
            // Intervals, traffic lights, hills. The two halves are not comparable, and the percentage shows the route,
            // not the body.
            return Ok(Err("pace_too_variable".into()));
        }

        let mean = |half: &[&DriftSample], pick: fn(&DriftSample) -> f64| {
            half.iter().map(|sample| pick(sample)).sum::<f64>() / half.len() as f64
        };
        let first_hr = mean(&first, |sample| sample.heart_rate);
        let second_hr = mean(&second, |sample| sample.heart_rate);
        let first_speed = mean(&first, |sample| sample.speed_mps);
        let second_speed = mean(&second, |sample| sample.speed_mps);

        // Meters per heartbeat. Heart rate is beats per minute, and speed is meters per second.
        let first_eff = first_speed * 60.0 / first_hr;
        let second_eff = second_speed * 60.0 / second_hr;
        if !(first_eff.is_finite() && second_eff.is_finite() && first_eff > 0.0) {
            return Ok(Err("not_enough_samples".into()));
        }

        Ok(Ok(HeartRateDrift {
            first_half_metres_per_beat: first_eff,
            second_half_metres_per_beat: second_eff,
            drift_percent: (second_eff - first_eff) / first_eff * 100.0,
            first_half_avg_hr: first_hr,
            second_half_avg_hr: second_hr,
            first_half_avg_speed_mps: first_speed,
            second_half_avg_speed_mps: second_speed,
            first_half_samples: first.len() as i64,
            second_half_samples: second.len() as i64,
            speed_cv,
        }))
    }

    pub(super) fn run_row(&self, workout_id: &str) -> Result<Option<RunRow>> {
        let row = self.conn.query_row(
            "SELECT workout_id, start_time, end_time, distance_meters, avg_hr,
                    training_load, source_scope
             FROM workouts_visible WHERE workout_id = ?1",
            rusqlite::params![workout_id],
            map_run_row,
        );
        match row {
            Ok(value) => Ok(Some(value?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Select comparable past runs, and return the reason for each exclusion too.
    ///
    /// Users ask "why was that run not counted". An average without the included
    /// range cannot be checked.
    pub(super) fn comparable_runs(
        &self,
        target: &RunRow,
    ) -> Result<(Vec<RunRow>, Vec<BaselineExclusion>)> {
        let Some(target_distance) = target.distance_meters.filter(|value| *value > 0.0) else {
            return Ok((
                Vec::new(),
                vec![BaselineExclusion {
                    workout_id: target.workout_id.clone(),
                    reason: "missing_distance".into(),
                }],
            ));
        };
        let Some(cutoff) = Duration::try_days(baseline::WINDOW_DAYS)
            .and_then(|delta| target.start_time.checked_sub_signed(delta))
        else {
            // Near DateTime::MIN, the 180-day window underflows. No earlier comparable run can exist,
            // so treat it as no baseline. Do not panic.
            return Ok((
                Vec::new(),
                vec![BaselineExclusion {
                    workout_id: target.workout_id.clone(),
                    reason: "outside_window".into(),
                }],
            ));
        };
        let cutoff = cutoff.to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT workout_id, start_time, end_time, distance_meters, avg_hr,
                    training_load, source_scope
             FROM workouts_visible
             WHERE workout_id <> ?1
               AND COALESCE(workout_type_override, workout_type) = 'run'
               AND start_time < ?2
               AND start_time >= ?3
             ORDER BY start_time DESC",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![target.workout_id, target.start_time.to_rfc3339(), cutoff],
            map_run_row,
        )?;

        let low = target_distance * (1.0 - baseline::DISTANCE_TOLERANCE);
        let high = target_distance * (1.0 + baseline::DISTANCE_TOLERANCE);
        let mut included = Vec::new();
        let mut excluded = Vec::new();
        for row in rows {
            let row = row??;
            let reason = match row.distance_meters {
                None => Some("missing_distance"),
                Some(distance) if !(low..=high).contains(&distance) => {
                    Some("distance_out_of_tolerance")
                }
                _ if row.duration_seconds().is_none() => Some("missing_duration"),
                _ if row.pace_seconds_per_km().is_none() => Some("implausible_pace"),
                _ if included.len() >= baseline::MAX_SAMPLES => Some("beyond_max_samples"),
                _ => None,
            };
            match reason {
                Some(reason) => excluded.push(BaselineExclusion {
                    workout_id: row.workout_id.clone(),
                    reason: reason.into(),
                }),
                None => included.push(row),
            }
        }
        Ok((included, excluded))
    }
}

pub(super) fn map_run_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Result<RunRow>> {
    let start: String = row.get(1)?;
    let end: String = row.get(2)?;
    Ok((|| {
        Ok(RunRow {
            workout_id: row.get(0)?,
            start_time: parse_time(&start)?,
            end_time: parse_time(&end)?,
            distance_meters: row.get(3)?,
            avg_hr: row.get(4)?,
            training_load: row.get(5)?,
            source_scope: row.get(6)?,
        })
    })())
}

pub(super) fn parse_time(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|error| {
            crate::models::ZeppBridgeError::ParseError(format!("Invalid workout time: {error}"))
        })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn run_fact<F>(
    fact_id: &str,
    metric: &str,
    unit: &str,
    value: Option<f64>,
    baseline: &[RunRow],
    extract: F,
    window: &BaselineWindow,
    source: &str,
) -> InsightFact
where
    F: Fn(&RunRow) -> Option<f64>,
{
    // The baseline only counts runs that have a value for this metric. A run with no heart rate must not
    // pull the heart rate baseline down, and must not count as 0.
    let mut values = Vec::new();
    let mut refs = Vec::new();
    for row in baseline {
        if let Some(sample) = extract(row) {
            values.push(sample);
            refs.push(row.workout_id.clone());
        }
    }

    let enough = values.len() >= baseline::MIN_SAMPLES;
    let (comparison, reason, reason_code) = match (value, mean(&values)) {
        (Some(current), Some(previous)) if enough && previous != 0.0 => {
            let delta = current - previous;
            (
                Some(Comparison {
                    baseline_value: round1(previous),
                    delta: round1(delta),
                    delta_percent: round1(delta / previous.abs() * 100.0),
                    direction: direction_of(delta, previous),
                }),
                None,
                None,
            )
        }
        // The baseline has enough samples but its mean is 0. A relative change with 0 as the divisor is not possible.
        (Some(_), Some(previous)) if enough && previous == 0.0 => (
            None,
            Some("The previous baseline mean is 0, so a relative change cannot be calculated.".into()),
            Some("workout_zero_baseline".to_string()),
        ),
        (Some(_), _) => (
            None,
            Some(format!(
                "Past runs with a similar distance (+/-{:.0}%) that have this data: only {}, fewer than {}, so only this run's value is reported, with no comparison.",
                baseline::DISTANCE_TOLERANCE * 100.0,
                values.len(),
                baseline::MIN_SAMPLES
            )),
            Some("workout_thin_baseline".to_string()),
        ),
        (None, _) => (
            None,
            Some("This workout has no data for this item.".into()),
            Some("workout_no_value".to_string()),
        ),
    };

    InsightFact {
        fact_id: fact_id.into(),
        metric: metric.into(),
        value: value.map(round1),
        unit: unit.into(),
        comparison,
        baseline_window: Some(window.clone()),
        evidence_count: values.len() as i64,
        source: source.to_string(),
        confidence: if value.is_none() {
            Confidence::Insufficient
        } else {
            Confidence::from_samples(values.len())
        },
        reason,
        reason_code,
        baseline_count: values.len() as i64,
        evidence_refs: refs,
    }
}
