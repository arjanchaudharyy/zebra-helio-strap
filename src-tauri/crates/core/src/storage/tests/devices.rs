use super::*;

/// A cloud business error code must reach the diagnostic report.
///
/// Every link in this chain was already there except the last step: `classify_business_code` can detect
/// "HTTP 200 but the cloud says it failed", and provenance puts it in its own class, but that
/// code went only into a human-readable Chinese sentence, and the diagnostic report had nothing.
#[test]
fn a_cloud_rejection_code_reaches_the_diagnostic_report() {
    use crate::storage::provenance::{Stage, StageErrorKind, StageOutcome};

    let dir = std::env::temp_dir().join("zeppbridge-cloud-rejection-code");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = Database::open_migrated(&dir.join("zepp.db")).unwrap();

    assert_eq!(db.diagnostic_cloud_rejection().unwrap(), None);

    // First a plain failure with no code: it must not be reported as a business rejection.
    db.record_stream_stage(
        "sleep",
        Stage::Fetch,
        &StageOutcome::Failed {
            kind: StageErrorKind::Network,
            message: Some("connection timed out".into()),
        },
    )
    .unwrap();
    assert_eq!(db.diagnostic_cloud_rejection().unwrap(), None);

    db.record_stream_stage(
        "workouts",
        Stage::Fetch,
        &StageOutcome::Failed {
            kind: StageErrorKind::CloudRejected { code: -1 },
            message: Some("The Zepp cloud rejected this request (code -1)".into()),
        },
    )
    .unwrap();

    let rejection = db
        .diagnostic_cloud_rejection()
        .unwrap()
        .expect("a business rejection must be readable");
    assert_eq!(rejection.stream, "workouts");
    assert_eq!(rejection.code, -1);
    assert!(rejection.at.is_some());

    // The same stream succeeded later, so the old code must not bother the report reader.
    db.record_stream_stage("workouts", Stage::Fetch, &StageOutcome::Ok)
        .unwrap();
    assert_eq!(db.diagnostic_cloud_rejection().unwrap(), None);

    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A firmware version is not a device identifier.
///
/// Shapes from real data (from the local `device_identities` table):
///   device ids -> `D8803CFFFEC19AC6` (hex MAC), `23229501001311`
///                 (numeric serial), `PRUC72 070007001c` (product code)
///   firmware   -> `0.116.137.19`, `0.132.139.2`, `V0.54.131.3`
#[test]
fn firmware_versions_are_not_mistaken_for_devices() {
    // The three ghost "devices" the user reported.
    assert!(looks_like_firmware_version("0.91.20.5"));
    assert!(looks_like_firmware_version("0.91.17.5"));
    // Firmware strings that really exist in a local library.
    assert!(looks_like_firmware_version("0.116.137.19"));
    assert!(looks_like_firmware_version("0.132.139.2"));
    assert!(looks_like_firmware_version("V0.54.131.3"));

    // No real device id may be caught by mistake.
    assert!(!looks_like_firmware_version("D8803CFFFEC19AC6"));
    assert!(!looks_like_firmware_version("23229501001311"));
    assert!(!looks_like_firmware_version("PRUC72 070007001c"));
    assert!(!looks_like_firmware_version("2445B138005129"));
    assert!(!looks_like_firmware_version("F75C87FFFE3A9B28"));

    // Two parts do not count: the rule needs at least three, so dotted serials are not caught.
    assert!(!looks_like_firmware_version("1.2"));
    assert!(!looks_like_firmware_version(""));
    assert!(!looks_like_firmware_version("1..2.3"));
    assert!(!looks_like_firmware_version("1.2.3a"));
}

/// A ghost device must not be recorded as a device.
#[test]
fn a_firmware_shaped_device_id_produces_no_identity() {
    let payload = serde_json::json!({
        "items": [
            { "deviceId": "0.91.20.5", "displayName": "Bip 6" },
            { "deviceId": "D8803CFFFEC19AC6", "sn": "23229501001311" }
        ]
    });
    let hints = device_identity_hints(&payload);
    let aliases: Vec<String> = hints.iter().flat_map(|h| h.aliases.clone()).collect();

    assert!(
        !aliases.iter().any(|a| a == "0.91.20.5"),
        "a firmware version must not become a device alias, got: {aliases:?}"
    );
    assert!(aliases.iter().any(|a| a == "D8803CFFFEC19AC6"));
    assert!(aliases.iter().any(|a| a == "23229501001311"));
}

/// When a firmware-shaped `sn` appears next to a real device, the device itself must stay,
/// but the fake `serial` must not be recorded.
#[test]
fn a_firmware_shaped_serial_does_not_poison_a_real_device() {
    let payload = serde_json::json!({
        "items": [{ "deviceId": "D8803CFFFEC19AC6", "sn": "0.91.20.5" }]
    });
    let hints = device_identity_hints(&payload);

    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].device_id.as_deref(), Some("D8803CFFFEC19AC6"));
    assert_eq!(
        hints[0].serial, None,
        "a firmware-shaped sn must not go into serial: upsert would also write it as an alias"
    );
    assert!(!hints[0].aliases.iter().any(|a| a == "0.91.20.5"));
}

