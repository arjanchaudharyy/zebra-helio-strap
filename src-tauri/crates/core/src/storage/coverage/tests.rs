use super::*;

fn db() -> Database {
    Database::in_memory().unwrap()
}

fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

#[test]
fn new_wellness_fetch_contract_rechecks_legacy_months_once() {
    let db = db();
    let from = date("2026-08-01");
    let to = date("2026-08-31");
    db.plan_backfill(from, to).unwrap();
    for stream in ["wellness", "daily_summary"] {
        db.record_backfill_chunk(stream, "2026-08-01", ChunkStatus::Persisted, 12, None, None)
            .unwrap();
    }
    db.set_app_meta("wellness_backfill_fetch_revision", "legacy")
        .unwrap();
    db.plan_backfill(from, to).unwrap();
    let queue = db.pending_backfill_chunks(100).unwrap();
    assert!(queue.iter().any(|c| c.stream == "wellness"));
    assert!(!queue.iter().any(|c| c.stream == "daily_summary"));
    db.record_backfill_chunk(
        "wellness",
        "2026-08-01",
        ChunkStatus::Persisted,
        14,
        None,
        None,
    )
    .unwrap();
    db.plan_backfill(from, to).unwrap();
    assert!(!db
        .pending_backfill_chunks(100)
        .unwrap()
        .iter()
        .any(|c| c.stream == "wellness"));
}

/// Regression gate for issue #10.
///
/// The old code took `pending_backfill_chunks(1)` each round. A failed chunk written back as `failed` still
/// met the pending condition and kept its sort position, so the next round picked it again. The other streams
/// of that month and earlier months never got a turn. This asserts the queue still hands out the other chunks when one keeps failing.
#[test]
fn a_failing_chunk_does_not_starve_the_rest_of_the_queue() {
    let db = db();
    db.plan_backfill(date("2026-07-01"), date("2026-08-31"))
        .unwrap();

    // heart_rate keeps failing in the newest month. With stream ASC sorting it is near the front
    // (after daily_summary), which is exactly where it blocks the others most easily.
    for round in 1..=MAX_AUTO_ATTEMPTS {
        let queue = db.pending_backfill_chunks(24).unwrap();
        assert!(
            queue
                .iter()
                .any(|chunk| chunk.stream == "heart_rate" && chunk.chunk_start == "2026-08-01"),
            "in round {round} the failed chunk must still be retryable"
        );
        // Each (stream, month) appears only once per round. This is what keeps the others from starving.
        let mut seen: Vec<(String, String)> = queue
            .iter()
            .map(|chunk| (chunk.stream.clone(), chunk.chunk_start.clone()))
            .collect();
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(before, seen.len(), "the same chunk must not repeat within one round");

        // The other streams and earlier months must all be in this round's queue.
        assert!(queue
            .iter()
            .any(|chunk| chunk.stream == "sleep" && chunk.chunk_start == "2026-08-01"));
        assert!(queue
            .iter()
            .any(|chunk| chunk.stream == "heart_rate" && chunk.chunk_start == "2026-07-01"));

        db.record_backfill_chunk(
            "heart_rate",
            "2026-08-01",
            ChunkStatus::Failed,
            0,
            Some("network interrupted"),
            Some("err.core.network"),
        )
        .unwrap();
    }

    // After automatic retries run out, this chunk leaves the automatic queue, and the others still move on.
    let queue = db.pending_backfill_chunks(24).unwrap();
    assert!(
        !queue
            .iter()
            .any(|chunk| chunk.stream == "heart_rate" && chunk.chunk_start == "2026-08-01"),
        "a chunk with no retries left must not hold the head of the queue"
    );
    assert!(
        queue.len() >= (BACKFILL_STREAMS.len() * 2) - 1,
        "the other chunks must still be workable"
    );
}

