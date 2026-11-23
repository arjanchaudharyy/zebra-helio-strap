//! Normalizer main entry: heart rate, band data, workouts, HRV, daily summaries (split out of normalizer/mod.rs, logic unchanged).

use super::*;

impl Normalizer {
    pub fn normalize_heart_rate(raw: &Value) -> Result<Vec<MetricSample>> {
        Self::normalize_heart_rate_with_diagnostics(raw)?.into_result("heart_rate")
    }

    pub fn normalize_heart_rate_with_diagnostics(
        raw: &Value,
    ) -> Result<NormalizedBatch<MetricSample>> {
        let items = extract_items(raw)?;
        let mut records = Vec::new();
        let mut diagnostics = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let object = item_object(item);
            let timestamp = object
                .and_then(|o| first_value(o, &["timestamp", "time", "timeStamp", "startTime"]))
                .and_then(parse_timestamp);
            let value =
                object.and_then(|o| first_number(o, &["value", "heartRate", "heart_rate", "hr"]));
            let (Some(timestamp), Some(value)) = (timestamp, value) else {
                // The second observed shape: `generatedTime` + base64 `heartRateData`.
                match object.and_then(packed_heart_rate_point) {
                    Some(Ok((timestamp, value))) => {
                        // 0 bpm is the "not measured" sentinel, not a real heartbeat. Same rule as the main path.
                        if !value.is_finite() || !(1.0..=300.0).contains(&value) {
                            diagnostics.push(format!("item {index}: heart rate invalid value"));
                            continue;
                        }
                        let device_id = object.and_then(device_id);
                        records.push(MetricSample {
                            metric: "heart_rate".into(),
                            timestamp,
                            value,
                            unit: "bpm".into(),
                            source_scope: source_scope(object, device_id.as_deref()),
                            device_id,
                        });
                        continue;
                    }
                    Some(Err(diagnostic)) => {
                        diagnostics.push(diagnostic);
                        continue;
                    }
                    None => {}
                }
                diagnostics.push(format!("item {index}: missing timestamp/value"));
                continue;
            };
            // 0 bpm is the "not measured" sentinel, not a real heartbeat. A 0 in score metrics must not use this rule.
            if !value.is_finite() || !(1.0..=300.0).contains(&value) {
                diagnostics.push(format!("item {index}: heart rate invalid value"));
                continue;
            }
            let device_id = object.and_then(device_id);
            records.push(MetricSample {
                metric: "heart_rate".into(),
                timestamp,
                value,
                unit: "bpm".into(),
                source_scope: source_scope(object, device_id.as_deref()),
                device_id,
            });
        }
        Ok(NormalizedBatch {
            records,
            diagnostics,
            capability: CapabilityStatus::Verified,
        })
    }

    pub fn normalize_band_data(raw: &Value) -> Result<BandNormalizedData> {
        Self::normalize_band_data_in_zone(raw, None)
    }

    /// Same as [`Self::normalize_band_data`], plus the device IANA time zone (`device_identities.timezone`),
    /// used to correct the hour that the summary `tz` misses during DST (see [`effective_band_offset`]).
    pub fn normalize_band_data_in_zone(
        raw: &Value,
        zone: Option<&str>,
    ) -> Result<BandNormalizedData> {
        let items = extract_items(raw)?;
        let mut sleep_sessions = Vec::new();
        let mut heart_rate_samples = Vec::new();
        let mut daily_metrics = Vec::new();
        let mut diagnostics = Vec::new();

        for (index, item) in items.iter().enumerate() {
            let Some(object) = item.as_object() else {
                diagnostics.push(format!("item {index}: not an object"));
                continue;
            };
            let source_device = device_id(object);
            let source_scope = if source_device.is_some() {
                SourceScope::Device
            } else {
                SourceScope::Unknown
            };

            let decoded_summary =
                object
                    .get("summary")
                    .and_then(Value::as_str)
                    .and_then(|encoded| match decode_base64_json(encoded) {
                        Ok(value) => Some(value),
                        Err(error) => {
                            diagnostics.push(format!("item {index}: summary decode failed: {error}"));
                            None
                        }
                    });

            if let Some(summary) = decoded_summary.as_ref().and_then(Value::as_object) {
                if let Some(sleep) = summary.get("slp").and_then(Value::as_object) {
                    match sleep_from_band_item(object, summary, sleep, source_scope.clone(), zone) {
                        Ok(session) => {
                            if session.stages.is_empty()
                                && sleep
                                    .get("stage")
                                    .and_then(Value::as_array)
                                    .is_some_and(|stages| !stages.is_empty())
                                && first_value(summary, &["tz"]).is_none()
                            {
                                diagnostics.push(format!(
                                    "item {index}: sleep stages missing tz; did not invent times from UTC"
                                ));
                            }
                            sleep_sessions.push(session);
                        }
                        Err(message) => diagnostics.push(format!("item {index}: {message}")),
                    }
                }
                daily_metrics.extend(daily_metrics_from_band_summary(
                    object,
                    summary,
                    source_scope.clone(),
                ));
            } else if let Some(session) = sleep_from_flat_object(object) {
                sleep_sessions.push(session);
            }

            match heart_rate_from_band_item(object, decoded_summary.as_ref(), zone) {
                Ok(samples) => heart_rate_samples.extend(samples),
                Err(message) => diagnostics.push(format!("item {index}: {message}")),
            }
        }

        let capability = if sleep_sessions.is_empty()
            && heart_rate_samples.is_empty()
            && daily_metrics.is_empty()
        {
            CapabilityStatus::Unverified
        } else {
            CapabilityStatus::Verified
        };

        Ok(BandNormalizedData {
            sleep_sessions,
            heart_rate_samples,
            daily_metrics,
            diagnostics,
            capability,
        })
    }

    pub fn normalize_workouts_with_sport(raw: &Value, sport: Option<&str>) -> Result<Vec<Workout>> {
        Self::normalize_workouts_with_diagnostics_and_sport(raw, sport)?.into_result("workouts")
    }

    pub(super) fn normalize_workouts_with_diagnostics_and_sport(
        raw: &Value,
        sport: Option<&str>,
    ) -> Result<NormalizedBatch<Workout>> {
        Self::normalize_workouts_with_context_batch(raw, sport, |_| Ok(None))
    }

    /// Resolve a stored device identity only for the identifier carried by
    /// this workout. Missing identifiers deliberately do not call the resolver.
    pub fn normalize_workouts_with_device_context(
        raw: &Value,
        sport: Option<&str>,
        resolver: impl FnMut(&str) -> Result<Option<String>>,
    ) -> Result<Vec<Workout>> {
        Self::normalize_workouts_with_context_batch(raw, sport, resolver)?.into_result("workouts")
    }

    fn normalize_workouts_with_context_batch(
        raw: &Value,
        _sport: Option<&str>,
        mut resolver: impl FnMut(&str) -> Result<Option<String>>,
    ) -> Result<NormalizedBatch<Workout>> {
        let items = extract_items(raw)?;
        let mut records = Vec::new();
        let mut diagnostics = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let Some(object) = item_object(item) else {
                diagnostics.push(format!("item {index}: not an object"));
                continue;
            };
            let start = first_value(object, &["start_time", "startTime", "beginTime", "trackid"])
                .and_then(parse_timestamp);
            let end = first_value(object, &["end_time", "endTime", "finishTime"])
                .and_then(parse_timestamp);
            let (Some(start_time), Some(end_time)) = (start, end) else {
                diagnostics.push(format!("item {index}: missing workout start/end"));
                continue;
            };
            if end_time <= start_time {
                diagnostics.push(format!("item {index}: workout end is not after start"));
                continue;
            }
            // The endpoint path is fetch provenance, not type evidence.
            // `/v1/sport/run/history.json` can return every activity, so using
            // `sport == run` here silently relabels unknown strength/custom
            // codes as outdoor runs. Keep unknown numeric facts explicit.
            let zepp_type =
                first_number(object, &["type", "sport_mode"]).map(|value| value.round() as i32);
            let explicit_type = first_string(
                object,
                &[
                    "workout_type",
                    "sportType",
                    "sport_title",
                    "sportTitle",
                    "sport_name",
                    "sportName",
                ],
            );
            let (workout_type, type_source) = if let Some(code) = zepp_type {
                let device_source =
                    first_number(object, &["deviceSource", "device_source"]).map(|v| v as i64);
                let device_name = first_string(object, &["deviceName", "productName", "modelName"]);
                let local_device = if code == 15 {
                    match device_id(object) {
                        Some(id) => resolver(&id)?,
                        None => None,
                    }
                } else {
                    None
                };
                match crate::sport_catalog::resolve_cloud_with_device(
                    i64::from(code),
                    device_source,
                    device_name.as_deref(),
                    local_device.as_deref(),
                ) {
                    Some(mapped) => (mapped.to_owned(), "numeric_mapped".to_owned()),
                    None => match explicit_type {
                        Some(value) => (normalize_type_text(&value), "string_field".to_owned()),
                        None => (format!("unknown:{code}"), "unknown_code".to_owned()),
                    },
                }
            } else if let Some(value) = explicit_type {
                (normalize_type_text(&value), "string_field".to_owned())
            } else {
                ("unknown".to_owned(), "missing".to_owned())
            };
            let workout_id = first_string(
                object,
                &["workout_id", "workoutId", "trackId", "trackid", "id"],
            )
            .unwrap_or_else(|| {
                // Stable fallback for responses that omit an official id.
                format!(
                    "{workout_type}:{}:{}",
                    start_time.timestamp(),
                    end_time.timestamp()
                )
            });
            let source_device = device_id(object);
            records.push(Workout {
                workout_id,
                workout_type: workout_type.clone(),
                normalized_type: workout_type.clone(),
                type_source,
                user_override: None,
                effective_type: workout_type,
                custom_label: None,
                start_time,
                end_time,
                distance_meters: first_number(
                    object,
                    &["distance_meters", "distanceMeters", "distance", "dis"],
                ),
                // Same rule as min_hr/total_steps and the other fields below: a negative number is the "not measured"
                // sentinel, not a real reading. Filter it out; do not store it as is.
                calories: first_number(object, &["calories", "calorie"])
                    .filter(|value| *value >= 0.0)
                    .map(|v| v as i32),
                avg_hr: first_number(
                    object,
                    &["avg_hr", "avgHr", "averageHeartRate", "avg_heart_rate"],
                )
                .filter(|value| *value > 0.0)
                .map(|v| v as i32),
                max_hr: first_number(
                    object,
                    &["max_hr", "maxHr", "maximumHeartRate", "max_heart_rate"],
                )
                .filter(|value| *value > 0.0)
                .map(|v| v as i32),
                // Zepp reports "not measured" as a negative sentinel, and only
                // running-type activities produce VO2 max at all: `-1` covers
                // 103 of 172 local workouts. Keeping it would hand downstream
                // readers a fabricated number, so it becomes null; the raw
                // payload still holds the original value.
                training_load: first_number(
                    object,
                    &[
                        "training_load",
                        "trainingLoad",
                        "trainLoad",
                        "exercise_load",
                    ],
                )
                .filter(|value| *value >= 0.0),
                vo2max: first_number(object, &["vo2max", "vo2Max", "VO2_MAX", "VO2_max"])
                    .filter(|value| *value > 0.0),
                // The cloud always sent the fields below, but before we read none of them. Each one has
                // its own "not measured" sentinel, so each is written out; they do not share one rule.
                min_hr: first_number(object, &["min_heart_rate", "minHeartRate", "min_hr"])
                    .filter(|value| *value > 0.0)
                    .map(|value| value as i32),
                // 0 steps is a fact for a ride, but "not measured" for a walk. We cannot tell them apart, so
                // accept only positive numbers. A workout with truly 0 steps has nothing to show anyway.
                total_steps: first_number(object, &["total_step", "totalStep", "steps"])
                    .filter(|value| *value > 0.0)
                    .map(|value| value as i32),
                moving_seconds: first_number(object, &["run_time", "runTime", "sportTime"])
                    .filter(|value| *value > 0.0)
                    .map(|value| value as i64),
                // The cloud gives two values: `elevationGain` in centimeters and `altitude_ascend` in rounded
                // meters. Prefer the centimeter value; it was not rounded early.
                elevation_gain_m: first_number(object, &["elevationGain", "elevation_gain"])
                    .filter(|value| *value >= 0.0)
                    .map(|value| value / 100.0)
                    .or_else(|| {
                        first_number(object, &["altitude_ascend", "altitudeAscend"])
                            .filter(|value| *value >= 0.0)
                    }),
                elevation_loss_m: first_number(object, &["elevationLoss", "elevation_loss"])
                    .filter(|value| *value >= 0.0)
                    .map(|value| value / 100.0)
                    .or_else(|| {
                        first_number(object, &["altitude_descend", "altitudeDescend"])
                            .filter(|value| *value >= 0.0)
                    }),
                // Altitude is also in centimeters. It matches the parsed per-second series in tests: one walk had
                // `highestAltitude` 9178 cm, series max 91.78 m.
                max_altitude_m: first_number(object, &["highestAltitude", "max_altitude"])
                    .filter(|value| value.is_finite())
                    .map(|value| value / 100.0),
                min_altitude_m: first_number(object, &["lowestAltitude", "min_altitude"])
                    .filter(|value| value.is_finite())
                    .map(|value| value / 100.0),
                // Training effect is stored as ten times an integer: 22 means 2.2.
                training_effect: first_number(object, &["te", "trainingEffect"])
                    .filter(|value| *value > 0.0)
                    .map(|value| value / 10.0),
                anaerobic_training_effect: first_number(
                    object,
                    &["anaerobic_te", "anaerobicTrainingEffect"],
                )
                .filter(|value| *value > 0.0)
                .map(|value| value / 10.0),
                rpe: first_number(object, &["rpe"])
                    .filter(|value| *value > 0.0)
                    .map(|value| value as i32),
                // Cadence is in steps per minute; it was checked against the cloud `max_frequency` / `avg_stride_length`,
                // see the table in export_fit.
                avg_cadence_spm: first_number(object, &["avg_frequency", "avgFrequency"])
                    .filter(|value| *value > 0.0),
                max_cadence_spm: first_number(object, &["max_frequency", "maxFrequency"])
                    .filter(|value| *value > 0.0),
                avg_stride_cm: first_number(object, &["avg_stride_length", "avgStrideLength"])
                    .filter(|value| *value > 0.0),
                // With no shoe selected, the cloud gives an empty string; treat it as none.
                gear_id: first_string(object, &["gearId", "gear_id"])
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                hr_zones: parse_heart_range(first_string(object, &["heart_range"]).as_deref()),
                source_scope: source_scope(Some(object), source_device.as_deref()),
                device_id: source_device,
                synced_at: None,
                // The geohash `location` is not a track. The history summary has no lat/lon points.
                gps_available: workout_has_track_geometry(object),
                sample_count: workout_sample_count(object),
                zepp_source: first_string(object, &["source"]),
                zepp_type,
            });
        }
        Ok(NormalizedBatch {
            records,
            diagnostics,
            capability: CapabilityStatus::Verified,
        })
    }

    pub fn extract_workout_strength_summaries(
        raw: &Value,
    ) -> BTreeMap<String, crate::decoder::strength::HistoryStrengthSummary> {
        let Ok(items) = extract_items(raw) else {
            return BTreeMap::new();
        };
        let mut out = BTreeMap::new();
        for item in items {
            let Some(object) = item_object(item) else {
                continue;
            };
            let start = first_value(object, &["start_time", "startTime", "beginTime", "trackid"])
                .and_then(parse_timestamp);
            let end = first_value(object, &["end_time", "endTime", "finishTime"])
                .and_then(parse_timestamp);
            let (Some(start_time), Some(end_time)) = (start, end) else {
                continue;
            };
            if end_time <= start_time {
                continue;
            }
            let zepp_type =
                first_number(object, &["type", "sport_mode"]).map(|value| value.round() as i32);
            let explicit_type = first_string(
                object,
                &[
                    "workout_type",
                    "sportType",
                    "sport_title",
                    "sportTitle",
                    "sport_name",
                    "sportName",
                ],
            );
            let workout_type = if let Some(code) = zepp_type {
                let device_source =
                    first_number(object, &["deviceSource", "device_source"]).map(|v| v as i64);
                let device_name = first_string(object, &["deviceName", "productName", "modelName"]);
                match crate::sport_catalog::resolve_cloud_with_device(
                    i64::from(code),
                    device_source,
                    device_name.as_deref(),
                    None,
                ) {
                    Some(mapped) => mapped.to_owned(),
                    None => match explicit_type {
                        Some(value) => normalize_type_text(&value),
                        None => format!("unknown:{code}"),
                    },
                }
            } else if let Some(value) = explicit_type {
                normalize_type_text(&value)
            } else {
                "unknown".to_owned()
            };

            let groups = crate::decoder::strength::parse_history_strength_groups(first_value(
                object,
                &["strength_training_group", "strengthTrainingGroup"],
            ));
            let is_strength =
                zepp_type == Some(52) || workout_type == "strength" || !groups.is_empty();
            if !is_strength {
                continue;
            }

            let workout_id = first_string(
                object,
                &["workout_id", "workoutId", "trackId", "trackid", "id"],
            )
            .unwrap_or_else(|| {
                format!(
                    "{workout_type}:{}:{}",
                    start_time.timestamp(),
                    end_time.timestamp()
                )
            });

            let total_sets = first_number(
                object,
                &["total_group", "totalGroup", "strength_total_sets"],
            )
            .filter(|value| *value > 0.0)
            .map(|value| value.round() as i32)
            .or_else(|| (!groups.is_empty()).then_some(groups.len() as i32));

            let total_reps =
                first_number(object, &["strength_total_reps", "total_reps", "totalReps"])
                    .filter(|value| *value >= 0.0)
                    .map(|value| value.round() as i32)
                    .or_else(|| {
                        (!groups.is_empty()).then(|| groups.iter().map(|g| g.reps.max(0)).sum())
                    });

            let rest_seconds = first_number(
                object,
                &[
                    "restTimeBetweenGroupsWithMillis",
                    "rest_time_between_groups_with_millis",
                ],
            )
            .filter(|value| *value >= 0.0)
            .map(|value| (value / 1000.0).round() as i64)
            .or_else(|| {
                first_number(
                    object,
                    &[
                        "rope_skipping_rest_time",
                        "ropeSkippingRestTime",
                        "rest_seconds",
                        "restTime",
                    ],
                )
                .filter(|value| *value >= 0.0)
                .map(|value| value.round() as i64)
            });

            let groups_json = (!groups.is_empty())
                .then(|| serde_json::to_string(&groups).ok())
                .flatten();

            if total_sets.is_some()
                || total_reps.is_some()
                || rest_seconds.is_some()
                || groups_json.is_some()
            {
                out.insert(
                    workout_id,
                    crate::decoder::strength::HistoryStrengthSummary {
                        total_sets,
                        total_reps,
                        rest_seconds,
                        groups_json,
                    },
                );
            }
        }
        out
    }

    pub fn normalize_hrv(raw: &Value) -> Result<Vec<MetricSample>> {
        Self::normalize_hrv_with_diagnostics(raw)?.into_result("hrv")
    }

    pub fn normalize_hrv_with_diagnostics(raw: &Value) -> Result<NormalizedBatch<MetricSample>> {
        let items = extract_items(raw)?;
        let mut records = Vec::new();
        let mut diagnostics = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let Some(object) = item_object(item) else {
                diagnostics.push(format!("item {index}: not an object"));
                continue;
            };
            if let Some(event_value) = object.get("value").and_then(Value::as_object) {
                if let Some(samples) = event_value.get("samples").and_then(Value::as_array) {
                    let base = first_value(event_value, &["startTime", "start_time"])
                        .and_then(parse_timestamp);
                    let source_device = device_id(event_value).or_else(|| device_id(object));
                    for (sample_index, sample) in samples.iter().enumerate() {
                        let Some(sample) = sample.as_object() else {
                            diagnostics
                                .push(format!("item {index} sample {sample_index}: not an object"));
                            continue;
                        };
                        let timestamp = first_value(sample, &["timestamp", "time"])
                            .and_then(parse_timestamp)
                            .or_else(|| {
                                let offset_ms = first_number(sample, &["s", "offset"])? as i64;
                                base.and_then(|value| add_milliseconds(value, offset_ms))
                            });
                        let hrv = first_value(sample, &["sdnn", "rmssd", "hrv", "value"])
                            .and_then(parse_number);
                        let (Some(timestamp), Some(sample_value)) = (timestamp, hrv) else {
                            diagnostics.push(format!(
                                "item {index} sample {sample_index}: missing HRV timestamp/value"
                            ));
                            continue;
                        };
                        if sample_value.is_finite() && sample_value >= 0.0 {
                            records.push(MetricSample {
                                metric: "hrv".into(),
                                timestamp,
                                value: sample_value,
                                unit: "ms".into(),
                                source_scope: source_scope(
                                    Some(event_value),
                                    source_device.as_deref(),
                                ),
                                device_id: source_device.clone(),
                            });
                        }
                    }
                    continue;
                }
            }
            let timestamp = first_value(object, &["timestamp", "time", "date", "dayId"])
                .and_then(parse_timestamp_or_date);
            let value =
                first_value(object, &["value", "hrv", "sdnn", "rmssd"]).and_then(parse_number);
            let (Some(timestamp), Some(value)) = (timestamp, value) else {
                diagnostics.push(format!("item {index}: missing HRV timestamp/value"));
                continue;
            };
            if !value.is_finite() || value < 0.0 {
                diagnostics.push(format!("item {index}: HRV invalid value"));
                continue;
            }
            let source_device = device_id(object);
            records.push(MetricSample {
                metric: "hrv".into(),
                timestamp,
                value,
                unit: "ms".into(),
                source_scope: source_scope(Some(object), source_device.as_deref()),
                device_id: source_device,
            });
        }
        Ok(NormalizedBatch {
            records,
            diagnostics,
            capability: CapabilityStatus::Verified,
        })
    }

    pub fn normalize_daily_summary(raw: &Value) -> Result<Vec<DailyMetric>> {
        let batch = Self::normalize_daily_summary_with_diagnostics(raw)?;
        // All entries are "no Charge score that day": recognized, just without a value. After per-day storage this
        // is common (one or two days a month), and must not count as a parse failure.
        if batch.records.is_empty()
            && extract_items(raw)?
                .iter()
                .all(|item| item.as_object().is_some_and(is_scoreless_charge_day))
        {
            return Ok(Vec::new());
        }
        batch.into_result("daily_summary")
    }

    pub fn normalize_daily_summary_with_diagnostics(
        raw: &Value,
    ) -> Result<NormalizedBatch<DailyMetric>> {
        let items = extract_items(raw)?;
        let mut indexed_items = items.iter().enumerate().collect::<Vec<_>>();
        // Entries with an explicit calendar day go last: canonical rows are overwritten in insert order, and date-only must not lose to
        // a fallback that has only an epoch and is split by UTC.
        indexed_items.sort_by_key(|(_, item)| daily_summary_sort_key(item));
        let mut records = Vec::new();
        let mut diagnostics = Vec::new();
        for (index, item) in indexed_items {
            let Some(object) = item.as_object() else {
                diagnostics.push(format!("item {index}: not an object"));
                continue;
            };
            let event_value = object.get("value").and_then(Value::as_object);
            let mut item_count = 0usize;
            let event_type = first_string(object, &["eventType"]);
            if event_type.as_deref() == Some("Charge") {
                if let Some(value) = event_value {
                    item_count += collect_charge_metrics(object, value, &mut records);
                }
            } else if let Some(samples) = event_value
                .and_then(|value| value.get("samples"))
                .and_then(Value::as_array)
            {
                let parent_device = event_value
                    .and_then(device_id)
                    .or_else(|| device_id(object));
                for sample in samples {
                    let Some(sample) = sample.as_object() else {
                        continue;
                    };
                    item_count += collect_daily_metrics(
                        sample,
                        Some(object),
                        parent_device.clone(),
                        &mut records,
                    );
                }
            } else {
                item_count += collect_daily_metrics(
                    object,
                    event_value,
                    event_value
                        .and_then(device_id)
                        .or_else(|| device_id(object)),
                    &mut records,
                );
            }
            if item_count == 0 {
                diagnostics.push(format!("item {index}: no known daily metric field"));
            }
        }
        let mut canonical = BTreeMap::new();
        for record in records {
            let key = (
                record.date.clone(),
                record.metric.clone(),
                record.source_scope.as_str().to_string(),
                record.device_id.clone().unwrap_or_default(),
            );
            canonical.insert(key, record);
        }
        Ok(NormalizedBatch {
            records: canonical.into_values().collect(),
            diagnostics,
            capability: CapabilityStatus::Verified,
        })
    }

    pub fn band_capability(raw: &Value) -> CapabilityStatus {
        Self::normalize_band_data(raw)
            .map(|result| result.capability)
            .unwrap_or(CapabilityStatus::Unverified)
    }
}

