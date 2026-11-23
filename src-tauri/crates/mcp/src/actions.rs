//! Two actions for the local AI: start one cloud sync, and export a range of days to a file.
//!
//! They do not change records the user wrote, so they do not go into the change ledger.
//!
//! - **Sync** uses the same path as `zeppbridge-cli sync` in scheduled tasks: it starts the CLI next to this binary directly.
//!   The CLI handles the write lock, sync lease, cancel and cloud probe; MCP adds no network or async runtime. When a local reparse is still pending, it
//!   returns `deferred` (so the desktop app aligns derived data first). That is not a failure. Client tool timeouts are often 60 seconds,
//!   so it waits at most [`SYNC_WAIT`]. If the sync is not done by then, it returns `running` and the CLI finishes on its own.
//! - **Export** uses the same builder as the desktop app and CLI (`build_ai_export`). It writes only a summary (no tracks)
//!   into `exports\` in the data directory, and returns only the file name relative to the data directory and the counts (contract: no local absolute paths).

use super::*;
use std::process::{Child, Command, Stdio};
use std::time::{Duration as StdDuration, Instant};
use zeppbridge_core::models::{ExportDetail, ExportScope, ExportSelection};

pub(super) const ACTION_TOOLS: [&str; 2] = ["sync_now", "export_data"];

const SYNC_WAIT: StdDuration = StdDuration::from_secs(45);

fn cli_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe
        .parent()?
        .join(format!("zeppbridge-cli{}", std::env::consts::EXE_SUFFIX));
    path.is_file().then_some(path)
}

/// CLI exit code contract (`docs/reference/cli-and-mcp.md`) -> status for the AI.
fn sync_state(code: Option<i32>) -> &'static str {
    match code {
        Some(0) => "done",
        Some(3) => "not_configured",
        Some(4) => "busy",
        Some(5) => "cloud_error",
        Some(7) => "schema_mismatch",
        Some(8) => "incomplete",
        _ => "failed",
    }
}

pub(super) fn run_sync(db: &Database) -> Result<Value, CallFailure> {
    if db
        .pending_replay_plan()
        .ok()
        .flatten()
        .is_some_and(|plan| plan.raw_records > 0)
    {
        return Ok(json!({
            "state": "deferred",
            "note": "A reparse is still pending on this computer (after a parser upgrade, local payloads must be recomputed). Ask the user to open ZeppBridge once and wait for it to finish, then sync; this is not a failure.",
        }));
    }
    let Some(cli) = cli_path() else {
        return Err(CallFailure::coded(
            "err.mcp.cli_missing",
            "There is no zeppbridge-cli next to this zeppbridge-mcp, so sync cannot start. Ask the user to click sync in ZeppBridge.".into(),
        ));
    };
    let mut command = Command::new(cli);
    command
        .args(["sync", "--json", "--no-reprocess"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: MCP may run under a GUI client with no console. Do not open a black window.
        command.creation_flags(0x0800_0000);
    }
    let child = command
        .spawn()
        .map_err(|error| CallFailure::plain(format!("Failed to start zeppbridge-cli: {error}")))?;
    wait_for_sync(child, SYNC_WAIT)
}

fn wait_for_sync(mut child: Child, wait: StdDuration) -> Result<Value, CallFailure> {
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let report_reader = std::thread::spawn(move || {
        use std::io::Read as _;
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).ok()?;
        serde_json::from_slice::<Value>(&bytes).ok()
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let report = report_reader.join().ok().flatten().unwrap_or(Value::Null);
                return Ok(json!({
                    "state": sync_state(status.code()),
                    "exitCode": status.code(),
                    "report": report,
                    "note": "Sync pulls data the cloud already has; if the watch has not sent data to the Zepp App yet, it is normal that the cloud has nothing new.",
                }));
            }
            Ok(None) if started.elapsed() < wait => {
                std::thread::sleep(
                    StdDuration::from_millis(250).min(wait.saturating_sub(started.elapsed())),
                );
            }
            other => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                    let _ = report_reader.join();
                });
                return match other {
                    Ok(_) => Ok(json!({
                        "state": "running",
                        "note": "Sync is still running in the background (a long history can take a few minutes). Later, use get_data_health to see the time and result of the last sync. Do not start it again.",
                    })),
                    Err(error) => Err(CallFailure::plain(format!("Failed to wait for the sync result: {error}"))),
                };
            }
        }
    }
}