#[test]
fn attempts_accumulate_on_failure_and_reset_on_success() {
    let db = db();
    db.plan_backfill(date("2026-08-01"), date("2026-08-31"))
        .unwrap();

    for _ in 0..2 {
        db.record_backfill_chunk(
            "hrv",
            "2026-08-01",
            ChunkStatus::Failed,
            0,
            Some("timeout"),
            Some("err.backfill.no_canonical_records"),
        )
        .unwrap();
    }
    let failed = db.failed_backfill_chunks().unwrap();
    let entry = failed
        .iter()
        .find(|chunk| chunk.stream == "hrv")
        .expect("a failed chunk must carry details");
    assert_eq!(entry.attempts, 2);
    assert_eq!(entry.error.as_deref(), Some("timeout"));
    assert!(!entry.exhausted);

    // Once it succeeds, it must not carry its history any longer.
    db.record_backfill_chunk("hrv", "2026-08-01", ChunkStatus::Persisted, 42, None, None)
        .unwrap();
    assert!(db
        .failed_backfill_chunks()
        .unwrap()
        .iter()
        .all(|chunk| chunk.stream != "hrv"));
}

#[test]
fn manual_retry_only_revives_failed_chunks() {
    let db = db();
    db.plan_backfill(date("2026-08-01"), date("2026-08-31"))
        .unwrap();
    db.record_backfill_chunk(
        "sleep",
        "2026-08-01",
        ChunkStatus::Persisted,
        30,
        None,
        None,
    )
    .unwrap();
    db.record_backfill_chunk(
        "wellness",
        "2026-08-01",
        ChunkStatus::EmptyFromCloud,
        0,
        None,
        None,
    )
    .unwrap();
    for _ in 0..MAX_AUTO_ATTEMPTS {
        db.record_backfill_chunk(
            "heart_rate",
            "2026-08-01",
            ChunkStatus::Failed,
            0,
            Some("parse failed"),
            Some("err.backfill.no_canonical_records"),
        )
        .unwrap();
    }

    let ledger = db.coverage_ledger().unwrap();
    assert!(ledger.needs_manual_retry, "a chunk with no retries left must prompt the user");
    assert_eq!(ledger.failed_chunks_detail.len(), 1);
    assert_eq!(ledger.failed_chunks_detail[0].chunk_start, "2026-08-01");

    assert_eq!(db.reset_failed_backfill_chunks().unwrap(), 1);
    assert!(db
        .pending_backfill_chunks(24)
        .unwrap()
        .iter()
        .any(|chunk| chunk.stream == "heart_rate"));

    // Persisted chunks and chunks the cloud confirmed empty must not be reset by "retry failed items".
    let after = db.coverage_ledger().unwrap();
    let sleep = after
        .streams
        .iter()
        .find(|item| item.stream == "sleep")
        .unwrap();
    assert_eq!(sleep.persisted_chunks, 1);
    let wellness = after
        .streams
        .iter()
        .find(|item| item.stream == "wellness")
        .unwrap();
    assert_eq!(wellness.empty_chunks, 1);
}

/// Chinese text sent to the UI must carry a code.
///
/// This is a general gate for this class of bug, not a patch for a few sentences. The backend still sends the
/// original Chinese text as a fallback (the CLI and logs use it), but whenever a field can be Chinese, the UI must
/// get a code from the sibling field `<field>_code` to look up the text in its own language.
///
/// Past leaks: `StorageEstimate.message` and the `error` of failed chunks in `CoverageLedger`.
/// Neither is an "error", so the earlier pass that added codes to errors missed them,
/// and the English UI still showed Chinese.
fn assert_chinese_carries_a_code(value: &serde_json::Value, path: &str) {
    fn has_chinese(text: &str) -> bool {
        text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    }
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if let serde_json::Value::String(text) = child {
                    if has_chinese(text) {
                        let code_key = format!("{key}_code");
                        let code = map.get(&code_key);
                        assert!(
                                matches!(code, Some(serde_json::Value::String(c)) if !c.is_empty()),
                                "{path}.{key} is Chinese but has no {code_key}: the English UI would show this Chinese text as is\nvalue: {text}"
                            );
                    }
                }
                assert_chinese_carries_a_code(child, &format!("{path}.{key}"));
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                assert_chinese_carries_a_code(child, &format!("{path}[{index}]"));
            }
        }
        _ => {}
    }
}

