//! The ZeppBridge shared core.
//!
//! The desktop app (Tauri), the CLI, the MCP server and the local REST API are only
//! adapters over this layer. The data model, SQLite schema and migrations,
//! normalization, query semantics, export, insights and write coordination are
//! implemented here once. No adapter may copy SQL, unit conversion or missing-value rules.

pub mod access;
pub mod ai_tasks;
pub mod auth;
pub mod connectors;
pub mod contract;
pub mod decoder;
pub mod device_catalog;
pub mod event_days;
pub mod export_fit;
pub mod export_formats;
pub mod export_unstrap;
pub mod fetcher;
pub mod import;
pub mod insight;
pub mod letters;
pub mod local_api;
pub mod models;
pub mod normalizer;
pub mod official;
pub mod paths;
pub mod push;
pub mod records;
pub mod redact;
pub mod sport_catalog;
pub mod sync;
pub mod training_plan;
pub mod workout_climbs;

pub mod storage;

/// A unique suffix for temp file and temp directory names: a nanosecond timestamp plus an
/// in-process sequence number. The caller adds the process ID.
///
/// A timestamp alone is not enough. The macOS clock is only accurate to the microsecond, so
/// two parallel tests can get the same name, and `create_dir` then fails with AlreadyExists.
/// Temp files have the same problem: two threads with the same name overwrite each other.
/// The sequence number prevents repeats in one process. The timestamp makes a clash with
/// leftovers from an earlier run almost impossible.
pub fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("{nanos}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}
