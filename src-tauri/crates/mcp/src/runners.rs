//! Tool execution: workout list, single insight, metric series, sleep detail, data health (the other seven are in browse.rs).

use super::*;

pub(super) fn run_list_workouts(
    db: &Database,
    args: &Value,
    scope: &AccessScope,
    permit: &Permit,
) -> Result<Value, CallFailure> {
    let (limit, offset) = page_args(args, 20, 200);
    let (workouts, has_more) = if scope.is_task_scoped() {
        // Grant by id, not by "the latest N in the whole library": a granted workout must come out however old it is (R4).
        // An id that is granted but deleted or not synced has no detail. Skip it openly.
        let mut granted = Vec::new();
        for workout_id in &permit.workout_ids {
            if let Some(workout) = db
                .get_workout_detail(workout_id)
                .map_err(|error| CallFailure::plain(error.user_message()))?
            {
                granted.push(workout);
            }
        }
        granted.sort_by_key(|workout| std::cmp::Reverse(workout.start_time));
        let has_more = granted.len() > offset.saturating_add(limit);
        (
            granted
                .into_iter()
                .skip(offset)
                .take(limit)
                .collect::<Vec<_>>(),
            has_more,
        )
    } else {
        let mut page = db
            .workouts_page(limit + 1, offset)
            .map_err(|error| CallFailure::plain(error.user_message()))?;
        let has_more = page.len() > limit;
        page.truncate(limit);
        (page, has_more)
    };
    let entries: Vec<Value> = workouts
        .iter()
        .map(|workout| {
            let mut entry = json!({
                "workoutId": workout.workout_id,
                "type": workout.effective_type,
                "customLabel": workout.custom_label,
                "startTime": workout.start_time.to_rfc3339(),
                "endTime": workout.end_time.to_rfc3339(),
                "distanceMeters": workout.distance_meters,
                "calories": workout.calories,
                "avgHr": workout.avg_hr,
                "maxHr": workout.max_hr,
                // Shoe id (Zepp gearId, #143). Only the id, no shoe name.
                "gearId": workout.gear_id,
                "sourceScope": workout.source_scope,
                "gpsAvailable": workout.gps_available,
                "sampleCount": workout.sample_count,
            });
            if scope.is_task_scoped() {
                // Fields the task excluded do not go out (R01).
                access::project_workout_list_entry(
                    &mut entry,
                    &permit.workout_excluded_fields(&workout.workout_id),
                );
            }
            entry
        })
        .collect();
    Ok(json!({
        "workouts": entries,
        "units": { "distance": "m", "heartRate": "bpm", "calories": "kcal" },
        "limit": limit,
        "offset": offset,
        "hasMore": has_more,
        "missingValues": contract::MISSING_VALUE_CONVENTION,
    }))
}

pub(super) fn run_workout_insight(
    db: &Database,
    args: &Value,
    scope: &AccessScope,
    permit: &Permit,
) -> Result<Value, CallFailure> {
    let workout_id = args
        .get("workoutId")
        .and_then(Value::as_str)
        .ok_or_else(|| CallFailure::plain("missing workoutId"))?;
    let mut insight = db
        .workout_insight(workout_id)
        .map_err(|error| CallFailure::plain(error.user_message()))?;
    if scope.is_task_scoped() {
        // R1: the original insight baseline is computed over the whole library. included/excluded would leak the ids,
        // dates and distances of ungranted workouts, and the aggregate facts average ungranted samples too.
        // Fetch the baseline rows that are still in the granted set and recompute over the granted set.
        let candidate_ids: BTreeSet<String> = insight
            .baseline_included
            .iter()
            .map(|entry| entry.workout_id.clone())
            .chain(
                insight
                    .baseline_excluded
                    .iter()
                    .map(|entry| entry.workout_id.clone()),
            )
            .filter(|id| permit.workout_ids.contains(id))
            // The baseline picks comparable records by distance: workouts with distance excluded stay out of the baseline, or their
            // distance would leak through the baseline mean (R01).
            .filter(|id| {
                !permit
                    .workout_excluded_fields(id)
                    .contains("distance_meters")
            })
            .collect();
        let mut rows = BTreeMap::new();
        for workout_id in candidate_ids {
            if let Some(workout) = db
                .get_workout_detail(&workout_id)
                .map_err(|error| CallFailure::plain(error.user_message()))?
            {
                rows.insert(workout_id, access::GrantedRun::from_workout(&workout));
            }
        }
        let baseline_ids: BTreeSet<String> = rows.keys().cloned().collect();
        access::rescore_insight(&mut insight, &baseline_ids, &rows);
        access::project_workout_insight(&mut insight, &permit.workout_excluded_fields(workout_id));
    }
    serde_json::to_value(&insight)
        .map_err(|error| CallFailure::plain(format!("serialization failed: {error}")))
}