#[test]
fn the_ledger_never_sends_chinese_without_a_code() {
    let db = db();
    db.plan_backfill(date("2026-08-01"), date("2026-08-31"))
        .unwrap();
    // Cover all three failure sources: a core error, a deterministic parse failure, and an old row with no code.
    db.record_backfill_chunk(
        "heart_rate",
        "2026-08-01",
        ChunkStatus::Failed,
        0,
        Some("The cloud returned a payload, but no usable records were parsed"),
        Some("err.backfill.no_canonical_records"),
    )
    .unwrap();
    db.record_backfill_chunk(
        "sleep",
        "2026-08-01",
        ChunkStatus::Failed,
        0,
        Some("Cannot reach the Zepp region. Check the network and try again"),
        Some("err.core.network"),
    )
    .unwrap();

    let ledger = db.coverage_ledger().unwrap();
    let json = serde_json::to_value(&ledger).unwrap();
    assert_chinese_carries_a_code(&json, "ledger");
}

/// "The cloud has nothing" and "we did not understand it" must be recorded separately.
///
/// Field case: one account's heart rate endpoint returned `{"items": []}` for its whole history. It clearly
/// says "no heart rate in this period". The old code treated "parsed 0 rows" as a failure, so
/// the UI listed a long run of red "failed" items, and the user thought months of data were lost.
/// Those chunks also stayed at the head of the queue and blocked all later chunks (the issue #10 case).
///
/// This pins the ledger-side meaning: `empty_from_cloud` is not a failure and is not retried,
/// but it still counts as "concluded", so it does not affect the completeness check.
#[test]
fn a_month_the_cloud_has_nothing_for_is_not_a_failure() {
    let db = db();
    db.plan_backfill(date("2026-08-01"), date("2026-08-31"))
        .unwrap();

    db.record_backfill_chunk(
        "heart_rate",
        "2026-08-01",
        ChunkStatus::EmptyFromCloud,
        0,
        None,
        None,
    )
    .unwrap();

    // Not in the failed list: the UI must not draw it in red.
    assert!(db
        .failed_backfill_chunks()
        .unwrap()
        .iter()
        .all(|chunk| chunk.stream != "heart_rate"));

    // Not back in the pending queue either: it did not fail, so a retry is pointless.
    assert!(db
        .pending_backfill_chunks(24)
        .unwrap()
        .iter()
        .all(|chunk| !(chunk.stream == "heart_rate" && chunk.chunk_start == "2026-08-01")));

    // But it is a "conclusion" and counts toward the completed total.
    let ledger = db.coverage_ledger().unwrap();
    let heart_rate = ledger
        .streams
        .iter()
        .find(|item| item.stream == "heart_rate")
        .unwrap();
    assert_eq!(heart_rate.empty_chunks, 1);
    assert_eq!(heart_rate.failed_chunks, 0);
    assert!(!ledger.needs_manual_retry);
}

#[test]
fn chunks_align_to_calendar_months_and_cover_both_ends() {
    let chunks = month_chunks(date("2026-01-15"), date("2026-03-02"));
    assert_eq!(chunks.len(), 3, "one chunk each for January, February and March");
    assert_eq!(chunks[0].0, date("2026-01-01"));
    assert_eq!(chunks[2].1, date("2026-04-01"));

    // Crossing a year boundary must not break.
    let across = month_chunks(date("2025-12-20"), date("2026-01-05"));
    assert_eq!(across.len(), 2);
    assert_eq!(across[1].0, date("2026-01-01"));

    // A reversed range produces no chunks, not one empty chunk.
    assert!(month_chunks(date("2026-03-01"), date("2026-01-01")).is_empty());
}