#[test]
fn a_stream_the_cloud_has_but_we_never_read_is_not_reported_as_available_data() {
    // Weight and blood pressure are only probed, not normalized. The probe says "the cloud has 42 records", and this machine has none.
    // If the two are mixed, the capability page makes people think ZeppBridge already stores their blood pressure.
    let db = Database::in_memory().unwrap();
    db.save_capability_probe(&[CapabilityProbe {
        stream: "blood_pressure".into(),
        surface: "v2_events".into(),
        cadence: "episodic".into(),
        window_days: 365,
        event_type: "blood_pressure".into(),
        sub_type: "real_data".into(),
        status: "available".into(),
        records: 42,
        latest_date: Some("2026-08-01".into()),
        fields: Vec::new(),
        failure: None,
    }])
    .unwrap();

    let overview = db.capability_overview(Local::now().date_naive()).unwrap();
    let row = overview
        .items
        .iter()
        .find(|item| item.stream == "blood_pressure")
        .expect("blood pressure must appear in the capability overview");
    assert_eq!(row.status, "available", "the cloud really has it, so say so");
    assert!(!row.ingested, "but this machine has not stored it, so it must not count as \"ready\"");
    assert!(
        row.note.is_some(),
        "must explain why it is not stored, and not let the user guess"
    );

    // A stream that is really stored still counts as ingested, or the flag has no meaning.
    assert!(overview
        .items
        .iter()
        .filter(|item| item.source == "derived")
        .all(|item| item.ingested));
}

