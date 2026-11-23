use super::*;

#[test]
fn food_day_buckets_sum_meal_samples_on_their_local_days() {
    // Shape and field names come from zepp-food-extractor's captured Food
    // response. Values here are synthetic. Berlin changes to UTC+2 on
    // 2026-03-29, so the second meal falls on the next local day.
    let early = DateTime::parse_from_rfc3339("2026-03-29T21:30:00Z")
        .unwrap()
        .timestamp_millis();
    let late = DateTime::parse_from_rfc3339("2026-03-29T22:30:00Z")
        .unwrap()
        .timestamp_millis();
    let raw = json!({"code": 0, "items": [{
        "timestamp": 1774742400000_i64,
        "value": {
            "timeZone": "1,Europe/Berlin",
            "energy": 9999,
            "samples": [
                {"mealtime": early, "energy": 95, "carbohydrates": 25,
                 "protein": 1, "fatTotal": 2, "foodLogId": "one"},
                {"mealtime": late, "energy": "50", "carbohydrates": 10,
                 "protein": 3, "fatTotal": 4, "foodLogId": "two"}
            ]
        }
    }]});
    let batch = Normalizer::normalize_wellness("wellness:food:v2_events", &raw);
    let rows: BTreeMap<_, _> = batch
        .daily_metrics
        .iter()
        .map(|row| ((row.date.as_str(), row.metric.as_str()), row.value))
        .collect();
    assert_eq!(rows.len(), 8);
    assert_eq!(rows[&("2026-03-29", "intake_calories")], 95.0);
    assert_eq!(rows[&("2026-03-30", "intake_calories")], 50.0);
    assert_eq!(rows[&("2026-03-29", "intake_carbs_g")], 25.0);
    assert_eq!(rows[&("2026-03-30", "intake_fat_g")], 4.0);
    assert!(batch.metric_samples.is_empty());
}

#[test]
fn food_accepts_probe_envelopes_and_nested_dates_without_inventing_macros() {
    // Synthetic fixtures: the reporter has not supplied a Food response.
    let items = json!([
        {"date": "2026-09-18", "calories": 500, "protein": 20},
        {"value": {"date": "2026-09-18", "calories": "300", "fat": 0}},
        {"date": "2026-09-19", "value": {"carbs": 40}},
        {"date": "2026-09-19", "unknownNutrient": 123},
        {"date": "2026-09-19", "calories": -1, "protein": 99999}
    ]);
    for raw in [
        json!({"items": items}),
        json!({"data": items}),
        json!({"code": 200, "data": {"items": items}}),
    ] {
        let batch = Normalizer::normalize_wellness("wellness:food:v2_events", &raw);
        let rows: BTreeMap<_, _> = batch
            .daily_metrics
            .iter()
            .map(|row| {
                (
                    (row.date.as_str(), row.metric.as_str()),
                    (row.value, row.unit.as_str()),
                )
            })
            .collect();
        assert_eq!(
            rows,
            BTreeMap::from([
                (("2026-09-18", "intake_calories"), (800.0, "kcal")),
                (("2026-09-18", "intake_protein_g"), (20.0, "g")),
                (("2026-09-18", "intake_fat_g"), (0.0, "g")),
                (("2026-09-19", "intake_carbs_g"), (40.0, "g")),
            ])
        );
        assert!(batch.metric_samples.is_empty());
    }
}

#[test]
fn food_epoch_strings_respect_local_day_and_explicit_dates() {
    let epoch = DateTime::parse_from_rfc3339("2026-09-18T23:30:00Z").unwrap();
    for time in [
        json!(epoch.timestamp()),
        json!(epoch.timestamp().to_string()),
        json!(epoch.timestamp_millis()),
        json!(epoch.timestamp_millis().to_string()),
    ] {
        let batch = Normalizer::normalize_wellness(
            "wellness:food:v2_events",
            &json!({
                "items": [{"value": {"time": time, "timeZone": "+02:00", "calories": 100}}]
            }),
        );
        assert_eq!(batch.daily_metrics.len(), 1);
        assert_eq!(batch.daily_metrics[0].date, "2026-09-19");
    }
    assert_eq!(
        parse_date(&json!("20260918")).as_deref(),
        Some("2026-09-18")
    );
    assert_eq!(parse_date(&json!("not-a-date")), None);
    let batch = Normalizer::normalize_wellness(
        "wellness:food:v2_events",
        &json!({
            "items": [{"date": "2026-09-17", "time": epoch.timestamp_millis(),
                       "timeZone": "+02:00", "calories": 100}]
        }),
    );
    assert_eq!(batch.daily_metrics[0].date, "2026-09-17");
}

