//! Export scope and selection, and results for AI (split from models/types.rs by domain; shapes unchanged).

use super::*;

/// How much of each stream an export carries.
///
/// The per-second workout series and per-minute heart rate are 99% of an
/// export's bytes; a 30-day `Full` export is ~9 MB, which no model will read.
/// `Summary` aggregates those two and keeps every structured metric intact, so
/// the same window fits in a context window. `Full` stays available for
/// archival and is what the CSV/GPX converters always use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportDetail {
    #[default]
    Summary,
    Full,
}

impl ExportDetail {
    pub fn is_full(self) -> bool {
        matches!(self, ExportDetail::Full)
    }
}

/// What one export covers.
///
/// The two variants are exclusive; there is no "which wins if both are sent". A request
/// with both a date range and a workout id is unclear, so it is rejected, not resolved.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ExportScope {
    DateRange {
        start: String,
        end: String,
    },
    // rename_all on the enum only renames variants, not their fields. So mark it again
    // here, or the frontend's `workoutId` is read as a missing field.
    #[serde(rename_all = "camelCase")]
    Workout {
        workout_id: String,
    },
}

/// The maximum span of one export. For history beyond 365 days, use a database snapshot,
/// not one JSON for AI.
pub const MAX_EXPORT_RANGE_DAYS: i64 = 365;

impl ExportScope {
    pub fn date_range(start: impl Into<String>, end: impl Into<String>) -> Self {
        ExportScope::DateRange {
            start: start.into(),
            end: end.into(),
        }
    }