#[test]
fn food_probe_is_cloud_evidence_until_sync_or_replay_imports_it() {
    let db = Database::in_memory().unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
    let food = || {
        db.capability_overview(today)
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.stream == "food")
            .unwrap()
    };
    assert_eq!(food().status, "no_records");
    let mut probe = CapabilityProbe {
        stream: "food".into(),
        surface: "v2_events".into(),
        cadence: "episodic".into(),
        window_days: 365,
        event_type: "Food".into(),
        sub_type: String::new(),
        status: "available".into(),
        records: 50,
        latest_date: Some("2026-09-18".into()),
        fields: Vec::new(),
        failure: None,
    };
    db.save_capability_probe(&[probe.clone()]).unwrap();
    let row = food();
    assert_eq!(row.status, "available");
    assert_eq!(row.source, "probed");
    assert!(!row.ingested);
    assert_eq!(row.records, 50);
    assert_eq!(row.records_unit_code, "records");

    // Stale, empty, and failed probes must not claim current cloud data.
    probe.latest_date = Some("2024-01-01".into());
    db.save_capability_probe(&[probe.clone()]).unwrap();
    assert_eq!(food().status, "no_records");
    probe.latest_date = Some("2026-09-18".into());
    for status in ["empty", "unknown", "unavailable"] {
        probe.status = status.into();
        db.save_capability_probe(&[probe.clone()]).unwrap();
        assert_eq!(food().status, "no_records");
    }
    probe.status = "available".into();
    db.save_capability_probe(&[probe]).unwrap();
    let raw = RawRecord {
        stream: "wellness".into(),
        source_key: "wellness:food:v2_events:2026-09-14:2026-09-20".into(),
        source_scope: SourceScope::UserFused,
        device_id: None,
        start_utc: today.and_hms_opt(0, 0, 0).unwrap().and_utc() - Duration::days(6),
        end_utc: Some(today.and_hms_opt(0, 0, 0).unwrap().and_utc()),
        payload: serde_json::json!({"data":{"items":[{
            "timestamp": 1789689600000_i64,
            "value": {"timeZone":"UTC","samples":[
                {"mealtime":1789718400000_i64,"energy":500,"protein":20},
                {"mealtime":1789732800000_i64,"energy":300,"fatTotal":10}
            ]}
        }]}}),
        capability: CapabilityStatus::Unverified,
    };
    // A v29 database retained this day-bucket response but could not read
    // value.samples[]. The v30 replay should recover it without a fetch.
    db.insert_raw_record(&raw).unwrap();
    db.set_app_meta(
        "normalizer_revision",
        "zepp-normalizer-2026-09-v29-food-envelopes",
    )
    .unwrap();
    db.set_app_meta(LAST_CLOUD_SYNC_AT_KEY, "2026-09-19T12:00:00Z")
        .unwrap();
    assert!(db.pending_replay_plan().unwrap().is_some());
    db.reprocess_raw_records_if_needed().unwrap().unwrap();
    assert!(db.reprocess_raw_records_if_needed().unwrap().is_none());
    // The live sync/backfill path must also be idempotent after replay.
    db.persist_fetched_record(&raw).unwrap();
    let row = food();
    assert_eq!(row.source, "derived");
    assert_eq!(row.status, "available");
    assert!(row.ingested);
    assert_eq!(row.records, 1);
    assert_eq!(row.records_unit_code, "days");
    assert_eq!(row.latest_date.as_deref(), Some("2026-09-18"));
    let (count, calories): (i64, f64) = db
        .conn
        .query_row(
            "SELECT COUNT(*), SUM(value) FROM daily_metrics WHERE metric = 'intake_calories'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((count, calories), (1, 800.0));
    assert_eq!(db.raw_record_count().unwrap(), 1);
    assert_eq!(
        db.get_app_meta(LAST_CLOUD_SYNC_AT_KEY).unwrap().as_deref(),
        Some("2026-09-19T12:00:00Z")
    );
}

#[test]
fn partial_capability_refresh_keeps_food_evidence_and_failures_do_not_erase_it() {
    let db = Database::in_memory().unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 9, 22).unwrap();
    let food = CapabilityProbe {
        stream: "food".into(),
        surface: "v2_events".into(),
        cadence: "episodic".into(),
        window_days: 365,
        event_type: "Food".into(),
        sub_type: String::new(),
        status: "available".into(),
        records: 12,
        latest_date: Some("2026-09-02".into()),
        fields: vec!["value.foodName".into()],
        failure: None,
    };
    db.save_capability_probe(std::slice::from_ref(&food))
        .unwrap();
    let saved: Vec<CapabilityProbe> = serde_json::from_str(
        &db.get_app_meta(CAPABILITY_PROBE_RESULT_KEY)
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert!(saved[0].fields.is_empty());
    let mut blood_pressure = food.clone();
    blood_pressure.stream = "blood_pressure".into();
    blood_pressure.event_type = "blood_pressure".into();
    blood_pressure.status = "empty".into();
    blood_pressure.records = 0;
    blood_pressure.latest_date = None;
    db.save_capability_probe(&[blood_pressure]).unwrap();

    let food_row = || {
        db.capability_overview(today)
            .unwrap()
            .items
            .into_iter()
            .find(|item| item.stream == "food")
            .unwrap()
    };
    assert_eq!(food_row().status, "available");
    assert_eq!(food_row().records, 12);
    assert_eq!(food_row().source, "probed");
    assert!(!food_row().ingested);

    let mut failed_food = food;
    failed_food.status = "error".into();
    failed_food.records = 0;
    failed_food.latest_date = None;
    db.save_capability_probe(&[failed_food]).unwrap();
    assert_eq!(food_row().records, 12);
}

