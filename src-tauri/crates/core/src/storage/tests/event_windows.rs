//! Daily event payloads: stored per day, repeat fetches do not rewrite, old full-window payloads get compacted.

use super::*;
use serde_json::{json, Value};

fn day_ms(day: &str) -> i64 {
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis()
}

/// One day of Charge entries. `total` = 255 is the sentinel for a day when the watch computed no score.
fn charge(day: &str, total: i64) -> Value {
    json!({
        "eventType": "Charge", "subType": "real_data", "timestamp": day_ms(day), "userId": "1",
        "value": {"deviceId": "1440,app", "samples": [
            {"jsonExtra": "{}", "mental": 70.0, "physical": 40.0, "s": 0, "status": 0, "total": total}
        ]}
    })
}

fn raw(source_key: &str, payload: Value) -> RawRecord {
    RawRecord {
        stream: "daily_summary".into(),
        source_key: source_key.into(),
        source_scope: SourceScope::UserFused,
        device_id: None,
        start_utc: ts(),
        end_utc: Some(ts()),
        payload,
        capability: CapabilityStatus::Verified,
    }
}

fn charge_rows(db: &Database) -> Vec<(String, String, f64)> {
    let mut stmt = db
        .conn
        .prepare(
            "SELECT date, metric, value FROM daily_metrics
             WHERE metric LIKE '%charge' ORDER BY date, metric",
        )
        .unwrap();
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap()
}

fn keys_and_fetch_times(db: &Database) -> Vec<(String, String)> {
    let mut stmt = db
        .conn
        .prepare("SELECT source_key, fetched_at FROM raw_records ORDER BY source_key")
        .unwrap();
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap()
}

fn set_fetched_at(db: &Database, source_key: &str, at: &str) {
    db.conn
        .execute(
            "UPDATE raw_records SET fetched_at = ?2 WHERE source_key = ?1",
            params![source_key, at],
        )
        .unwrap();
}

/// Auto sync fetches the same day again every 15 minutes. When the content did not change, "wrote N rows" must not report 0, and
/// "last received from the cloud" must not stay at the first fetch. When the content changed, the derived rows must really change.
#[test]
fn refetching_a_day_replaces_it_in_place_and_an_identical_refetch_keeps_counts_and_freshness() {
    let db = Database::in_memory().unwrap();
    let key = "events:Charge:real_data:day:2026-09-26";
    let first = raw(key, json!({"items": [charge("2026-09-26", 60)]}));
    let (id, counts) = db.persist_fetched_record(&first).unwrap();
    assert_eq!(counts.primary_records, 3, "hybrid / physical / mental");
    set_fetched_at(&db, key, "2026-09-26T08:00:00+00:00");

    let (again_id, again) = db.persist_fetched_record(&first).unwrap();
    assert_eq!(again_id, id);
    assert_eq!(
        again.primary_records, 3,
        "a skipped rewrite must still report the rows this payload produces"
    );
    let fetched_at: String = db
        .conn
        .query_row(
            "SELECT fetched_at FROM raw_records WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        fetched_at.as_str() > "2026-09-26T08:00:00+00:00",
        "the cloud fetch time must refresh"
    );

    let changed = raw(key, json!({"items": [charge("2026-09-26", 72)]}));
    let epoch = content_epoch();
    let (changed_id, _) = db.persist_fetched_record(&changed).unwrap();
    // The UI uses it to decide whether to drop its read cache: if content changes and it does not move forward, every page keeps showing old values.
    // (Other test threads also advance it, so we only assert "changed means it advances", not "unchanged means it stays".)
    assert!(content_epoch() > epoch, "content changed, so the content epoch must advance");
    assert_eq!(changed_id, id, "same day overwrites, it does not insert a new row");
    assert_eq!(db.count_raw_records().unwrap(), 1);
    assert!(charge_rows(&db).contains(&("2026-09-26".into(), "hybrid_charge".into(), 72.0)));
}