pub(super) fn run_metric_series(
    db: &Database,
    args: &Value,
    scope: &AccessScope,
    permit: &Permit,
) -> Result<Value, CallFailure> {
    let metrics = metric_args(args).map_err(CallFailure::plain)?;
    let days = args
        .get("days")
        .and_then(Value::as_i64)
        .unwrap_or(90)
        .clamp(1, 1825);
    let mut series = db
        .metric_series(&metrics, days)
        .map_err(|error| CallFailure::plain(error.user_message()))?;
    if scope.is_task_scoped() {
        // authorize already ensures every requested category overlaps a grant window. Here we clip the points to
        // granted days and recompute latest/average/days_with_data/window_days, so the series
        // describes exactly the granted range it covers (R3).
        series = access::clip_metric_series(series, permit);
    }
    Ok(json!({
        "series": serde_json::to_value(&series)
            .map_err(|error| CallFailure::plain(format!("serialization failed: {error}")))?,
        "requestedMetrics": metrics,
        "missingValues": contract::MISSING_VALUE_CONVENTION,
        "time": contract::TIME_CONVENTION,
    }))
}

pub(super) fn run_baselines(db: &Database, args: &Value) -> Result<Value, CallFailure> {
    let metrics = metric_args(args).map_err(CallFailure::plain)?;
    let norms = db
        .metric_norms(&metrics, Local::now().date_naive())
        .map_err(|error| CallFailure::plain(error.user_message()))?;
    Ok(json!({
        "baselines": serde_json::to_value(&norms)
            .map_err(|error| CallFailure::plain(format!("serialization failed: {error}")))?,
        "requestedMetrics": metrics,
        "missingValues": contract::MISSING_VALUE_CONVENTION,
        "time": contract::TIME_CONVENTION,
    }))
}

pub(super) fn run_sleep_detail(
    db: &Database,
    args: &Value,
    scope: &AccessScope,
    permit: &Permit,
) -> Result<Value, CallFailure> {
    let session = match args.get("sleepId").and_then(Value::as_str) {
        Some(id) => {
            let detail = db
                .get_sleep_detail(id)
                .map_err(|error| CallFailure::plain(error.user_message()))?;
            match detail {
                // Sleep belongs to the local day of waking (same basis as the sleep list/insight);
                // deny it if it falls outside the window (R2).
                Some(session) if scope.is_task_scoped() => {
                    let day = session.end_time.with_timezone(&Local).date_naive();
                    if !permit.day_permitted(AccessCategory::Sleep, day) {
                        return Err(CallFailure::denied(
                            "This night of sleep is not inside the grant window of any shared task.",
                            windows_json(&permit.windows),
                        ));
                    }
                    Some(session)
                }
                other => other,
            }
        }
        None => {
            if scope.is_task_scoped() {
                // In task scope, "the latest night" = the latest night inside the grant window, not the newest in the whole library
                // (R2). Newer records outside the window must not be seen; if the window has none, deny openly
                // instead of falling back to the newest in the whole library.
                let sleep_id = access::latest_sleep_in_windows(db, permit)
                    .map_err(|error| CallFailure::plain(error.user_message()))?;
                match sleep_id {
                    Some(sleep_id) => db
                        .get_sleep_detail(&sleep_id)
                        .map_err(|error| CallFailure::plain(error.user_message()))?,
                    None => {
                        return Err(CallFailure::denied(
                            "There is no sleep record inside the grant window yet.",
                            windows_json(&permit.windows),
                        ));
                    }
                }
            } else {
                let latest = db
                    .get_recent_sleep_sessions(1)
                    .map_err(|error| CallFailure::plain(error.user_message()))?
                    .into_iter()
                    .next();
                // The list deliberately omits stages; load the same detail
                // as an explicit sleepId instead of returning that summary.
                match latest {
                    Some(session) => db
                        .get_sleep_detail(&session.sleep_id)
                        .map_err(|error| CallFailure::plain(error.user_message()))?,
                    None => None,
                }
            }
        }
    };
    Ok(match session {
        Some(session) => {
            let mut sleep = serde_json::to_value(&session)
                .map_err(|error| CallFailure::plain(format!("serialization failed: {error}")))?;
            if scope.is_task_scoped() {
                // Sleep fields the task excluded do not go out (R01).
                let day = session.end_time.with_timezone(&Local).date_naive();
                access::project_sleep_fields(&mut sleep, &permit.sleep_excluded_fields(day));
            }
            json!({
                "sleep": sleep,
                "units": { "stageMinutes": "min", "heartRate": "bpm" },
                "missingValues": contract::MISSING_VALUE_CONVENTION,
            })
        }
                // "This computer has no such night" and "this night has no data" are the same statement:
                // do not return an empty shell with every value at 0.
        None => json!({ "sleep": Value::Null, "reason": "There is no matching sleep record on this computer." }),
    })
}

pub(super) fn run_data_health(
    db: &Database,
    args: &Value,
    database_bytes: u64,
) -> Result<Value, CallFailure> {
    let window = args
        .get("windowDays")
        .and_then(Value::as_i64)
        .unwrap_or(30)
        .clamp(1, 365);
    let health = db
        .data_health(window, database_bytes)
        .map_err(|error| CallFailure::plain(error.user_message()))?;
    serde_json::to_value(health).map_err(|error| CallFailure::plain(format!("serialization failed: {error}")))
}
