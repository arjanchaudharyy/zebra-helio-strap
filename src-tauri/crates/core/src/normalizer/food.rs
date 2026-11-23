//! Food records: per-meal samples and per-day macronutrients (split out of normalizer/mod.rs).

use super::*;

/// Food records: `/v2/users/me/events?eventType=Food`, with no subType.
///
/// The day-bucket `value.samples[]` shape and its nutrient fields are documented
/// from a captured Zepp response at
/// https://github.com/lcanis/zepp-food-extractor/blob/main/docs/api-notes.md.
/// We still have no raw
/// response from the #103 reporter, so other regional or Health Connect shapes
/// remain unverified. Unknown fields are diagnosed and raw responses retained.
///
/// Writes `daily_metrics`, not `metric_samples`: a day has several meals, but what matters is "how much
/// was eaten today". Per-meal calories alone do not make a curve to compare with activity, and
/// u/Vast_Snow_5216 asked for exactly "the correlation between nutrition, recovery and activity".
/// One macronutrient: metric name, possible field names, unit, and the range a real reading can
/// fall in.
///
/// As with weight, the names come from the ecosystem, not from payloads we have seen, so each value must
/// fall in its range to be used. 40000 kcal in one day is not calories; it is something else with the same name.
pub(super) struct Macro {
    pub(super) metric: &'static str,
    pub(super) names: &'static [&'static str],
    pub(super) unit: &'static str,
    pub(super) range: (f64, f64),
}

/// One body-composition metric: what to call it, which server field names have
/// been seen to carry it, its unit, and the range a real human reading falls in.
///
/// The range is not decoration. Only `weight`, `bmi` and `height` were read off
/// a live account - that account has no scale, so the fat / muscle / water
/// fields a scale adds were not there to look at, and their names come from the
/// wider Zepp ecosystem rather than from a response anyone here has seen.
/// Publishing a health reading under a guessed name is worse than publishing
/// nothing, so every unconfirmed field has to land inside a range only that
/// metric can occupy. A `fatRate` that comes back as 31.4 is a body-fat
/// percentage; one that comes back as 1980 is something else wearing the same
/// name, and it gets dropped and reported rather than charted.
pub(super) struct BodyMetric {
    pub(super) metric: &'static str,
    pub(super) aliases: &'static [&'static str],
    pub(super) unit: &'static str,
    pub(super) range: (f64, f64),
}

