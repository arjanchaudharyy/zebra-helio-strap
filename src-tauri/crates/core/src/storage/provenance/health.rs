//! Data health report: three stages per stream, coverage explanation and suggested actions (split out of storage/provenance.rs, logic unchanged).

use super::*;

impl Database {
    /// Records the result of one stage of one stream.
    ///
    /// Each call touches only one stage's columns, so a combination such as fetch ok, parse failed is
    /// kept as it is, and is not flattened into one state by the last write.
    pub fn record_stream_stage(
        &self,
        stream: &str,
        stage: Stage,
        outcome: &StageOutcome,
    ) -> Result<()> {
        // Backfill and official supplements do not write user-visible per-stream state (see `Database::quiet_sync_state`).
        if self.sync_state_is_quiet() {
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        let prefix = stage.column_prefix();
        self.conn.execute(
            "INSERT OR IGNORE INTO stream_provenance(stream, updated_at) VALUES(?1, ?2)",
            rusqlite::params![stream, now],
        )?;
        match outcome {
            StageOutcome::Ok => {
                let sql = format!(
                    "UPDATE stream_provenance
                     SET last_{prefix}_ok_at = ?2,
                         last_{prefix}_error_at = NULL,
                         last_{prefix}_error_kind = NULL,
                         last_{prefix}_error_message = NULL,
                         updated_at = ?2
                     WHERE stream = ?1"
                );
                self.conn.execute(&sql, rusqlite::params![stream, now])?;
            }
            StageOutcome::Failed { kind, message } => {
                // The business code gets its own column. It is also in `message`, but that is a
                // human-readable sentence, and the diagnostic report can only send allowlisted fields. Pulling the
                // number out of prose with a regex would turn the text into a protocol.
                let code = match kind {
                    StageErrorKind::CloudRejected { code } => Some(*code),
                    _ => None,
                };
                let sql = format!(
                    "UPDATE stream_provenance
                     SET last_{prefix}_error_at = ?2,
                         last_{prefix}_error_kind = ?3,
                         last_{prefix}_error_message = ?4,
                         last_error_code = ?5,
                         updated_at = ?2
                     WHERE stream = ?1"
                );
                self.conn.execute(
                    &sql,
                    rusqlite::params![stream, now, kind.as_str(), message.as_deref(), code],
                )?;
            }
        }
        Ok(())
    }

    /// Records how many canonical rows this run wrote. Kept apart from stage state, because "write ok but
    /// 0 rows" and "write failed" are two different things.
    pub fn record_stream_written(&self, stream: &str, records: i64) -> Result<()> {
        if self.sync_state_is_quiet() {
            return Ok(());
        }
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO stream_provenance(stream, last_written_records, updated_at)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(stream) DO UPDATE SET
                 last_written_records = excluded.last_written_records,
                 updated_at = excluded.updated_at",
            rusqlite::params![stream, records, now],
        )?;
        Ok(())
    }