#[test]
fn a_fetched_but_unparsed_stream_is_not_reported_as_empty() {
    // "empty_in_range" claims the stream is wired and the account has no
    // data. For a stream whose raw responses are on disk but whose field
    // mapping is not verified yet, that is false in a way that would send
    // a reader looking for a device problem that does not exist.
    let db = Database::in_memory().unwrap();
    db.insert_raw_record(&RawRecord {
        stream: "wellness".into(),
        source_key: "wellness:spo2:user_events:2023-11-01:2023-11-08".into(),
        source_scope: SourceScope::UserFused,
        device_id: None,
        start_utc: ts(),
        end_utc: Some(ts() + chrono::Duration::days(7)),
        payload: serde_json::json!({ "items": [] }),
        capability: CapabilityStatus::Unverified,
    })
    .unwrap();

    let export = parsed_export(&db, &["spo2", "sleep"], ExportDetail::Summary);
    let capabilities = &export["capabilities"];
    assert_eq!(capabilities["spo2"]["status"], "raw_pending");
    assert_eq!(capabilities["spo2"]["raw_records"], 1);
    // A stream with no raw responses at all still reports plain emptiness.
    assert_eq!(capabilities["sleep"]["status"], "empty_in_range");
}

#[test]
fn one_physical_device_gets_one_label() {
    // Zepp stores an identity row per alias. The strap's rows share a
    // serial but differ in device_id, and keying a group on both reported
    // one device as two.
    let db = Database::in_memory().unwrap();
    for (alias, device_id) in [
        ("2445B138005129", "2445B138005129"),
        ("D85403FFFEE4D576", "D85403FFFEE4D576"),
    ] {
        db.conn
            .execute(
                "INSERT INTO device_identities
                        (alias, name, firmware, serial, device_id, timezone, updated_at)
                     VALUES (?1, ?2, NULL, ?3, ?4, NULL, ?5)",
                params![
                    alias,
                    // A Chinese nickname prefix glued to the product name (escaped on purpose).
                    "\u{51cc}\u{82cd}\u{7684}Helio Strap",
                    "2445B138005129",
                    device_id,
                    Utc::now().to_rfc3339()
                ],
            )
            .unwrap();
    }
    db.insert_metric_sample(&MetricSample {
        metric: "hrv".into(),
        timestamp: ts(),
        value: 45.0,
        unit: "ms".into(),
        source_scope: SourceScope::Device,
        device_id: Some("D85403FFFEE4D576".into()),
    })
    .unwrap();

    let export = parsed_export(&db, &["hrv"], ExportDetail::Summary);
    let devices = export["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 1, "one strap must not appear twice");
    assert_eq!(devices[0]["label"], "device_1");
    assert_eq!(devices[0]["model"], "Amazfit Helio Strap");
    assert_eq!(devices[0]["kind"], "strap");
    // Neither the serial nor the user's nickname may leave the machine.
    let encoded = serde_json::to_string(&export).unwrap();
    assert!(!encoded.contains("2445B138005129"));
    assert!(!encoded.contains("\u{51cc}\u{82cd}"));
    assert_eq!(
        export["data"]["metric_samples"][0]["device_label"],
        "device_1"
    );
}

#[test]
fn capability_overview_never_calls_missing_data_unsupported() {
    // This API answers "200 with no items" for event names that cannot
    // exist, so an absence never proves a device lacks a sensor. Saying
    // "your watch does not support blood pressure" to someone who simply
    // has not measured would send them shopping for hardware they own.
    let db = Database::in_memory().unwrap();
    let today = Local::now().date_naive();
    let overview = db.capability_overview(today).unwrap();
    let by_stream: std::collections::BTreeMap<_, _> = overview
        .items
        .iter()
        .map(|item| (item.stream.as_str(), item))
        .collect();

    // Nothing synced yet: everything is absent, and nothing is condemned.
    assert!(overview
        .items
        .iter()
        .all(|item| item.status != "unsupported"));
    assert_eq!(by_stream["heart_rate"].status, "no_records");
    // A stream that needs a request and has never been checked says so.
    assert_eq!(by_stream["blood_pressure"].status, "unknown");
    assert_eq!(by_stream["blood_pressure"].source, "probed");

    db.insert_metric_sample(&MetricSample {
        metric: "heart_rate".into(),
        timestamp: Local
            .with_ymd_and_hms(today.year(), today.month(), today.day(), 12, 0, 0)
            .unwrap()
            .with_timezone(&Utc),
        value: 60.0,
        unit: "bpm".into(),
        source_scope: SourceScope::Device,
        device_id: None,
    })
    .unwrap();
    let overview = db.capability_overview(today).unwrap();
    let heart_rate = overview
        .items
        .iter()
        .find(|item| item.stream == "heart_rate")
        .unwrap();
    assert_eq!(heart_rate.status, "available");
    assert_eq!(heart_rate.records, 1);
    // Derived from stored rows, so it cost no request.
    assert_eq!(heart_rate.source, "derived");
}

#[test]
fn capability_window_is_inclusive_local_calendar_days() {
    let db = Database::in_memory().unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 8, 20).unwrap();
    // The heart_rate window is 30 days: today - 29 .. today, both ends included.
    let on_start_boundary = today - Duration::days(29);
    let just_before = today - Duration::days(30);
    for (day, value) in [(on_start_boundary, 60.0), (just_before, 61.0)] {
        db.insert_metric_sample(&MetricSample {
            metric: "heart_rate".into(),
            timestamp: Local
                .with_ymd_and_hms(day.year(), day.month(), day.day(), 12, 0, 0)
                .unwrap()
                .with_timezone(&Utc),
            value,
            unit: "bpm".into(),
            source_scope: SourceScope::Device,
            device_id: None,
        })
        .unwrap();
    }

    let overview = db.capability_overview(today).unwrap();
    let heart_rate = overview
        .items
        .iter()
        .find(|item| item.stream == "heart_rate")
        .unwrap();
    assert_eq!(
        heart_rate.records, 1,
        "the day on the window boundary counts, the day before it does not"
    );
    assert_eq!(heart_rate.latest_date.as_deref(), Some("2026-07-22"));
}

