//! Drafting and publishing outside the desktop process (MCP `draft_training_plan` / `publish_training_plan`).
//!
//! It uses the same three steps as the desktop `commands/training_plan.rs::publish` (see [`super::publish`]):
//! prepare and record inside the write lock -> release the lock and go online -> record the result inside the write lock. Only the
//! database access differs: there is no long-lived connection. Each step opens a writable connection, takes the cross-process write lock, and releases both after the write. Neither is held while online.
//!
//! Two differences from the desktop are on purpose:
//!
//! - **It never clears a window** (`confirm_clear` is always false). Only the user can confirm a clear in ZeppBridge.
//!   A draft that would clear returns `NeedsClearConfirmation` and changes nothing.
//! - Sync entry point: the caller (MCP) is sync code, so this builds a single-thread runtime for the online step.

use super::publish::{ensure_connected, send_window};
use super::{check_plan, PlanCheck, PlanContext, PlanDocument};
use crate::models::{error::Result, ZeppBridgeError};
use crate::official::{OfficialClient, OfficialStore};
use crate::storage::training_plan::{
    DraftOrigin, DraftPreview, Prepared, PublishRecord, PublishRequest, SendOutcome,
};
use crate::storage::write_lock::{self, WritePurpose};
use crate::storage::{Database, CURRENT_SCHEMA_VERSION};
use chrono::NaiveDate;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

/// The wait limit for a short write lock, the same as the desktop short write.
const LOCK_WAIT: Duration = Duration::from_secs(5);

/// Take the write lock, open a writable connection, do one thing, then release all.
pub fn write_once<T>(data_dir: &Path, mutation: impl FnOnce(&Database) -> Result<T>) -> Result<T> {
    let _guard = write_lock::acquire_with_timeout(data_dir, WritePurpose::Metadata, LOCK_WAIT)
        .map_err(|error| ZeppBridgeError::Busy(error.to_string()))?;
    let db = Database::open_without_migration(data_dir.join("zepp.db"))?;
    let version: i64 = db
        .conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version != CURRENT_SCHEMA_VERSION {
        return Err(ZeppBridgeError::Busy(format!(
            "The local database is v{version}, and this program needs v{CURRENT_SCHEMA_VERSION}. Open ZeppBridge once to finish the upgrade, then try again"
        )));
    }
    mutation(&db)
}

/// The draft result: if validation fails, `draft_id` is empty and nothing is saved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftResult {
    pub draft_id: Option<String>,
    pub check: PlanCheck,
    /// After save, the day-by-day "before send / after send". Empty when nothing is saved.
    pub preview: Option<DraftPreview>,
}

/// Validate and save a plan that MCP drafted. If a blocking issue exists, do not save, and return the issues as is so the AI can fix them.
pub fn draft_plan(
    data_dir: &Path,
    document: &PlanDocument,
    today: NaiveDate,
) -> Result<DraftResult> {
    write_once(data_dir, |db| {
        let check = check_plan(
            document,
            PlanContext {
                today,
                max_hr: db.training_plan_max_hr()?,
            },
        );
        if !check.publishable() {
            return Ok(DraftResult {
                draft_id: None,
                check,
                preview: None,
            });
        }
        let id = db.save_plan_draft(DraftOrigin::Mcp, document)?;
        let preview = db.preview_plan_draft(&id, today)?;
        Ok(DraftResult {
            draft_id: Some(id),
            check,
            preview: Some(preview),
        })
    })
}

/// The publish outcome, with the same shape as the desktop `PublishResult`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StandalonePublish {
    pub outcome: Prepared,
    pub record: Option<PublishRecord>,
    pub records: Vec<PublishRecord>,
}

fn finished(db: &Database, outcome: Prepared) -> Result<StandalonePublish> {
    let (record, records) = match &outcome {
        Prepared::Send { batch_id, .. } => {
            (db.plan_batch_summary(*batch_id)?, db.plan_batch(*batch_id)?)
        }
        _ => (None, Vec::new()),
    };
    Ok(StandalonePublish {
        outcome,
        record,
        records,
    })
}

/// Send a draft to the watch. The caller must first confirm that "allow AI to publish directly" is on.
pub fn publish_draft_blocking(
    data_dir: &Path,
    draft_id: &str,
    today: NaiveDate,
) -> Result<StandalonePublish> {
    let request = PublishRequest::Draft {
        id: draft_id.to_string(),
    };
    // The demo database has no credentials and does not go online: like the desktop, record it as delivered.
    let demo = data_dir.join(".demo-library").is_file()
        && Database::open_read_only(data_dir.join("zepp.db"))?.is_demo_library()?;
    if demo {
        return write_once(data_dir, |db| {
            let outcome = db.prepare_plan_publish(&request, today, false)?;
            if let Prepared::Send { sends, .. } = &outcome {
                for send in sends {
                    db.finish_plan_publish(send.publish_id, &SendOutcome::Delivered)?;
                }
            }
            finished(db, outcome)
        });
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| ZeppBridgeError::ConfigError(format!("Cannot start the network runtime: {error}")))?;
    runtime.block_on(publish_draft(data_dir, request, today))
}

async fn publish_draft(
    data_dir: &Path,
    request: PublishRequest,
    today: NaiveDate,
) -> Result<StandalonePublish> {
    let store = OfficialStore::new(data_dir);
    let client = OfficialClient::new()?;
    // If no usable token exists, do not touch the ledger.
    ensure_connected(&store, &client).await?;
    let prepared = write_once(data_dir, |db| {
        db.prepare_plan_publish(&request, today, false)
    })?;
    let Prepared::Send { sends, .. } = &prepared else {
        return Ok(StandalonePublish {
            outcome: prepared,
            record: None,
            records: Vec::new(),
        });
    };
    // The order and the ledger rules match the desktop: send window by window in ledger order and record each one. If authorization fails, stop
    // sending, and record the remaining windows with the same refusal.
    let mut abort: Option<SendOutcome> = None;
    for send in sends {
        let outcome = match &abort {
            Some(outcome) => outcome.clone(),
            None => send_window(&store, &client, &send.body).await,
        };
        if matches!(&outcome, SendOutcome::Rejected { error_code, .. }
            if error_code == "err.core.needs_reauth")
        {
            abort = Some(outcome.clone());
        }
        write_once(data_dir, |db| {
            db.finish_plan_publish(send.publish_id, &outcome)
        })?;
    }
    if abort.is_some() {
        return Err(ZeppBridgeError::NeedsReauth(
            "Authentication expired. Reconnect Zepp in ZeppBridge".into(),
        ));
    }
    write_once(data_dir, |db| finished(db, prepared.clone()))
}