    /// Time the local replay finished. **Does not change the cloud sync time.**
    pub fn record_local_replay(&self, manual: bool) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.set_app_meta(LAST_LOCAL_REPLAY_AT_KEY, &now)?;
        if manual {
            self.set_app_meta(LAST_MANUAL_REPROCESS_AT_KEY, &now)?;
        }
        Ok(())
    }

    /// Runs `PRAGMA integrity_check` explicitly and records the result.
    ///
    /// On a large database this is a full table scan, so it is a user-triggered action and does not run
    /// when the page opens. Normally the page shows the last result and its time.
    pub fn run_integrity_check(&self) -> Result<IntegrityCheckResult> {
        let first: String = self
            .conn
            .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))?;
        let ok = first.eq_ignore_ascii_case("ok");
        let result = IntegrityCheckResult {
            checked_at: Utc::now().to_rfc3339(),
            ok,
            detail: (!ok).then(|| first.clone()),
        };
        if let Ok(encoded) = serde_json::to_string(&result) {
            self.set_app_meta(LAST_INTEGRITY_CHECK_KEY, &encoded)?;
        }
        Ok(result)
    }

    pub(super) fn last_integrity_check(&self) -> Result<Option<IntegrityCheckResult>> {
        Ok(self
            .get_app_meta(LAST_INTEGRITY_CHECK_KEY)?
            .and_then(|value| serde_json::from_str(&value).ok()))
    }

    #[allow(clippy::type_complexity)]
    pub(super) fn stage_states(
        &self,
    ) -> Result<(BTreeMap<String, [StageState; 3]>, BTreeMap<String, i64>)> {
        let mut states = BTreeMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT stream,
                    last_fetch_ok_at, last_fetch_error_at, last_fetch_error_kind, last_fetch_error_message,
                    last_parse_ok_at, last_parse_error_at, last_parse_error_kind, last_parse_error_message,
                    last_write_ok_at, last_write_error_at, last_write_error_kind, last_write_error_message,
                    last_written_records
             FROM stream_provenance",
        )?;
        let rows = stmt.query_map([], |row| {
            let stream: String = row.get(0)?;
            let mut stages = [
                StageState::never(),
                StageState::never(),
                StageState::never(),
            ];
            for (index, base) in [1usize, 5, 9].into_iter().enumerate() {
                let ok_at: Option<String> = row.get(base)?;
                let error_at: Option<String> = row.get(base + 1)?;
                let error_kind: Option<String> = row.get(base + 2)?;
                let error_message: Option<String> = row.get(base + 3)?;
                // The latest event decides the current state: a success after a failure must show as success.
                let failed_is_newer = match (&ok_at, &error_at) {
                    (_, None) => false,
                    (None, Some(_)) => true,
                    (Some(ok), Some(err)) => err.as_str() > ok.as_str(),
                };
                stages[index] = if failed_is_newer {
                    StageState {
                        state: "failed".into(),
                        at: error_at,
                        last_ok_at: ok_at,
                        error_kind,
                        message: error_message,
                    }
                } else if ok_at.is_some() {
                    StageState {
                        state: "ok".into(),
                        at: ok_at.clone(),
                        last_ok_at: ok_at,
                        error_kind: None,
                        message: None,
                    }
                } else {
                    StageState::never()
                };
            }
            let written: i64 = row.get(13)?;
            Ok((stream, stages, written))
        })?;
        let mut written = BTreeMap::new();
        for row in rows {
            let (stream, stages, count) = row?;
            written.insert(stream.clone(), count);
            states.insert(stream, stages);
        }
        Ok((states, written))
    }

    pub(super) fn scalar_i64(&self, sql: &str) -> Result<i64> {
        Ok(self.conn.query_row(sql, [], |row| row.get(0))?)
    }

    pub(super) fn source_breakdown(
        &self,
        table: &str,
        filter: &str,
    ) -> Result<Vec<SourceBreakdown>> {
        let sql = format!(
            "SELECT COALESCE(NULLIF(TRIM(source_scope), ''), 'unknown') AS scope, COUNT(*)
             FROM {table} {filter}
             GROUP BY scope ORDER BY COUNT(*) DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(SourceBreakdown {
                source: normalize_source(&row.get::<_, String>(0)?),
                records: row.get(1)?,
            })
        })?;
        // Normalization can produce duplicate keys (for example two spellings both map to unknown).
        // Merge the counts instead of showing two rows.
        let mut merged: BTreeMap<String, i64> = BTreeMap::new();
        for row in rows {
            let row = row?;
            *merged.entry(row.source).or_default() += row.records;
        }
        let mut out: Vec<SourceBreakdown> = merged
            .into_iter()
            .map(|(source, records)| SourceBreakdown { source, records })
            .collect();
        out.sort_by(|a, b| b.records.cmp(&a.records).then(a.source.cmp(&b.source)));
        Ok(out)
    }

    /// The full backend contract of the data health center.
    ///
    /// This call does not run `integrity_check` and does not touch the network: opening the page must be cheap.
    pub fn data_health(&self, window_days: i64, database_bytes: u64) -> Result<DataHealth> {
        let window_days = window_days.clamp(7, 3650);
        let (stages, written) = self.stage_states()?;
        let freshness = self.stream_freshness()?;

        let mut streams = Vec::new();
        for (stream, label, cadence) in STREAM_CATALOG {
            let raw_records = self.conn.query_row(
                "SELECT COUNT(*) FROM raw_records WHERE stream = ?1",
                [stream],
                |row| row.get::<_, i64>(0),
            )?;
            let (canonical_records, sources, observed) = self.stream_facts(stream, window_days)?;
            let stage = stages.get(stream);
            let mut fetch = stage.map_or_else(StageState::never, |s| s[0].clone());
            let parse = stage.map_or_else(StageState::never, |s| s[1].clone());
            let write = stage.map_or_else(StageState::never, |s| s[2].clone());
            // When an old database is upgraded, the provenance table is empty, but raw_records clearly holds
            // payloads. Writing "never fetched" as never would mislead the user, so the existing
            // fetched_at serves as the lower bound of the last successful fetch.
            if fetch.state == "never" {
                if let Some(at) = freshness
                    .get(stream)
                    .and_then(|value| value.last_cloud_sync_at.clone())
                {
                    fetch = StageState {
                        state: "ok".into(),
                        at: Some(at.clone()),
                        last_ok_at: Some(at),
                        error_kind: None,
                        message: None,
                    };
                }
            }
            streams.push(StreamHealth {
                stream: stream.to_string(),
                label: label.to_string(),
                cadence,
                fetch,
                parse,
                write,
                raw_records,
                canonical_records,
                last_written_records: written.get(stream).copied().unwrap_or(0),
                sources,
                coverage: explain_coverage(cadence, window_days, observed),
            });
        }

        let occasional_metrics = self.metric_health(window_days)?;

        let raw_total = self.scalar_i64("SELECT COUNT(*) FROM raw_records")?;
        let canonical_total = self.scalar_i64(
            "SELECT (SELECT COUNT(*) FROM metric_samples)
                  + (SELECT COUNT(*) FROM daily_metrics)
                  + (SELECT COUNT(*) FROM sleep_sessions)
                  + (SELECT COUNT(*) FROM workouts)",
        )?;
        // Canonical rows are upserted by natural keys: losing ownership does
        // not undo a successful normalization attempt.
        let normalization_by_stream = self.normalization_health()?;
        let pending_normalization = normalization_by_stream
            .iter()
            .filter(|row| row.state == "pending")
            .map(|row| row.records)
            .sum();

        let (last_cloud_sync_at, last_cloud_sync_outcome) = self.cloud_sync_metadata()?;
        let newest_sample_at = freshness
            .values()
            .filter_map(|value| value.newest_sample_at.clone())
            .max();

        // An empty database owes no replay: no revision is recorded only because there is nothing to replay yet.
        // Telling it "history is stuck on an old parser" is a warning with no content.
        let replay_plan = self.pending_replay_plan()?;
        let database = DatabaseHealth {
            schema_version: self.diagnostic_schema_version()?,
            normalizer_revision: NORMALIZER_REVISION.to_string(),
            stored_normalizer_revision: self.stored_normalizer_revision()?,
            normalizer_replay_pending: replay_plan
                .as_ref()
                .map(|plan| plan.raw_records > 0)
                .unwrap_or(false),
            replay_in_progress: super::super::replay_in_progress(),
            database_bytes,
            raw_records: raw_total,
            canonical_records: canonical_total,
            pending_normalization,
            normalization_by_stream,
            last_integrity_check: self.last_integrity_check()?,
        };
        let timings = HealthTimings {
            last_cloud_sync_at,
            last_cloud_sync_outcome,
            last_local_replay_at: self.get_app_meta(LAST_LOCAL_REPLAY_AT_KEY)?,
            last_manual_reprocess_at: self.get_app_meta(LAST_MANUAL_REPROCESS_AT_KEY)?,
            newest_sample_at,
        };
        let actions = suggested_actions(&database, &timings, &streams);

        Ok(DataHealth {
            generated_at: Utc::now().to_rfc3339(),
            database,
            timings,
            streams,
            occasional_metrics,
            actions,
        })
    }

    /// Canonical counts, source split and the set of observed dates.
    pub(super) fn stream_facts(
        &self,
        stream: &str,
        window_days: i64,
    ) -> Result<(i64, Vec<SourceBreakdown>, Observed)> {
        let cutoff = (Utc::now() - Duration::days(window_days))
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        match stream {
            "heart_rate" | "hrv" => {
                let metric = if stream == "heart_rate" {
                    "heart_rate"
                } else {
                    "hrv"
                };
                // The heart rate stream alone has over 100,000 rows: the total is summed from the source split instead of
                // counted again. "Last N days" uses `timestamp >= date` for an index range scan. RFC 3339
                // compares as a string, which is the same as comparing the first 10 characters.
                let sources =
                    self.source_breakdown("metric_samples", &format!("WHERE metric = '{metric}'"))?;
                let count = sources.iter().map(|source| source.records).sum();
                let observed = self.observed_days_for(
                    &format!(
                        "SELECT DISTINCT substr(timestamp, 1, 10) FROM metric_samples
                         WHERE metric = '{metric}' AND timestamp >= ?1"
                    ),
                    &cutoff,
                )?;
                Ok((count, sources, observed))
            }
            "daily_summary" | "wellness" => {
                // Only rows owned by this stream establish its coverage.
                // Food/stress must not make a failed daily-summary day look complete.
                let filter = format!(
                    "WHERE raw_record_id IN (SELECT id FROM raw_records WHERE stream = '{stream}')"
                );
                let rows = if stream == "wellness" {
                    format!("(SELECT date, source_scope FROM daily_metrics {filter}
                        UNION ALL SELECT date(timestamp, 'localtime') AS date, source_scope FROM metric_samples {filter})")
                } else {
                    format!("(SELECT date, source_scope FROM daily_metrics {filter})")
                };
                let count = self.scalar_i64(&format!("SELECT COUNT(*) FROM {rows}"))?;
                let sources = self.source_breakdown(&rows, "")?;
                let observed = self.observed_days_for(
                    &format!("SELECT DISTINCT date FROM {rows} WHERE date >= ?1"),
                    &cutoff,
                )?;
                Ok((count, sources, observed))
            }
            "sleep" => {
                let count = self.scalar_i64("SELECT COUNT(*) FROM sleep_sessions")?;
                let sources = self.source_breakdown("sleep_sessions", "")?;
                let observed = self.observed_days_for(
                    "SELECT DISTINCT substr(end_time, 1, 10) FROM sleep_sessions
                     WHERE substr(end_time, 1, 10) >= ?1",
                    &cutoff,
                )?;
                Ok((count, sources, observed))
            }
            "workouts" | "workout_detail" => {
                let count = self.scalar_i64("SELECT COUNT(*) FROM workouts")?;
                let sources = self.source_breakdown("workouts", "")?;
                let observed = self.observed_days_for(
                    "SELECT DISTINCT substr(start_time, 1, 10) FROM workouts
                     WHERE substr(start_time, 1, 10) >= ?1",
                    &cutoff,
                )?;
                Ok((count, sources, observed))
            }
            _ => Ok((0, Vec::new(), Observed::default())),
        }
    }

    pub(super) fn observed_days_for(&self, sql: &str, cutoff: &str) -> Result<Observed> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([cutoff], |row| row.get::<_, Option<String>>(0))?;
        let mut days: Vec<String> = rows
            .collect::<std::result::Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        days.sort();
        days.dedup();
        Ok(Observed { days })
    }

    /// One row per metric. Occasional metrics (VO₂max, lactate threshold and so on) report only observed dates
    /// and are not part of the gap check.
    pub(super) fn metric_health(&self, window_days: i64) -> Result<Vec<StreamHealth>> {
        let mut metrics: Vec<String> = Vec::new();
        // Skip from one distinct metric name to the next (one index lookup per step) instead of a DISTINCT that scans
        // the whole table index: metric_samples has hundreds of thousands of rows but only a handful of metrics.
        for table in ["metric_samples", "daily_metrics"] {
            let sql = format!(
                "WITH RECURSIVE m(metric) AS (
                     SELECT MIN(metric) FROM {table}
                     UNION ALL
                     SELECT (SELECT MIN(metric) FROM {table} WHERE metric > m.metric)
                     FROM m WHERE m.metric IS NOT NULL
                 )
                 SELECT metric FROM m WHERE metric IS NOT NULL"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                metrics.push(row?);
            }
        }
        metrics.sort();
        metrics.dedup();

        let cutoff = (Utc::now() - Duration::days(window_days))
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let mut out = Vec::new();
        for metric in metrics {
            let cadence = metric_cadence(&metric);
            if cadence != StreamCadence::Occasional {
                continue;
            }
            let sample_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM metric_samples WHERE metric = ?1",
                [&metric],
                |row| row.get(0),
            )?;
            let daily_count: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM daily_metrics WHERE metric = ?1",
                [&metric],
                |row| row.get(0),
            )?;
            let mut days: Vec<String> = Vec::new();
            for sql in [
                "SELECT DISTINCT substr(timestamp, 1, 10) FROM metric_samples
                 WHERE metric = ?1 AND timestamp >= ?2",
                "SELECT DISTINCT date FROM daily_metrics WHERE metric = ?1 AND date >= ?2",
            ] {
                let mut stmt = self.conn.prepare(sql)?;
                let rows =
                    stmt.query_map([&metric, &cutoff], |row| row.get::<_, Option<String>>(0))?;
                for row in rows {
                    if let Some(day) = row? {
                        days.push(day);
                    }
                }
            }
            days.sort();
            days.dedup();
            let sources = if sample_count >= daily_count {
                self.source_breakdown("metric_samples", &format!("WHERE metric = '{metric}'"))?
            } else {
                self.source_breakdown("daily_metrics", &format!("WHERE metric = '{metric}'"))?
            };
            out.push(StreamHealth {
                label: metric_label(&metric),
                stream: metric,
                cadence,
                fetch: StageState::never(),
                parse: StageState::never(),
                write: StageState::never(),
                raw_records: 0,
                canonical_records: sample_count + daily_count,
                last_written_records: 0,
                sources,
                coverage: explain_coverage(cadence, window_days, Observed { days }),
            });
        }
        Ok(out)
    }
}

