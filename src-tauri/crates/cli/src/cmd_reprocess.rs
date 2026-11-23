//! Open the database, reprocess on demand, the reprocess subcommand (split out of main.rs, same logic).

use super::*;

pub(super) fn open_read_only() -> Result<Database, (u8, String)> {
    let dir = data_dir().map_err(|message| (EXIT_FAILED, message))?;
    let db_path = dir.join("zepp.db");
    if !db_path.exists() {
        return Err((
            EXIT_NOT_CONFIGURED,
            format!(
                "No local database yet. Connect an account in the ZeppBridge desktop app \
                 and sync once first, or copy an existing zepp.db here. {}",
                where_it_looked(&dir)
            ),
        ));
    }
    // Read-only connection: the CLI query path does not take the write lock, so a long sync cannot block it.
    Database::open_read_only(db_path).map_err(|error| match error {
        // A schema mismatch means "upgrade first", not "the database is broken". The scheduler script
        // must respond to each case in a different way, so the exit codes must differ.
        ZeppBridgeError::ConfigError(message) => (EXIT_SCHEMA, message),
        other => {
            let (code, _) = exit_code_for(&other);
            (code, user_text(&other))
        }
    })
}

/* ---------------------------- Parser reprocessing ----------------------------
 * After each parser upgrade, local history must be replayed to catch up: new workout codes apply only to replayed
 * records. Records stored earlier as `unknown:211` do not change by themselves. The desktop app does this in a background
 * thread at startup, but headless users by definition never start the desktop app. So their history
 * would stay forever on the rules of the first sync, while the upgrade looks successful.
 *
 * This split of work is on purpose:
 *
 * * `sync` catches up automatically before it syncs. In a headless setup it is the only command that is already long, already writes the database,
 *   and already runs on a timer. With the replay in it, the user does nothing.
 * * `status` and `export` only show a notice and never run it. A command that should answer in a second but suddenly runs for minutes
 *   is worse than one that reports old data.
 * * `reprocess` is the explicit entry point: for people who want it now, or want to redo the whole database.
 *
 * Opening the database does not replay, for the same reason: it could make **every** command suddenly stop for minutes. */

/// The result of one replay, for the two callers to build their own output.
pub(super) struct ReplayOutcome {
    pub(super) from_revision: Option<String>,
    pub(super) raw_records: i64,
    pub(super) streams: BTreeMap<String, i64>,
    pub(super) elapsed: Duration,
}

impl ReplayOutcome {
    pub(super) fn total_records(&self) -> i64 {
        self.streams.values().sum()
    }

    pub(super) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "fromRevision": self.from_revision,
            "toRevision": NORMALIZER_REVISION,
            "rawRecords": self.raw_records,
            "streams": self.streams,
            "totalRecords": self.total_records(),
            "elapsedSeconds": (self.elapsed.as_millis() as f64) / 1000.0,
        })
    }

    pub(super) fn human(&self) -> String {
        format!(
            "Replayed {} stored payloads with the current parser: {} derived records in {:.1}s",
            self.raw_records,
            self.total_records(),
            self.elapsed.as_secs_f64()
        )
    }
}

/// Say what will happen before the replay. Minutes of silence look the same as a hang in the terminal.
pub(super) fn announce_replay(plan: &ReplayPlan) {
    let scope = if plan.streams.is_empty() {
        "every stream".to_string()
    } else {
        plan.streams.join(", ")
    };
    // `--all` can run when the revision did not change. Then "upgraded from X to X" would be false,
    // and the user would think they just missed an upgrade.
    let reason = if plan.stored_revision.as_deref() == Some(plan.target_revision.as_str()) {
        format!(
            "Replaying with the current parser ({})",
            plan.target_revision
        )
    } else {
        format!(
            "Parser upgraded from {} to {}; replaying",
            plan.stored_revision
                .as_deref()
                .unwrap_or("an earlier revision"),
            plan.target_revision
        )
    };
    eprintln!(
        "{reason}: {} stored payloads ({scope}). No network is used. Do not interrupt.",
        plan.raw_records
    );
}

/// Run one replay and hold the cross-process write lock the whole time.
///
/// The lock is taken and released here: the sync takes the same lock again, and nesting would deadlock.
///
/// Returns `None` = when we got the lock, nothing was left to do. In the twenty seconds of waiting, the desktop app or
/// another `zeppbridge-cli` may have just finished the same replay. Then a report like "replayed
/// N responses, got 0 derived records" would fool people with a self-contradicting number.
pub(super) fn run_replay(
    dir: &Path,
    db: &Database,
    plan: &ReplayPlan,
    force_all: bool,
) -> Result<Option<ReplayOutcome>, (u8, &'static str, String)> {
    // A replay rewrites all derived data, so it must exclude sync, migration and restore. Wait 20 seconds. If the lock does not come,
    // report busy, not failure: the scheduler script retries on that and must not raise an alert.
    let _guard =
        write_lock::acquire_with_timeout(dir, WritePurpose::Reprocess, Duration::from_secs(20))
            .map_err(|error| {
                let code = write_lock_exit(&error);
                (code, error_kind_for(code), error.to_string())
            })?;
    let started = Instant::now();
    let streams = if force_all {
        let streams = db
            .reprocess_raw_records()
            .map_err(|error| replay_failure(&error))?;
        // A manual reparse is recorded on its own timeline. The cloud sync time does not change.
        db.record_local_replay(true)
            .map_err(|error| replay_failure(&error))?;
        Some(streams)
    } else {
        db.reprocess_raw_records_if_needed()
            .map_err(|error| replay_failure(&error))?
    };
    Ok(streams.map(|streams| ReplayOutcome {
        from_revision: plan.stored_revision.clone(),
        raw_records: plan.raw_records,
        streams,
        elapsed: started.elapsed(),
    }))
}