/// The second observed item shape of `/users/{id}/heartRate` (2026-10-06, one real account,
/// see main #124):
///
/// ```json
/// { "items": [ { "deviceId": "D8...", "deviceSource": 10551552,
///                "generatedTime": 1791095423, "heartRateData": "Xg==",
///                "timeZone": "8", "type": 2 } ] }
/// ```
///
/// `heartRateData` is a base64 byte string. Only one meaning is verified: **one byte is one
/// reading** (`"Xg=="` = `[0x5E]` = 94 bpm, matching a single watch measurement at the same time).
/// Multiple bytes have no reference sample, so only one byte is accepted; multiple bytes give a diagnostic and are skipped.
///
/// `generatedTime` is Unix **seconds** (1791095423 = 2026-10-04T06:30:23Z);
/// the seconds/milliseconds boundary in `parse_timestamp` handles it.
///
/// Returns `None` = not this shape; the original `missing timestamp/value` diagnostic applies.
fn packed_heart_rate_point(
    object: &Map<String, Value>,
) -> Option<std::result::Result<(DateTime<Utc>, f64), String>> {
    let encoded = first_value(object, &["heartRateData"]).and_then(Value::as_str)?;
    let timestamp = first_number(object, &["generatedTime", "createTime"])
        .filter(|value| value.is_finite() && *value > 0.0)
        .and_then(|seconds| parse_timestamp(&Value::from(seconds)))?;
    let bytes = match STANDARD.decode(encoded.trim()) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Some(Err(format!("heartRateData is not valid base64: {error}")));
        }
    };
    let [reading] = bytes.as_slice() else {
        return Some(Err(format!(
            "heartRateData has {} bytes; only single-byte readings are verified; not parsed",
            bytes.len()
        )));
    };
    Some(Ok((timestamp, f64::from(*reading))))
}

/// Numeric `type` of a Zepp workout summary -> canonical sport name.
/// 1/6/9/223 were checked one by one against real Zepp app records (run/walk/ride/AI activity);
/// 8/10/14/23/92 come from a community reference implementation; 13/22/192 are inferred from local record
/// traits (cadence/stride/pace/heart rate). They can be corrected when new device codes appear.
pub(super) fn normalize_type_text(value: &str) -> String {
    if let Some(key) = crate::sport_catalog::text_alias(value) {
        return key.to_owned();
    }
    let normalized = value.trim().to_lowercase().replace([' ', '-'], "_");
    if normalized.is_empty() {
        "unknown".to_owned()
    } else {
        normalized
    }
}