#[derive(Debug, Default, Clone)]
pub(super) struct Observed {
    pub(super) days: Vec<String>,
}

pub(super) fn normalize_source(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "device" => "device".into(),
        "user_fused" | "user" | "fused" => "user_fused".into(),
        "" => "unknown".into(),
        "unknown" => "unknown".into(),
        // An unknown scope is always marked unknown, never guessed to be device data.
        _ => "unknown".into(),
    }
}

pub(super) fn explain_coverage(
    cadence: StreamCadence,
    window_days: i64,
    observed: Observed,
) -> CoverageExplanation {
    let observed_days = observed.days.len() as i64;
    let first = observed.days.first().cloned();
    let latest = observed.days.last().cloned();

    if !cadence.has_expected_days() {
        let note = match cadence {
            StreamCadence::PerEvent => {
                "Produced per event: no records means no matching activity in this period, not a gap.".into()
            }
            _ => "The watch reports this only now and then: blank dates are normal and do not mean data loss.".into(),
        };
        return CoverageExplanation {
            kind: "observations".into(),
            window_days,
            observed_days,
            gap_dates: Vec::new(),
            gap_total: 0,
            first_observed_at: first,
            latest_observed_at: latest,
            note,
        };
    }

    // Count gaps only inside the range where data was observed. A blank before the first data means
    // "sync has not started yet". Counting it as missing would show every new user a wall of red.
    let (gap_dates, gap_total) = match (&first, &latest) {
        (Some(first), Some(latest)) => {
            let present: std::collections::HashSet<&str> =
                observed.days.iter().map(String::as_str).collect();
            let mut gaps = Vec::new();
            let mut total = 0i64;
            if let (Some(start), Some(end)) = (parse_day(first), parse_day(latest)) {
                let mut cursor = start;
                while cursor <= end {
                    let key = cursor.format("%Y-%m-%d").to_string();
                    if !present.contains(key.as_str()) {
                        total += 1;
                        if gaps.len() < MAX_REPORTED_GAPS {
                            gaps.push(key);
                        }
                    }
                    cursor += Duration::days(1);
                }
            }
            (gaps, total)
        }
        _ => (Vec::new(), 0),
    };

    let note = if observed_days == 0 {
        "No local data for this period yet. Run a sync first.".into()
    } else if gap_total == 0 {
        "No gaps observed since the first day with data.".into()
    } else {
        format!("Since the first day with data, {gap_total} days have no observed data. A watch not worn, not synced or not returned by the cloud all cause gaps.")
    };

    CoverageExplanation {
        kind: "gaps".into(),
        window_days,
        observed_days,
        gap_dates,
        gap_total,
        first_observed_at: first,
        latest_observed_at: latest,
        note,
    }
}