#[test]
fn candidate_rejection_does_not_establish_device_support() {
    let db = Database::in_memory().unwrap();
    let probe = |status: &str| CapabilityProbe {
        stream: "blood_pressure".into(),
        surface: "v2_events".into(),
        cadence: "episodic".into(),
        window_days: 365,
        event_type: "blood_pressure".into(),
        sub_type: "real_data".into(),
        status: status.into(),
        records: 0,
        latest_date: None,
        fields: Vec::new(),
        failure: None,
    };

    db.save_capability_probe(&[probe("empty")]).unwrap();
    let overview = db.capability_overview(Local::now().date_naive()).unwrap();
    let item = overview
        .items
        .iter()
        .find(|item| item.stream == "blood_pressure")
        .unwrap();
    assert_eq!(item.status, "no_records", "an empty answer proves nothing");

    db.save_capability_probe(&[probe("unavailable")]).unwrap();
    let overview = db.capability_overview(Local::now().date_naive()).unwrap();
    let item = overview
        .items
        .iter()
        .find(|item| item.stream == "blood_pressure")
        .unwrap();
    assert_eq!(
        item.status, "unknown",
        "one candidate cannot establish device support"
    );
}

#[test]
fn capability_candidates_are_saved_separately_and_positive_evidence_wins() {
    let db = Database::in_memory().unwrap();
    let mut positive = CapabilityProbe {
        stream: "blood_pressure".into(),
        surface: "blood_pressure_days".into(),
        cadence: "episodic".into(),
        window_days: 7,
        event_type: "bloodPressure".into(),
        sub_type: String::new(),
        status: "available".into(),
        records: 2,
        latest_date: Some("2026-10-11".into()),
        fields: vec!["systolic".into()],
        failure: None,
    };
    let mut empty = positive.clone();
    empty.surface = "v2_events".into();
    empty.status = "empty".into();
    empty.records = 0;
    for probes in [
        vec![positive.clone(), empty.clone()],
        vec![empty, positive.clone()],
    ] {
        db.save_capability_probe(&probes).unwrap();
        let item = db
            .capability_overview(Local::now().date_naive())
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.stream == "blood_pressure")
            .unwrap();
        assert_eq!(item.status, "available");
        assert_eq!(item.window_days, 7);
        assert_eq!(item.records, 2);
        assert!(!item.ingested);
    }
    positive.stream = "emotion".into();
    positive.status = "error".into();
    positive.records = 0;
    db.save_capability_probe(&[positive]).unwrap();
    let item = db
        .capability_overview(Local::now().date_naive())
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.stream == "emotion")
        .unwrap();
    assert_eq!(item.status, "unknown");
    assert!(!item.note.unwrap().contains("no measurements"));
}