#[test]
fn food_unknown_or_encoded_payloads_remain_unparsed() {
    for raw in [
        json!({"data": "encoded"}),
        json!({"items": []}),
        json!({"items": [{"date": "2026-09-18", "unknownNutrient": 123}]}),
        json!({"items": [{"value": "encoded"}]}),
        json!({"items": [{"calories": 100}]}),
    ] {
        let batch = Normalizer::normalize_wellness("wellness:food:v2_events", &raw);
        assert!(batch.daily_metrics.is_empty());
        assert!(batch.metric_samples.is_empty());
    }
}

/// The shape of one real response (fetched from a live account on 2026-09-04 and redacted; readings were changed).
///
/// The three records differ on purpose: on real accounts the `summary` key set varies, and
/// `timeZone` on three records of the same account is an IANA name, `GMT+08:00` and a
/// millisecond offset. Parsing must rely on `generatedTime` and not touch this time zone mess.
fn weight_payload() -> Value {
    serde_json::json!({
        "items": [
            {
                "generatedTime": 1764743530_i64,
                "createTime": 1764743529_i64,
                "weightType": 1,
                "memberId": "-1",
                "deviceSource": -1,
                "appName": "com.xiaomi.hm.health",
                "userId": "1",
                "summary": {
                    "weight": 68.2, "bmi": 22.1, "height": 175.0,
                    "age": 30, "bodyStyle": 0, "dataSourceType": 1,
                    "deviceSn": "", "deviceType": 1, "source": 1,
                    "syncHealthConnect": false, "timeZone": "Asia/Shanghai"
                }
            },
            {
                "generatedTime": 1738465258_i64,
                "createTime": 1738465258_i64,
                "weightType": 1,
                "memberId": "-1",
                "summary": {
                    "weight": 69.0, "bmi": 22.5, "height": 175.0,
                    "bodyBalanceScore": 55, "oneFootMeasureTime": 55.0,
                    "encryptImpedance": "x", "deviceType": 1, "source": 1,
                    "syncHealth": 1, "syncHealthConnect": false
                }
            },
            {
                "generatedTime": 1722772344_i64,
                "createTime": 1722772344_i64,
                "weightType": 0,
                "memberId": "-1",
                "summary": {
                    "weight": 70.4, "bmi": 23.0, "height": 175.0,
                    "timeZone": "28800000", "source": -1, "deviceType": 1
                }
            }
        ]
    })
}

fn samples_named<'a>(batch: &'a WellnessNormalizedData, metric: &str) -> Vec<&'a MetricSample> {
    batch
        .metric_samples
        .iter()
        .filter(|sample| sample.metric == metric)
        .collect()
}

/// This case is the reason for the whole feature: before, it fetched no records, because it asked the wrong endpoint.
#[test]
fn a_weigh_in_becomes_timestamped_samples_not_daily_rows() {
    let batch = Normalizer::normalize_weight(&weight_payload());

    let weights = samples_named(&batch, "weight");
    assert_eq!(weights.len(), 3, "three records should give three weight samples");
    // One day can have several weigh-ins, so these are metric_samples, not daily_metrics.
    assert!(batch.daily_metrics.is_empty(), "weight must not be squashed to one number per day");
    assert_eq!(weights[0].unit, "kg");

    // generatedTime is Unix seconds. Read as milliseconds it lands 50,000 years ahead, and on the chart that error
    // shows only as "no data at all", which is another form of the symptom this fixes.
    assert_eq!(
        weights[0].timestamp.format("%Y-%m-%d").to_string(),
        "2025-12-03",
        "timestamps are read as seconds"
    );
    assert!(
        weights.iter().all(|sample| {
            let year: i32 = sample.timestamp.format("%Y").to_string().parse().unwrap();
            (2024..=2026).contains(&year)
        }),
        "none lands in an absurd year"
    );
}

/// The `summary` key set differs per record. Missing fields are normal, not errors.
#[test]
fn a_summary_missing_fields_yields_fewer_samples_not_an_error() {
    let batch = Normalizer::normalize_weight(&weight_payload());
    assert_eq!(samples_named(&batch, "bmi").len(), 3);
    assert_eq!(samples_named(&batch, "height").len(), 3);
    // Only the second record has bodyBalanceScore.
    assert_eq!(samples_named(&batch, "body_balance_score").len(), 1);
    // This account has no scale, so no body composition field may be invented.
    for metric in ["body_fat_rate", "muscle_mass", "bone_mass", "bmr"] {
        assert!(
            samples_named(&batch, metric).is_empty(),
            "{metric} must not appear without this field"
        );
    }
}