    /// Validates and normalizes. Dates must be `YYYY-MM-DD`, the end cannot be before the
    /// start, the span has a limit, and the workout id cannot be empty.
    pub fn validated(&self) -> std::result::Result<ExportScope, String> {
        match self {
            ExportScope::DateRange { start, end } => {
                let parsed_start = chrono::NaiveDate::parse_from_str(start.trim(), "%Y-%m-%d")
                    .map_err(|_| "Invalid export start date".to_string())?;
                let parsed_end = chrono::NaiveDate::parse_from_str(end.trim(), "%Y-%m-%d")
                    .map_err(|_| "Invalid export end date".to_string())?;
                if parsed_end < parsed_start {
                    return Err("Export end date cannot be before the start date".into());
                }
                if (parsed_end - parsed_start).num_days() > MAX_EXPORT_RANGE_DAYS {
                    return Err("A single export cannot span more than 366 days".into());
                }
                Ok(ExportScope::DateRange {
                    start: parsed_start.format("%Y-%m-%d").to_string(),
                    end: parsed_end.format("%Y-%m-%d").to_string(),
                })
            }
            ExportScope::Workout { workout_id } => {
                let trimmed = workout_id.trim();
                if trimmed.is_empty() {
                    return Err("Workout id cannot be empty".into());
                }
                Ok(ExportScope::Workout {
                    workout_id: trimmed.to_string(),
                })
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSelection {
    /// New callers send this.
    #[serde(default)]
    pub scope: Option<ExportScope>,
    /// The date range from old callers. Kept for compatibility; it always becomes an `ExportScope` first.
    #[serde(default)]
    pub start_date: Option<String>,
    #[serde(default)]
    pub end_date: Option<String>,
    pub data_types: Vec<String>,
    /// Absent means `Summary`; older callers keep working.
    #[serde(default)]
    pub detail: ExportDetail,
}

impl ExportSelection {
    /// Turns the new and old forms into one scope.
    ///
    /// Both `scope` and `startDate/endDate` is a conflicting request. It is an error, with no
    /// priority rule. A priority rule invites the bug "I sent a workoutId, so I expected
    /// only that one workout".
    pub fn resolve_scope(&self) -> std::result::Result<ExportScope, String> {
        let legacy = match (self.start_date.as_deref(), self.end_date.as_deref()) {
            (Some(start), Some(end)) => Some(ExportScope::date_range(start, end)),
            (None, None) => None,
            _ => return Err("An export date range needs both a start and an end".into()),
        };
        match (self.scope.as_ref(), legacy) {
            (Some(_), Some(_)) => {
                Err("Choose one export scope: a date range or one workout, not both".into())
            }
            (Some(scope), None) => scope.validated(),
            (None, Some(scope)) => scope.validated(),
            (None, None) => Err("The export request has no scope: it needs a date range or a workout id".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportResult {
    pub path: String,
    pub record_count: usize,
    pub bytes: usize,
    pub generated_at: String,
    /// How many files this export wrote.
    ///
    /// Only FIT sets it. A FIT activity file holds one activity, so a date range export
    /// writes many files in one folder, and `path` points at that folder. Other formats
    /// write one file, so this is `None` and the UI does not say "1 file in total".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHandoffMetadata {
    pub precise_route_included: bool,
    pub authentication_fields_removed: bool,
    pub identity_fields_removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHandoffResult {
    pub mode: String,
    pub clipboard_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    pub bytes: usize,
    pub records: usize,
    pub redactions: Vec<String>,
    pub metadata: AiHandoffMetadata,
}

#[cfg(test)]
mod export_scope_tests {
    use super::*;

    fn selection(
        scope: Option<ExportScope>,
        start: Option<&str>,
        end: Option<&str>,
    ) -> ExportSelection {
        ExportSelection {
            scope,
            start_date: start.map(str::to_string),
            end_date: end.map(str::to_string),
            data_types: vec!["workouts".into()],
            detail: ExportDetail::default(),
        }
    }

    #[test]
    fn a_request_that_names_both_a_range_and_a_workout_is_refused() {
        // This is a conflicting request. A priority rule invites the bug "I sent a
        // workoutId, so I expected only that one workout", so it is rejected.
        let both = selection(
            Some(ExportScope::Workout {
                workout_id: "run-1".into(),
            }),
            Some("2026-08-01"),
            Some("2026-08-07"),
        );
        let error = both.resolve_scope().unwrap_err();
        assert!(error.contains("Choose one"), "{error}");
    }

    #[test]
    fn a_request_with_no_range_at_all_is_refused() {
        let error = selection(None, None, None).resolve_scope().unwrap_err();
        assert!(error.contains("has no scope"), "{error}");
    }

    #[test]
    fn half_a_legacy_range_is_refused_rather_than_guessed() {
        assert!(selection(None, Some("2026-08-01"), None)
            .resolve_scope()
            .is_err());
        assert!(selection(None, None, Some("2026-08-07"))
            .resolve_scope()
            .is_err());
    }

    #[test]
    fn legacy_date_fields_still_work_on_their_own() {
        let resolved = selection(None, Some("2026-08-01"), Some("2026-08-07"))
            .resolve_scope()
            .unwrap();
        assert_eq!(
            resolved,
            ExportScope::date_range("2026-08-01", "2026-08-07")
        );
    }

    #[test]
    fn a_reversed_range_is_refused() {
        let error = ExportScope::date_range("2026-08-07", "2026-08-01")
            .validated()
            .unwrap_err();
        assert!(error.contains("cannot be before"), "{error}");
    }

    #[test]
    fn an_oversized_range_is_refused_at_the_documented_boundary() {
        // A difference of exactly 365 days (366 days with both ends) is allowed; one more day is rejected.
        assert!(ExportScope::date_range("2025-08-01", "2026-08-01")
            .validated()
            .is_ok());
        assert!(ExportScope::date_range("2025-08-01", "2026-08-02")
            .validated()
            .is_err());
    }

    #[test]
    fn a_malformed_date_is_refused_rather_than_silently_clamped() {
        assert!(ExportScope::date_range("not-a-date", "2026-08-01")
            .validated()
            .is_err());
        assert!(ExportScope::date_range("2026-08-01", "2026-13-45")
            .validated()
            .is_err());
    }

    #[test]
    fn an_empty_workout_id_is_refused() {
        let error = ExportScope::Workout {
            workout_id: "   ".into(),
        }
        .validated()
        .unwrap_err();
        assert!(error.contains("cannot be empty"), "{error}");
    }

    #[test]
    fn scope_round_trips_through_the_ipc_shape_the_frontend_sends() {
        let workout: ExportScope =
            serde_json::from_str(r#"{"kind":"workout","workoutId":"run-1"}"#).unwrap();
        assert_eq!(
            workout,
            ExportScope::Workout {
                workout_id: "run-1".into()
            }
        );
        let range: ExportScope =
            serde_json::from_str(r#"{"kind":"dateRange","start":"2026-08-01","end":"2026-08-07"}"#)
                .unwrap();
        assert_eq!(range, ExportScope::date_range("2026-08-01", "2026-08-07"));
    }
}