#[test]
fn device_lookup_does_not_fall_back_to_first_device() {
    let db = Database::in_memory().unwrap();
    db.upsert_device_identity(&DeviceIdentityHint {
        aliases: vec!["SN-ONE".into(), "MAC-ONE".into()],
        name: Some("Watch One".into()),
        firmware: Some("1.0.0".into()),
        serial: Some("SN-ONE".into()),
        device_id: Some("MAC-ONE".into()),
        timezone: None,
    })
    .unwrap();
    db.upsert_device_identity(&DeviceIdentityHint {
        aliases: vec!["SN-TWO".into(), "MAC-TWO".into()],
        name: Some("Watch Two".into()),
        firmware: Some("2.0.0".into()),
        serial: Some("SN-TWO".into()),
        device_id: Some("MAC-TWO".into()),
        timezone: None,
    })
    .unwrap();
    let one = db.lookup_device_profile("SN-ONE").unwrap().unwrap();
    let two = db.lookup_device_profile("MAC-TWO").unwrap().unwrap();
    assert_eq!(one.name.as_deref(), Some("Watch One"));
    assert_eq!(two.name.as_deref(), Some("Watch Two"));
    assert!(db.lookup_device_profile("UNKNOWN").unwrap().is_none());
}

#[test]
fn device_data_summary_excludes_fused_records_and_keeps_identity_aliases() {
    let db = Database::in_memory().unwrap();
    let timestamp = ts();
    db.insert_metric_sample(&MetricSample {
        metric: "heart_rate".into(),
        timestamp,
        value: 72.0,
        unit: "bpm".into(),
        source_scope: SourceScope::Device,
        device_id: Some("SN-HELIO".into()),
    })
    .unwrap();
    db.insert_metric_sample(&MetricSample {
        metric: "heart_rate".into(),
        timestamp: timestamp + chrono::Duration::minutes(2),
        value: 80.0,
        unit: "bpm".into(),
        source_scope: SourceScope::UserFused,
        device_id: Some("SN-HELIO".into()),
    })
    .unwrap();
    let (has_data, latest) = db.device_data_summary(&["sn-helio".to_string()]).unwrap();
    assert!(has_data);
    assert_eq!(latest.as_deref(), Some("2023-11-14T22:13:20+00:00"));
    let (has_unknown, _) = db
        .device_data_summary(&["missing-device".to_string()])
        .unwrap();
    assert!(!has_unknown);
}

/// Hiding a device is only a local state row: hidden means not in the list, restore brings it back, and manual assignment is not affected.
#[test]
fn hiding_a_device_only_hides_it_and_restoring_brings_it_back() {
    use crate::storage::device_local_state::is_hidden_device;

    let db = Database::in_memory().unwrap();
    db.set_device_model_override("ABC123", Some("amazfit-balance-2"))
        .unwrap();

    db.set_device_hidden("ABC123", true).unwrap();
    let hidden = db.hidden_device_keys().unwrap();
    // Case-insensitive. A profile built from the serial must also be seen as the same device.
    assert!(is_hidden_device(&hidden, Some("abc123"), None));
    assert!(!is_hidden_device(&hidden, Some("OTHER"), Some("SN9")));
    // Hiding again with a different case does not leave two rows.
    db.set_device_hidden("abc123", true).unwrap();
    assert_eq!(db.hidden_device_keys().unwrap().len(), 1);

    // The assignment is still there.
    assert_eq!(
        db.device_model_override(&["ABC123"])
            .unwrap()
            .map(|value| value.catalog_id),
        Some("amazfit-balance-2".to_string())
    );

    db.set_device_hidden("ABC123", false).unwrap();
    assert!(db.hidden_device_keys().unwrap().is_empty());
    assert!(db.device_model_override(&["ABC123"]).unwrap().is_some());
    assert!(db.set_device_hidden("  ", true).is_err());
}