pub(super) fn replay_failure(error: &ZeppBridgeError) -> (u8, &'static str, String) {
    let (code, kind) = exit_code_for(error);
    (code, kind, user_text(error))
}

/// Ask read-only "does this database need a replay?" If yes, show a notice.
///
/// One SELECT, which a short command can afford. It never replaces the replay: to say it and to run four minutes on our own
/// are two different things.
pub(super) fn pending_replay_notice(plan: Option<&ReplayPlan>) -> Option<String> {
    let plan = plan?;
    if plan.raw_records == 0 {
        return None;
    }
    Some(format!(
        "Derived data here still comes from {}; the current parser is {}. {} stored payloads need a replay. Run `zeppbridge-cli reprocess`, or let the next sync do it.",
        plan.stored_revision.as_deref().unwrap_or("an earlier revision"),
        plan.target_revision,
        plan.raw_records
    ))
}

/// Open a writable connection and run the schema migration as well.
///
/// A headless setup has no "start the desktop app once first" step, so migration can happen only here.
pub(super) fn open_writable() -> Result<(std::path::PathBuf, Database), (u8, String)> {
    let dir = data_dir().map_err(|message| (EXIT_FAILED, message))?;
    let db_path = dir.join("zepp.db");
    if !db_path.exists() {
        return Err((
            EXIT_NOT_CONFIGURED,
            format!(
                "No local database yet. Connect an account in the ZeppBridge desktop app \
                 and sync once first, or copy an existing zepp.db here. {}",
                where_it_looked(&dir)
            ),
        ));
    }
    let db = Database::open_migrated(&db_path).map_err(|error| {
        let (code, _) = exit_code_for(&error);
        (code, user_text(&error))
    })?;
    Ok((dir, db))
}

/* ---------------------------- reprocess ---------------------------- */

pub(super) fn cmd_reprocess(args: &[String]) -> u8 {
    let json_mode = scan_json_flag(args);
    let flags = match Flags::parse(args) {
        Ok(flags) => flags,
        Err(message) => return fail(json_mode, EXIT_USAGE, "usage", &message),
    };
    if let Err(message) = flags.reject_unknown(&["json", "all"]) {
        return fail(json_mode, EXIT_USAGE, "usage", &message);
    }
    if let Err(message) = flags.reject_duplicates() {
        return fail(json_mode, EXIT_USAGE, "usage", &message);
    }
    let force_all = flags.has("all");

    let (dir, db) = match open_writable() {
        Ok(value) => value,
        Err((code, message)) => return fail(json_mode, code, error_kind_for(code), &message),
    };
    let plan = match db.pending_replay_plan() {
        Ok(plan) => plan,
        Err(error) => {
            let (code, kind) = exit_code_for(&error);
            return fail(json_mode, code, kind, &user_text(&error));
        }
    };

    // Without --all, do nothing when the revision already matches. This command must be safe to put in
    // a scheduled task: running once an hour must not replay the whole database once an hour.
    if plan.is_none() && !force_all {
        emit(
            json_mode,
            serde_json::json!({
                "ok": true,
                "replayed": false,
                "reason": "up_to_date",
                "revision": NORMALIZER_REVISION,
            }),
            &format!("Derived data already comes from the current parser ({NORMALIZER_REVISION}); nothing to replay"),
        );
        return EXIT_OK;
    }

    // With --all we also need a plan: the report must say which revision we came from and how many responses we processed.
    let plan = plan.unwrap_or_else(|| ReplayPlan {
        stored_revision: Some(NORMALIZER_REVISION.to_string()),
        target_revision: NORMALIZER_REVISION.to_string(),
        streams: Vec::new(),
        raw_records: 0,
    });
    let effective = if force_all {
        let mut forced = plan.clone();
        forced.streams = Vec::new();
        forced.raw_records = match db.raw_record_count() {
            Ok(count) => count,
            Err(error) => {
                let (code, kind) = exit_code_for(&error);
                return fail(json_mode, code, kind, &user_text(&error));
            }
        };
        forced
    } else {
        plan
    };
    announce_replay(&effective);

    match run_replay(&dir, &db, &effective, force_all) {
        Ok(Some(outcome)) => {
            let mut payload = outcome.to_json();
            payload["ok"] = serde_json::Value::Bool(true);
            payload["replayed"] = serde_json::Value::Bool(true);
            payload["reason"] = serde_json::Value::String(
                if force_all {
                    "forced"
                } else {
                    "revision_changed"
                }
                .into(),
            );
            emit(json_mode, payload, &outcome.human());
            EXIT_OK
        }
        Ok(None) => {
            emit(
                json_mode,
                serde_json::json!({
                    "ok": true,
                    "replayed": false,
                    "reason": "already_done",
                    "revision": NORMALIZER_REVISION,
                }),
                "Another ZeppBridge finished this replay while we waited for the write lock",
            );
            EXIT_OK
        }
        Err((code, kind, message)) => fail(json_mode, code, kind, &message),
    }
}

/* ------------------------------ status ------------------------------ */