/// When the name matches but the number does not, it must not count as that metric.
///
/// The body composition field names come from the ecosystem, not from responses we have seen. When a name matches
/// but the meaning differs, drawing it on the body fat curve is worse than showing nothing: the user will believe it.
#[test]
fn a_reading_outside_the_plausible_range_is_dropped_and_reported() {
    let payload = serde_json::json!({
        "items": [{
            "generatedTime": 1764743530_i64,
            "summary": { "weight": 68.2, "fatRate": 1980.0 }
        }]
    });
    let batch = Normalizer::normalize_weight(&payload);
    assert_eq!(samples_named(&batch, "weight").len(), 1, "weight is kept as usual");
    assert!(
        samples_named(&batch, "body_fat_rate").is_empty(),
        "1980 cannot be a body fat rate"
    );
    assert!(
        batch
            .diagnostics
            .iter()
            .any(|line| line.contains("body_fat_rate")),
        "a dropped value must be reported, or nobody knows the name means something else: {:?}",
        batch.diagnostics
    );
}

/// Once a user with a scale syncs, their real field names appear in the diagnostics.
///
/// Without a scale, this is our only honest way to complete the list: do not guess names; let the data report them.
#[test]
fn unrecognised_summary_fields_are_reported_so_they_can_be_named_later() {
    let payload = serde_json::json!({
        "items": [{
            "generatedTime": 1764743530_i64,
            "summary": { "weight": 68.2, "someUnknownScaleField": 12.5 }
        }]
    });
    let batch = Normalizer::normalize_weight(&payload);
    let joined = batch.diagnostics.join(" ");
    assert!(
        joined.contains("someUnknownScaleField"),
        "unseen fields must be reported: {joined}"
    );
    // Known context fields must not mix into this diagnostic, or real new names get lost.
    assert!(!joined.contains("timeZone"));
}

/// An empty response is a fact, not a fault: an account with no scale and no manual weight looks like this.
#[test]
fn an_empty_page_is_not_an_error() {
    let batch = Normalizer::normalize_weight(&serde_json::json!({ "items": [] }));
    assert!(batch.metric_samples.is_empty());
    assert!(batch.diagnostics.is_empty(), "empty needs no explanation");
}

/// A 2026-11 report: the user made family scale profiles for himself and his son, and after import one weight line showed
/// 75 kg one day and 23 kg the next. Family records carry their own positive memberId, and none may
/// enter the owner's timeline. Records without memberId (official channel) are the owner as usual.
#[test]
fn family_members_on_a_shared_scale_never_enter_the_holders_timeline() {
    let payload = serde_json::json!({
        "items": [
            { "generatedTime": 1764743530_i64, "memberId": "-1",
              "summary": { "weight": 75.0 } },
            { "generatedTime": 1764829930_i64, "memberId": "1234567",
              "summary": { "weight": 23.0, "bmi": 15.0 } },
            { "generatedTime": 1764916330_i64, "memberId": 7654321_i64,
              "summary": { "weight": 31.0 } },
            { "generatedTime": 1765002730_i64, "memberId": -1_i64,
              "summary": { "weight": 74.6 } },
            { "generatedTime": 1765089130_i64,
              "summary": { "weight": 74.9 } }
        ]
    });
    let batch = Normalizer::normalize_weight(&payload);
    let weights: Vec<f64> = samples_named(&batch, "weight")
        .iter()
        .map(|sample| sample.value)
        .collect();
    assert_eq!(weights, vec![75.0, 74.6, 74.9], "only the owner's three weigh-ins remain");
    assert!(
        samples_named(&batch, "bmi").is_empty(),
        "family body composition must not mix in either"
    );
    let joined = batch.diagnostics.join(" ");
    assert!(
        joined.contains("skipped 2 family member"),
        "skipped records must be reported: {joined}"
    );
    assert!(!joined.contains("23"), "diagnostics must not carry family readings: {joined}");
}

/// Export selects by type and the database stores by metric name; both lists must match.
#[test]
fn every_body_metric_is_exportable() {
    for spec in &BODY_METRICS {
        assert!(
            crate::storage::BODY_COMPOSITION_METRICS.contains(&spec.metric),
            "{} can be stored but not exported",
            spec.metric
        );
    }
    assert_eq!(
        BODY_METRICS.len(),
        crate::storage::BODY_COMPOSITION_METRICS.len()
    );
}

#[test]
fn a_weight_far_in_the_future_is_skipped() {
    let millis_as_seconds = serde_json::json!({
        "items": [{
            "generatedTime": 1_764_743_530_000i64,
            "summary": { "weight": 68.2 }
        }]
    });
    let batch = Normalizer::normalize_weight(&millis_as_seconds);
    assert!(
        samples_named(&batch, "weight").is_empty(),
        "milliseconds read as seconds land 50,000 years ahead and must be dropped"
    );
    assert!(
        batch.diagnostics.iter().any(|line| line.contains("timestamp")),
        "a dropped value must be reported: {:?}",
        batch.diagnostics
    );

    let two_days_out = (Utc::now() + Duration::days(2)).timestamp();
    let near_future = serde_json::json!({
        "items": [{
            "generatedTime": two_days_out,
            "summary": { "weight": 68.2 }
        }]
    });
    let batch = Normalizer::normalize_weight(&near_future);
    assert!(samples_named(&batch, "weight").is_empty());
}
