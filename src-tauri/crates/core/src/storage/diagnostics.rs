//! Read-only queries for the diagnostics report (split out of storage/mod.rs by domain, logic unchanged).

use super::*;

impl Database {
    pub fn diagnostic_schema_version(&self) -> Result<i64> {
        self.conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(Into::into)
    }

    /// The latest "HTTP 200, but the cloud says it failed".
    ///
    /// Reads only three things: which stream, which code, and when. **It does not read the cloud's message text**:
    /// that is free text from the server and can contain account details, and this report
    /// promises the user to send only allowlisted fields. One integer is enough to pin down "what expired credentials look like".
    ///
    /// Why it is needed: `classify_business_code` currently maps every code other than 1 to
    /// `CloudRejected`, and does not yet dare to call it "needs a new login", because all 1075 payloads kept
    /// on this machine have `code = 1` and no failure code has been seen. Until a real failure code arrives,
    /// sending the user back to the QR login would trade a certain bad experience for a guess.
    /// (Matching real feedback: D1 `c1f03eb2` "All my readings are showing empty".)
    pub fn diagnostic_cloud_rejection(&self) -> Result<Option<DiagnosticCloudRejection>> {
        let mut stmt = self.conn.prepare(
            "SELECT stream, last_error_code,
                    COALESCE(last_fetch_error_at, last_parse_error_at, last_write_error_at)
             FROM stream_provenance
             WHERE last_error_code IS NOT NULL
               AND 'cloud_rejected' IN (
                   COALESCE(last_fetch_error_kind, ''),
                   COALESCE(last_parse_error_kind, ''),
                   COALESCE(last_write_error_kind, '')
               )
             ORDER BY updated_at DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map([], |row| {
            Ok(DiagnosticCloudRejection {
                stream: row.get(0)?,
                code: row.get(1)?,
                at: row.get(2)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn diagnostic_unknown_workout_codes(&self) -> Result<Vec<DiagnosticWorkoutCode>> {
        let mut stmt = self.conn.prepare(
            "SELECT zepp_type, COUNT(*)
             FROM workouts
             WHERE workout_type_source = 'unknown_code' AND zepp_type IS NOT NULL
             GROUP BY zepp_type ORDER BY zepp_type",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DiagnosticWorkoutCode {
                code: row.get(0)?,
                records: row.get(1)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Workout type corrections the user made, grouped as "code -> our reading -> the user's reading".
    ///
    /// Only rows with a `zepp_type`: a correction without the original code does not help fill the catalog, and
    /// filling the catalog is the only reason this report exists. Grouping by triple instead of listing records
    /// keeps the report size tied to "how many kinds of error" and not to "how many rows the user changed".
    pub fn diagnostic_workout_type_corrections(&self) -> Result<Vec<DiagnosticWorkoutCorrection>> {
        let mut stmt = self.conn.prepare(
            "SELECT zepp_type, workout_type, workout_type_override, COUNT(*)
             FROM workouts
             WHERE workout_type_override IS NOT NULL
               AND zepp_type IS NOT NULL
               AND workout_type_override <> workout_type
             GROUP BY zepp_type, workout_type, workout_type_override
             ORDER BY COUNT(*) DESC, zepp_type",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DiagnosticWorkoutCorrection {
                code: row.get(0)?,
                interpreted: row.get(1)?,
                corrected: row.get(2)?,
                records: row.get(3)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    pub fn diagnostic_workout_type_conflicts(&self) -> Result<i64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM workouts WHERE workout_type_conflict IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }
}