#[test]
fn planning_the_same_range_twice_does_not_reopen_finished_chunks() {
    let db = db();
    let planned = db
        .plan_backfill(date("2026-01-01"), date("2026-02-28"))
        .unwrap();
    assert_eq!(planned, (BACKFILL_STREAMS.len() * 2) as i64);

    db.record_backfill_chunk(
        "sleep",
        "2026-01-01",
        ChunkStatus::Persisted,
        31,
        None,
        None,
    )
    .unwrap();

    // Plan the same range again: completed chunks must not go back to pending.
    let replanned = db
        .plan_backfill(date("2026-01-01"), date("2026-02-28"))
        .unwrap();
    assert_eq!(replanned, 0, "replanning must not add any chunks");

    let ledger = db.coverage_ledger().unwrap();
    let sleep = ledger
        .streams
        .iter()
        .find(|stream| stream.stream == "sleep")
        .unwrap();
    assert_eq!(sleep.persisted_chunks, 1);
    assert_eq!(sleep.records, 31);
}

#[test]
fn a_month_the_cloud_had_nothing_for_is_not_a_failure_and_is_not_retried() {
    let db = db();
    db.plan_backfill(date("2026-01-01"), date("2026-01-31"))
        .unwrap();
    db.record_backfill_chunk(
        "hrv",
        "2026-01-01",
        ChunkStatus::EmptyFromCloud,
        0,
        None,
        None,
    )
    .unwrap();

    let pending = db.pending_backfill_chunks(100).unwrap();
    assert!(
        !pending
            .iter()
            .any(|chunk| chunk.stream == "hrv" && chunk.chunk_start == "2026-01-01"),
        "a chunk the cloud confirmed empty must not be retried again and again"
    );

    let ledger = db.coverage_ledger().unwrap();
    let hrv = ledger
        .streams
        .iter()
        .find(|stream| stream.stream == "hrv")
        .unwrap();
    assert_eq!(hrv.empty_chunks, 1);
    assert_eq!(hrv.failed_chunks, 0, "no data is not a failure");
    assert_eq!(hrv.empty_months, vec!["2026-01-01"]);
}

#[test]
fn a_failed_chunk_stays_retryable_and_keeps_its_reason() {
    let db = db();
    db.plan_backfill(date("2026-01-01"), date("2026-01-31"))
        .unwrap();
    db.record_backfill_chunk(
        "workouts",
        "2026-01-01",
        ChunkStatus::Failed,
        0,
        Some("network timeout"),
        Some("err.core.network"),
    )
    .unwrap();

    let pending = db.pending_backfill_chunks(100).unwrap();
    let failed = pending
        .iter()
        .find(|chunk| chunk.stream == "workouts")
        .expect("a failed chunk must still be pending");
    assert_eq!(failed.error.as_deref(), Some("network timeout"));

    // After a successful retry the error must be cleared, not left in place.
    db.record_backfill_chunk(
        "workouts",
        "2026-01-01",
        ChunkStatus::Persisted,
        5,
        None,
        None,
    )
    .unwrap();
    let ledger = db.coverage_ledger().unwrap();
    let workouts = ledger
        .streams
        .iter()
        .find(|stream| stream.stream == "workouts")
        .unwrap();
    assert_eq!(workouts.failed_chunks, 0);
    assert_eq!(workouts.persisted_chunks, 1);
}