pub(super) fn food_metrics(items: &[Value], out: &mut WellnessNormalizedData) {
    const MACROS: [Macro; 4] = [
        Macro {
            metric: "intake_calories",
            names: &["calories", "calorie", "kcal", "energy"],
            unit: "kcal",
            range: (1.0, 20000.0),
        },
        Macro {
            metric: "intake_protein_g",
            names: &["protein", "proteins"],
            unit: "g",
            range: (0.0, 1000.0),
        },
        Macro {
            metric: "intake_fat_g",
            names: &["fatTotal", "fat", "fats"],
            unit: "g",
            range: (0.0, 1000.0),
        },
        Macro {
            metric: "intake_carbs_g",
            names: &["carbohydrate", "carbohydrates", "carbs"],
            unit: "g",
            range: (0.0, 2000.0),
        },
    ];

    // Sum per day: a day can have several records (one each for breakfast, lunch and dinner, or one per meal).
    let mut per_day: BTreeMap<(String, &'static str), (f64, &'static str)> = BTreeMap::new();
    let mut unknown: BTreeMap<String, usize> = BTreeMap::new();

    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        let nested = object.get("value").and_then(Value::as_object);
        // Captured Food responses put meals under value.samples[]. Treat each
        // sample as a meal; the enclosing item is only a day bucket. Reading
        // both would double-count if the bucket later gains aggregate fields.
        let mut entries = Vec::new();
        if let Some((value, samples)) = nested.and_then(|value| {
            value
                .get("samples")
                .and_then(Value::as_array)
                .filter(|samples| !samples.is_empty())
                .map(|samples| (value, samples))
        }) {
            for sample in samples {
                let Some(meal) = sample.as_object() else {
                    continue;
                };
                if let Some(date) = food_sample_date(meal, object, value) {
                    entries.push((meal, None, date, true));
                } else {
                    out.diagnostics.push("food: a meal record has no usable date".into());
                }
            }
        } else if let Some(date) = summary_date(object, nested) {
            // Keep the earlier direct-item and value-level formats.
            entries.push((object, nested, date, false));
        } else {
            out.diagnostics.push("food: a record has no usable date".into());
        }

        for (meal, value_object, date, is_sample) in entries {
            let text = |key: &str| {
                first_value_from(meal, value_object, &[key]).and_then(|value| match value {
                    Value::String(text) if !text.trim().is_empty() => Some(text.clone()),
                    Value::Number(number) => Some(number.to_string()),
                    _ => None,
                })
            };
            let mut detail = FoodEntry {
                date: date.clone(),
                food_log_id: text("foodLogId"),
                food_name: text("foodName"),
                food_text: text("foodText"),
                meal_type: text("mealType"),
                mealtime: first_value_from(meal, value_object, &["mealtime", "timestamp", "time"])
                    .and_then(parse_timestamp)
                    .map(|time| time.to_rfc3339()),
                measure_weight: first_number_from(meal, value_object, &["measureWeight"])
                    .filter(|weight| weight.is_finite() && *weight >= 0.0),
                nutrients: BTreeMap::new(),
            };
            let mut matched: Vec<&str> = Vec::new();
            for Macro {
                metric,
                names,
                unit,
                range,
            } in MACROS
            {
                for name in names {
                    if meal.contains_key(*name)
                        || value_object
                            .is_some_and(|inner: &Map<String, Value>| inner.contains_key(*name))
                    {
                        matched.push(name);
                    }
                }
                let Some(value) = first_number_from(meal, value_object, names) else {
                    continue;
                };
                if !value.is_finite() || value < range.0 || value > range.1 {
                    out.diagnostics.push(format!(
                        "food: reading {value} for {metric} is not in {range:?}; ignored"
                    ));
                    continue;
                }
                detail.nutrients.insert(metric.to_string(), value);
                let entry = per_day.entry((date.clone(), metric)).or_insert((0.0, unit));
                entry.0 += value;
            }
            if let Some(fiber) = first_number_from(meal, value_object, &["fiber"])
                .filter(|fiber| fiber.is_finite() && (0.0..=1000.0).contains(fiber))
            {
                detail.nutrients.insert("fiber_g".into(), fiber);
            }
            // A day aggregate alone must not be presented as an individual meal.
            if is_sample
                || detail.food_name.is_some()
                || detail.food_text.is_some()
                || detail.food_log_id.is_some()
            {
                out.food_entries.push(detail);
            }
            let ignored = if is_sample {
                &FOOD_SAMPLE_FIELDS_NOT_MACROS[..]
            } else {
                &FOOD_FIELDS_NOT_MACROS[..]
            };
            for key in meal.keys() {
                if matched.contains(&key.as_str()) || ignored.contains(&key.as_str()) {
                    continue;
                }
                *unknown.entry(key.clone()).or_default() += 1;
            }
        }
    }

    for ((date, metric), (value, unit)) in per_day {
        out.daily_metrics.push(DailyMetric {
            date,
            metric: metric.to_string(),
            value,
            unit: unit.to_string(),
            source_scope: SourceScope::UserFused,
            device_id: None,
        });
    }
    if !unknown.is_empty() {
        let listed = unknown
            .iter()
            .map(|(name, count)| format!("{name}x{count}"))
            .collect::<Vec<_>>()
            .join(", ");
        out.diagnostics
            .push(format!("food: fields not parsed yet: {listed}"));
    }
}

pub(super) fn food_sample_date(
    sample: &Map<String, Value>,
    bucket: &Map<String, Value>,
    value: &Map<String, Value>,
) -> Option<String> {
    let date_keys = ["date", "day", "dayId", "dateString", "localDate"];
    if let Some(date) = first_value(sample, &date_keys).and_then(parse_date) {
        return Some(date);
    }
    if let Some(mealtime) = sample.get("mealtime").and_then(parse_timestamp) {
        let zone_keys = ["timeZone", "time_zone", "tz"];
        let zone = first_value_from(sample, Some(value), &zone_keys)
            .or_else(|| first_value(bucket, &zone_keys));
        if let Some(zone) = zone {
            if let Some(name) = zone
                .as_str()
                .map(|text| text.rsplit(',').next().unwrap_or(text).trim())
            {
                if let (Ok(zone), Ok(timestamp)) = (
                    jiff::tz::TimeZone::get(name),
                    jiff::Timestamp::from_second(mealtime.timestamp()),
                ) {
                    return Some(timestamp.to_zoned(zone).date().to_string());
                }
            }
            if let Some(offset) = timezone_offset_seconds(zone) {
                if let Some(local) = mealtime.checked_add_signed(Duration::seconds(offset)) {
                    return Some(local.format("%Y-%m-%d").to_string());
                }
            }
        }
    }
    summary_date(bucket, Some(value))
}

/// Fields in a food record that belong to "the record itself", not to a nutrition reading.
///
/// They are excluded so that the "fields not parsed yet" diagnostic above lists only truly new things.
/// After the first user with food records syncs, that line is our only lead.
pub(super) const FOOD_FIELDS_NOT_MACROS: [&str; 10] = [
    "date",
    "dateString",
    "day",
    "eventType",
    "subType",
    "time",
    "timestamp",
    "timeZone",
    "userId",
    "value",
];

pub(super) const FOOD_SAMPLE_FIELDS_NOT_MACROS: [&str; 13] = [
    "foodLogId",
    "foodName",
    "fiber",
    "measureWeight",
    "mealType",
    "mealtime",
    "emoji",
    "labels",
    "foodText",
    "timeZone",
    "date",
    "dateString",
    "timestamp",
];