pub(super) fn run_export(db: &Database, args: &Value) -> Result<Value, CallFailure> {
    let (start, end) = match date_args(args).map_err(CallFailure::plain)? {
        (Some(start), Some(end)) => (start, end),
        _ => {
            return Err(CallFailure::plain(
                "startDate and endDate are both required (YYYY-MM-DD, at most 366 days)",
            ))
        }
    };
    let start = start.format("%Y-%m-%d").to_string();
    let end = end.format("%Y-%m-%d").to_string();
    let scope = ExportScope::date_range(&start, &end)
        .validated()
        .map_err(CallFailure::plain)?;
    let types: Vec<String> = match args.get("types") {
        None | Some(Value::Null) => ["workouts", "daily_activity", "sleep"]
            .map(str::to_string)
            .to_vec(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_ascii_lowercase)
            .collect(),
        Some(_) => return Err(CallFailure::plain("types is an array of data types")),
    };
    let unknown: Vec<&str> = types
        .iter()
        .map(String::as_str)
        .filter(|kind| !zeppbridge_core::storage::EXPORT_DATA_TYPES.contains(kind))
        .collect();
    if types.is_empty() || !unknown.is_empty() {
        return Err(CallFailure::plain(format!(
            "types can only be chosen from: {}",
            zeppbridge_core::storage::EXPORT_DATA_TYPES.join(", ")
        )));
    }
    let selection = ExportSelection {
        scope: Some(scope),
        start_date: None,
        end_date: None,
        data_types: types.clone(),
        detail: ExportDetail::Summary,
    };
    let (text, records) = db
        .build_ai_export(&selection)
        .map_err(|error| CallFailure::coded(error.code(), error.user_message()))?;
    let dir = db
        .data_dir()
        .ok_or_else(|| CallFailure::plain("Cannot find the local data directory, so the export file cannot be written."))?
        .join("exports");
    std::fs::create_dir_all(&dir)
        .map_err(|error| CallFailure::plain(format!("Cannot create the export directory: {error}")))?;
    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let name = format!("zeppbridge-{start}_{end}-{stamp}.json");
    paths::write_file_atomically(&dir.join(&name), text.as_bytes())
        .map_err(|error| CallFailure::plain(format!("Failed to write the export file: {error}")))?;
    // Contract: no local absolute paths. Give only the position relative to the data directory; the user sees it after opening the data directory on the desktop.
    Ok(json!({
        "file": format!("exports/{name}"),
        "records": records,
        "bytes": text.len(),
        "types": types,
        "range": { "start": start, "end": end },
        "note": "The file is in the exports folder of the ZeppBridge data directory on the user's computer (the desktop settings can open the data directory). This is a summary export (no workout tracks). For specific data, use the read tools directly; you do not need to read this file.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT_MARKER: &str = "ZEPPBRIDGE_SYNC_FIXTURE_MARKER";

    #[test]
    #[ignore = "spawned by the sync pipe regression test"]
    fn sync_output_fixture() {
        if let Some(path) = std::env::var_os(OUTPUT_MARKER) {
            std::thread::sleep(StdDuration::from_millis(200));
            println!("{}", "x".repeat(512 * 1024));
            std::fs::write(path, b"finished").unwrap();
        }
    }

    #[test]
    fn sync_drains_large_output_before_and_after_returning_running() {
        for (timeout, expected) in [
            (StdDuration::from_secs(5), "done"),
            (StdDuration::ZERO, "running"),
        ] {
            let marker = std::env::temp_dir().join(format!(
                "zb-sync-{}-{}",
                std::process::id(),
                zeppbridge_core::unique_suffix(),
            ));
            let child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "actions::tests::sync_output_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .stdout(Stdio::piped())
                .env(OUTPUT_MARKER, &marker)
                .spawn()
                .unwrap();
            let report =
                wait_for_sync(child, timeout).unwrap_or_else(|_| panic!("sync wait failed"));
            assert_eq!(report["state"], expected);
            let deadline = Instant::now() + StdDuration::from_secs(5);
            while !marker.is_file() && Instant::now() < deadline {
                std::thread::sleep(StdDuration::from_millis(20));
            }
            assert!(marker.is_file());
            std::fs::remove_file(marker).unwrap();
        }
    }
}