#[test]
fn the_ledger_only_calls_itself_complete_when_every_chunk_has_an_answer() {
    let db = db();
    assert!(
        !db.coverage_ledger().unwrap().complete,
        "doing nothing is not the same as done"
    );

    db.plan_backfill(date("2026-01-01"), date("2026-01-31"))
        .unwrap();
    assert!(!db.coverage_ledger().unwrap().complete);

    for stream in BACKFILL_STREAMS {
        db.record_backfill_chunk(stream, "2026-01-01", ChunkStatus::Persisted, 1, None, None)
            .unwrap();
    }
    let ledger = db.coverage_ledger().unwrap();
    assert!(ledger.complete);
    assert_eq!(ledger.completed_chunks, ledger.total_chunks);
    assert_eq!(ledger.requested_from.as_deref(), Some("2026-01-01"));
}

#[test]
fn pending_chunks_come_back_newest_first() {
    let db = db();
    db.plan_backfill(date("2026-01-01"), date("2026-03-31"))
        .unwrap();
    let pending = db.pending_backfill_chunks(3).unwrap();
    assert_eq!(pending.len(), 3);
    assert!(
        pending
            .iter()
            .all(|chunk| chunk.chunk_start == "2026-03-01"),
        "a backfill can be cancelled at any time, so the most recent months come first"
    );
}

#[test]
fn chunk_status_round_trips_including_partial() {
    for status in [
        ChunkStatus::Pending,
        ChunkStatus::Persisted,
        ChunkStatus::EmptyFromCloud,
        ChunkStatus::Partial,
        ChunkStatus::Failed,
    ] {
        assert_eq!(ChunkStatus::parse(status.as_str()), status);
    }
    assert!(ChunkStatus::Partial.needs_work());
    assert!(ChunkStatus::Failed.needs_work());
    assert!(!ChunkStatus::Persisted.needs_work());
    assert!(!ChunkStatus::EmptyFromCloud.needs_work());
}

/// Data was written even after a sub-slice 404 / parse failure: the rows stay in the database, but the month
/// must not be drawn as "fully persisted".
#[test]
fn a_partial_write_is_not_a_complete_local_copy() {
    let db = db();
    db.plan_backfill(date("2026-01-01"), date("2026-01-31"))
        .unwrap();
    db.record_backfill_chunk(
        "heart_rate",
        "2026-01-01",
        ChunkStatus::Partial,
        12,
        Some("This chunk was only partly written and needs a retry"),
        Some("err.backfill.partial_window"),
    )
    .unwrap();

    let pending = db.pending_backfill_chunks(100).unwrap();
    let chunk = pending
        .iter()
        .find(|chunk| chunk.stream == "heart_rate")
        .expect("a partly written chunk must be fetched again");
    assert_eq!(chunk.records, 12);
    assert!(chunk.persisted_at.is_none(), "a partial write must not set persisted_at");
    assert!(ChunkStatus::parse(&chunk.status).needs_work());

    let ledger = db.coverage_ledger().unwrap();
    assert!(!ledger.complete, "a month with holes is not a complete copy");
    let heart_rate = ledger
        .streams
        .iter()
        .find(|stream| stream.stream == "heart_rate")
        .unwrap();
    assert_eq!(heart_rate.persisted_chunks, 0);
    assert_eq!(heart_rate.failed_chunks, 1);
    assert_eq!(heart_rate.records, 12);
    assert_eq!(
        ledger.failed_chunks_detail[0].error_code.as_deref(),
        Some("err.backfill.partial_window")
    );

    let json = serde_json::to_value(&ledger).unwrap();
    assert_chinese_carries_a_code(&json, "ledger");
}