pub(super) fn parse_day(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

/// Suggest actions only when there is something to do. Do not show "a row of buttons that is always there" as decoration.
pub(super) fn suggested_actions(
    database: &DatabaseHealth,
    timings: &HealthTimings,
    streams: &[StreamHealth],
) -> Vec<HealthAction> {
    let mut actions = Vec::new();
    if streams
        .iter()
        .any(|stream| stream.fetch.error_kind.as_deref() == Some("auth"))
    {
        actions.push(HealthAction {
            id: "reauth".into(),
            code: "reauth".into(),
            label: "Reconnect the Zepp account".into(),
            reason: "Some data streams cannot fetch data because authentication expired.".into(),
            destructive: false,
        });
    }
    if database.pending_normalization > 0
        || database.replay_in_progress
        || database.normalizer_replay_pending
    {
        // State the two reasons separately. "Payloads produced no records" and "records came from old rules"
        // are two different things to the user. One merged sentence makes the second case look like data loss.
        let reason = if database.normalizer_replay_pending
            && database.stored_normalizer_revision.as_deref() != Some(NORMALIZER_REVISION)
        {
            format!(
                "Local derived data was produced by {}, the current parser is {}. Replay does not use the network and does not change the cloud sync time.",
                database
                    .stored_normalizer_revision
                    .as_deref()
                    .unwrap_or("an earlier version"),
                database.normalizer_revision
            )
        } else {
            format!(
                "{} retained payloads are not normalized yet. Replay does not use the network and does not change the cloud sync time.",
                database.pending_normalization
            )
        };
        actions.push(HealthAction {
            id: "reprocess".into(),
            code: "reprocess".into(),
            label: "Replay local payloads with the current parser".into(),
            reason,
            destructive: false,
        });
    }
    if streams.iter().any(|stream| {
        stream.fetch.state == "failed" && stream.fetch.error_kind.as_deref() != Some("auth")
    }) {
        actions.push(HealthAction {
            id: "sync".into(),
            code: "sync_retry".into(),
            label: "Sync again".into(),
            reason: "Last time, some data streams could not fetch data from the cloud.".into(),
            destructive: false,
        });
    }
    if timings.last_cloud_sync_at.is_none() {
        actions.push(HealthAction {
            id: "sync".into(),
            code: "sync_first".into(),
            label: "Run the first sync".into(),
            reason: "This device has no successful cloud sync on record yet.".into(),
            destructive: false,
        });
    }
    actions.push(HealthAction {
        id: "integrity_check".into(),
        code: "integrity_check".into(),
        label: "Check database integrity".into(),
        reason: "Runs SQLite integrity_check on the whole database. A large database takes a moment.".into(),
        destructive: false,
    });
    actions.push(HealthAction {
        id: "open_data_folder".into(),
        code: "open_data_folder".into(),
        label: "Open data folder".into(),
        reason: "The local database, backups and exports are all here.".into(),
        destructive: false,
    });
    actions.dedup_by(|a, b| a.id == b.id);
    actions
}
