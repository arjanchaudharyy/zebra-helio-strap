//! The status subcommand (split out of main.rs, same logic).

use super::*;

pub(super) fn cmd_status(args: &[String]) -> u8 {
    let json_mode = scan_json_flag(args);
    let flags = match Flags::parse(args) {
        Ok(flags) => flags,
        Err(message) => return fail(json_mode, EXIT_USAGE, "usage", &message),
    };
    if let Err(message) = flags.reject_unknown(&["json"]) {
        return fail(json_mode, EXIT_USAGE, "usage", &message);
    }
    if let Err(message) = flags.reject_duplicates() {
        return fail(json_mode, EXIT_USAGE, "usage", &message);
    }

    let dir = match data_dir() {
        Ok(dir) => dir,
        Err(message) => return fail(json_mode, EXIT_FAILED, "failed", &message),
    };
    let auth_status = AuthManager::new(dir.clone()).status();
    let connected = auth_status
        .as_ref()
        .map(|status| status.configured)
        .unwrap_or(false);

    // The resolved data directory (user name masked). In the exit 6 feedback, the first thing to rule out
    // was "the CLI reads a different directory than the app writes". Before, status did not say where it reads.
    let data_dir_shown = paths::mask_user_path(&dir);
    let db = match open_read_only() {
        Ok(db) => db,
        Err((code, message)) => {
            // Even without a database we must print the reason, or the scheduler script cannot even check status.
            if json_mode {
                println!(
                    "{}",
                    serde_json::json!({
                        "ok": false,
                        "errorKind": error_kind_for(code),
                        "message": message,
                        "dataDir": data_dir_shown,
                    })
                );
                return code;
            }
            return fail(json_mode, code, error_kind_for(code), &message);
        }
    };

    let database_bytes = std::fs::metadata(dir.join("zepp.db"))
        .map(|meta| meta.len())
        .unwrap_or(0);
    let health = match db.data_health(30, database_bytes) {
        Ok(health) => health,
        Err(error) => {
            let (code, kind) = exit_code_for(&error);
            return fail(json_mode, code, kind, &user_text(&error));
        }
    };
    // A read-only question: does this database need a reprocess? status says so, but never runs it.
    // A command that a scheduler script calls every minute must not suddenly run four minutes after a parser upgrade.
    let replay_plan = db.pending_replay_plan().ok().flatten();
    let replay_notice = pending_replay_notice(replay_plan.as_ref());
    let ledger = db.coverage_ledger().ok();
    let workouts = db.get_recent_workouts(1).unwrap_or_default();
    // "How much history is on this machine" is a question every output must answer, not only the desktop app:
    // a scheduler script also needs to know, before it exports six months, if this machine has only 30 days.
    let coverage = db
        .local_coverage(Local::now().date_naive())
        .unwrap_or_default();

    let payload = serde_json::json!({
        "ok": true,
        "version": VERSION,
        "connected": connected,
        "dataDir": data_dir_shown,
        // Which fallback the read-only open used (`immutable` / `truncated_header`). Usually null.
        "readFallback": db.read_fallback().map(|fallback| fallback.as_str()),
        "databaseBytes": database_bytes,
        "schemaVersion": health.database.schema_version,
        // The revision actually stored in the database, not this program's own constant. The two can differ,
        // and a difference is the only thing worth reporting here.
        "normalizerRevision": health.database.stored_normalizer_revision,
        "normalizerRevisionExpected": NORMALIZER_REVISION,
        "normalizerReplayPending": health.database.normalizer_replay_pending,
        "normalizerReplayRawRecords": replay_plan
            .as_ref()
            .map(|plan| plan.raw_records)
            .unwrap_or(0),
        "lastCloudSyncAt": health.timings.last_cloud_sync_at,
        "latestWorkoutAt": workouts.first().map(|workout| workout.start_time.to_rfc3339()),
        "streams": health.streams.iter().map(|stream| serde_json::json!({
            "stream": stream.stream,
            "fetch": stream.fetch.state,
            "parse": stream.parse.state,
            "write": stream.write.state,
            "rawRecords": stream.raw_records,
            "canonicalRecords": stream.canonical_records,
        })).collect::<Vec<_>>(),
        "historyPlanned": ledger.as_ref().map(|value| value.total_chunks > 0),
        "historyComplete": ledger.as_ref().map(|value| value.complete),
        "historyPendingChunks": ledger
            .as_ref()
            .map(|value| value.total_chunks - value.completed_chunks),
        "coverageEarliestDay": coverage.earliest_day,
        "coverageLatestDay": coverage.latest_day,
        "coverageDays": coverage.covered_days,
    });

    let human = format!(
        "ZeppBridge {VERSION}\nData folder: {data_dir_shown}\nAccount: {}\nDatabase: {} bytes, schema v{}\nLocal coverage: {}\nLast cloud sync: {}\nHistory ledger: {}{}",
        if connected { "connected" } else { "not connected" },
        database_bytes,
        health.database.schema_version,
        // JSON had these three fields for a long time, but plain text was missing a line. Two outputs of one command
        // that give different answers to "how much history is on this machine" is the worst thing this project can do.
        match coverage.earliest_day.as_deref() {
            Some(day) => format!("{} days, earliest {}", coverage.covered_days, day),
            None => "nothing stored yet".to_string(),
        },
        health
            .timings
            .last_cloud_sync_at
            .as_deref()
            .unwrap_or("never"),
        // "Never scheduled any" and "scheduled but not done" are not the same. The first is not a slow backfill;
        // the backfill was never planned at all.
        match ledger.as_ref() {
            Some(value) if value.total_chunks == 0 => "no backfill planned yet".to_string(),
            Some(value) if value.complete => "every month chunk has an outcome".to_string(),
            Some(value) => format!(
                "{} month chunks still without an outcome",
                value.total_chunks - value.completed_chunks
            ),
            None => "ledger unreadable".to_string(),
        },
        match replay_notice.as_deref() {
            Some(notice) => format!("\nParser: {notice}"),
            None => String::new(),
        }
    );
    emit(json_mode, payload, &human);
    EXIT_OK
}

/* ------------------------------ sync ------------------------------ */