/// Retention cleanup before 3.0 deleted data without updating the ledger: months marked "persisted" with no payload in the window
/// go back to pending backfill on upgrade (v41). Months that still have payloads, or that the cloud confirmed empty, stay. Fixed only once.
#[test]
fn months_marked_persisted_without_any_raw_are_reopened_once_on_upgrade() {
    use crate::models::{CapabilityStatus, RawRecord, SourceScope};
    let db = db();
    db.plan_backfill(date("2024-08-01"), date("2024-09-30"))
        .unwrap();
    for month in ["2024-08-01", "2024-09-01"] {
        db.record_backfill_chunk("workouts", month, ChunkStatus::Persisted, 19, None, None)
            .unwrap();
    }
    db.record_backfill_chunk(
        "sleep",
        "2024-08-01",
        ChunkStatus::EmptyFromCloud,
        0,
        None,
        None,
    )
    .unwrap();
    // Only September still has payloads.
    let kept = to_utc(date("2024-09-03"));
    db.insert_raw_record(&RawRecord {
        stream: "workouts".into(),
        source_key: "sport_history:run:hkept".into(),
        source_scope: SourceScope::Device,
        device_id: None,
        start_utc: kept,
        end_utc: Some(kept + chrono::Duration::days(1)),
        payload: serde_json::json!({ "data": { "summary": [] } }),
        capability: CapabilityStatus::Verified,
    })
    .unwrap();

    // A 3.0.5 database is upgraded.
    db.conn.execute_batch("PRAGMA user_version = 40;").unwrap();
    db.migrate().unwrap();
    let status = |stream: &str, month: &str| -> String {
        db.conn
            .query_row(
                "SELECT status FROM coverage_ledger WHERE stream = ?1 AND chunk_start = ?2",
                [stream, month],
                |row| row.get(0),
            )
            .unwrap()
    };
    assert_eq!(status("workouts", "2024-08-01"), "pending");
    assert_eq!(status("workouts", "2024-09-01"), "persisted");
    assert_eq!(status("sleep", "2024-08-01"), "empty_from_cloud");
    assert!(db
        .pending_backfill_chunks(100)
        .unwrap()
        .iter()
        .any(|c| c.stream == "workouts" && c.chunk_start == "2024-08-01"));

    // Fixed only once: the migration reruns at every startup and must not scan again (later cleanup updates the ledger itself).
    db.record_backfill_chunk(
        "workouts",
        "2024-08-01",
        ChunkStatus::Persisted,
        19,
        None,
        None,
    )
    .unwrap();
    db.migrate().unwrap();
    assert_eq!(status("workouts", "2024-08-01"), "persisted");
    let version: i64 = db
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, crate::storage::CURRENT_SCHEMA_VERSION);
}

/// Kintsugi month calendar: the gold line goes only to months with records actually written. Months the cloud says are empty are not drawn as cracks.
/// If one stream failed, the month is still a crack, even if other streams were written.
#[test]
fn month_seams_only_gild_months_that_gained_records() {
    let db = db();
    db.plan_backfill(date("2025-01-01"), date("2025-04-30"))
        .unwrap();
    for stream in BACKFILL_STREAMS {
        db.record_backfill_chunk(
            stream,
            "2025-01-01",
            ChunkStatus::EmptyFromCloud,
            0,
            None,
            None,
        )
        .unwrap();
        db.record_backfill_chunk(stream, "2025-02-01", ChunkStatus::Persisted, 0, None, None)
            .unwrap();
    }
    db.record_backfill_chunk(
        "sleep",
        "2025-03-01",
        ChunkStatus::Persisted,
        20,
        None,
        None,
    )
    .unwrap();
    db.record_backfill_chunk(
        "workouts",
        "2025-03-01",
        ChunkStatus::Failed,
        0,
        Some("x"),
        None,
    )
    .unwrap();
    db.record_backfill_chunk("sleep", "2025-04-01", ChunkStatus::Persisted, 9, None, None)
        .unwrap();
    let months = db.coverage_ledger().unwrap().months;
    let state = |m: &str| {
        months
            .iter()
            .find(|seam| seam.month == m)
            .map(|seam| seam.state.as_str())
            .unwrap_or("missing")
            .to_string()
    };
    assert_eq!(state("2025-01"), "empty");
    assert_eq!(state("2025-02"), "empty");
    assert_eq!(state("2025-03"), "crack");
    assert_eq!(state("2025-04"), "gold");
}