/// The watch computed no score that day: we recognize it, it just has no value. Stored per day, it is its own row and must not be
/// treated as a parse failure and put in the quarantine table.
#[test]
fn a_scoreless_charge_day_is_kept_as_an_empty_day_not_a_parse_failure() {
    let db = Database::in_memory().unwrap();
    let key = "events:Charge:real_data:day:2026-09-25";
    let (id, counts) = db
        .persist_fetched_record(&raw(key, json!({"items": [charge("2026-09-25", 255)]})))
        .unwrap();
    assert_eq!(counts.primary_records, 0);
    let quarantined: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM raw_quarantine WHERE raw_record_id = ?1",
            [id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(quarantined, 0);
    // A payload we truly cannot recognize must still be reported.
    assert!(db
        .persist_fetched_record(&raw(
            "events:Charge:real_data:day:2026-09-24",
            json!({"items": [{"eventType": "Charge", "timestamp": day_ms("2026-09-24")}]}),
        ))
        .is_err());
}

/// A database from before 3.0: each sync stored one full-window payload. After compaction each day has only one per-day payload, with
/// the cloud time of the last fetch for that day. Derived rows stay exactly the same, also after replay.
#[test]
fn old_window_payloads_consolidate_into_one_record_per_day_without_changing_derived_rows() {
    let db = Database::in_memory().unwrap();
    let windows = [
        (
            "events:Charge:real_data:1000:2000",
            json!({"items": [charge("2026-09-01", 50), charge("2026-09-02", 255), charge("2026-09-03", 60)]}),
            "2026-09-03T10:00:00+00:00",
        ),
        (
            "events:Charge:real_data:1500:2500",
            json!({"items": []}),
            "2026-09-03T10:15:00+00:00",
        ),
        (
            "events:Charge:real_data:2000:3000",
            json!({"items": [charge("2026-09-02", 255), charge("2026-09-03", 61), charge("2026-09-04", 70)]}),
            "2026-09-04T10:00:00+00:00",
        ),
    ];
    for (key, payload, fetched_at) in &windows {
        // Normalizing an empty window fails (the payload is still kept). That is how the old code stored it.
        let _ = db.persist_fetched_record(&raw(key, payload.clone()));
        set_fetched_at(&db, key, fetched_at);
    }
    // A day that already has a newer per-day payload must not be overwritten by the old window.
    let fresh = "events:Charge:real_data:day:2026-09-04";
    db.persist_fetched_record(&raw(fresh, json!({"items": [charge("2026-09-04", 71)]})))
        .unwrap();
    set_fetched_at(&db, fresh, "2026-09-05T09:00:00+00:00");
    let before = charge_rows(&db);
    assert!(db.pending_raw_payload_count().unwrap() > 0);

    let report = db.compact_raw_payloads().unwrap();

    assert_eq!(
        report.compacted, 3,
        "two windows with content + one empty window that already has a newer fetch"
    );
    assert_eq!(charge_rows(&db), before);
    assert_eq!(
        keys_and_fetch_times(&db),
        vec![
            (
                "events:Charge:real_data:day:2026-09-01".to_string(),
                "2026-09-03T10:00:00+00:00".to_string()
            ),
            (
                "events:Charge:real_data:day:2026-09-02".into(),
                "2026-09-04T10:00:00+00:00".into()
            ),
            (
                "events:Charge:real_data:day:2026-09-03".into(),
                "2026-09-04T10:00:00+00:00".into()
            ),
            (fresh.into(), "2026-09-05T09:00:00+00:00".into()),
        ]
    );
    assert_eq!(
        db.pending_raw_payload_count().unwrap(),
        0,
        "otherwise every startup flashes \"compacting\" again"
    );
    assert_eq!(db.compact_raw_payloads().unwrap().compacted, 0);

    db.reprocess_raw_records().unwrap();
    assert_eq!(
        charge_rows(&db),
        before,
        "replaying the compacted payloads gives the same derived rows"
    );
}

/// A window payload that derived rows still point to (it is the only source of some metric that day) must not be deleted.
#[test]
fn a_window_still_owning_a_derived_row_is_kept() {
    let db = Database::in_memory().unwrap();
    let old_key = "events:Charge:real_data:1000:2000";
    db.persist_fetched_record(&raw(old_key, json!({"items": [charge("2026-09-01", 50)]})))
        .unwrap();
    set_fetched_at(&db, old_key, "2026-09-01T10:00:00+00:00");
    // Later that day became a sentinel: the new version produces no hybrid_charge, and the old row still points to the old window.
    let new_key = "events:Charge:real_data:3000:4000";
    db.persist_fetched_record(&raw(new_key, json!({"items": [charge("2026-09-01", 255)]})))
        .unwrap();
    set_fetched_at(&db, new_key, "2026-09-02T10:00:00+00:00");
    let before = charge_rows(&db);

    db.compact_raw_payloads().unwrap();

    assert_eq!(charge_rows(&db), before);
    let keys: Vec<String> = keys_and_fetch_times(&db)
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    assert!(keys.contains(&old_key.to_string()), "{keys:?}");
    assert!(!keys.contains(&new_key.to_string()), "{keys:?}");
}

/// Before the first compaction, keep a backup we can verify: compaction deletes thousands of payloads, and they cannot come back.
/// After that, when there is nothing new to compact, we do not back up again at each startup.
#[test]
fn the_first_consolidation_leaves_a_verified_backup_behind() {
    let dir = std::env::temp_dir().join(format!(
        "zeppbridge-consolidate-backup-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = Database::new(dir.join("zepp.db")).unwrap();
    db.persist_fetched_record(&raw(
        "events:Charge:real_data:1000:2000",
        json!({"items": [charge("2026-09-01", 50)]}),
    ))
    .unwrap();
    assert!(backup::list_backups(&dir).unwrap().is_empty());

    // Take a snapshot before compaction. When compaction is complete and the database is intact, the snapshot is deleted silently.
    db.compact_raw_payloads().unwrap();
    assert!(backup::list_backups(&dir).unwrap().is_empty());

    db.compact_raw_payloads().unwrap();
    assert!(backup::list_backups(&dir).unwrap().is_empty());
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}
