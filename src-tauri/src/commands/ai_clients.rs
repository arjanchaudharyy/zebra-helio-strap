//! The "Connect to a local AI" part of the "Hand off to AI tools" card in settings (Claude Code, Codex, Claude Desktop, OpenCode, Grok, Factory, Qwen Code, and others).
//!
//! The installer puts `zeppbridge-mcp` as a sidecar next to `ZeppBridge.exe` (a packaging decision;
//! on macOS it is in `ZeppBridge.app/Contents/MacOS/`, for deb / rpm in `/usr/bin/`, always next to the main program).
//! So the app knows its real path, and the button can give a working command. The user does not have to find it.
//! This module answers only two questions: where the sidecar is, and whether it can find this data by itself. It also writes
//! the Claude Desktop `.mcpb` to the directory that the user picks.

use crate::app_state::AppState;
use crate::ipc_error::AppError;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use zeppbridge_core::paths::write_file_atomically;

/// Name of the MCP program that ships in the installer (no extension). It matches `externalBin` in
/// `tauri.sidecar.conf.json`. Tauri removes the target triple suffix when it packages it.
const SIDECAR_STEM: &str = "zeppbridge-mcp";
/// The command line tool that also ships in the installer. Settings only show its path and do not register it anywhere.
#[cfg(not(all(windows, feature = "store")))]
const CLI_STEM: &str = "zeppbridge-cli";
/// Store aliases have a package-specific name so they can coexist with PATH
/// users of the GitHub edition without sharing the WindowsApps alias entry.
#[cfg(all(windows, feature = "store"))]
const STORE_SIDECAR_ALIAS_STEM: &str = "zeppbridge-store-mcp";
#[cfg(all(windows, feature = "store"))]
const STORE_CLI_ALIAS_STEM: &str = "zeppbridge-store-cli";
/// File name of the written `.mcpb`.
const BUNDLE_FILE: &str = "ZeppBridge.mcpb";

#[derive(Debug, Clone, Serialize)]
pub struct McpSidecar {
    /// Absolute path of the bundled `zeppbridge-mcp`. Empty for dev builds or old installers that do not include it.
    pub path: Option<String>,
    /// By default the MCP reads "the data next to itself". When that is not the database that the app uses
    /// (dev build, `ZEPPBRIDGE_DATA_DIR`, fallback when the install directory is not writable), the config must
    /// set this environment variable explicitly. Otherwise the AI sees an empty database.
    pub data_dir_env: Option<String>,
    /// Local AI clients that registered **another copy** of zeppbridge-mcp (usually an old one from the 2.x toolkit):
    /// `claude_desktop` / `claude_code` / `codex` / `opencode` / `grok` / `factory` / `qwen`. Empty when no sidecar ships. See [`stale`].
    pub stale_clients: Vec<&'static str>,
    /// Clients that registered this copy, but whose read scope does not match the "shared tasks only" switch (the default reads the full database).
    pub scope_stale_clients: Vec<&'static str>,
    /// "Shared tasks only" is on: the generated config includes `--scope task`.
    pub task_only: bool,
    /// The zeppbridge-mcp next to the app is older than the app: while an AI client holds it open, an update cannot replace this file
    /// (a user reported that Claude Desktop kept showing "database v47, program v41").
    pub outdated: bool,
    /// Running from an AppImage (Linux): the sidecar is in the package, but each launch mounts it under a new
    /// `/tmp/.mount_*`, so the path registered in an AI client no longer exists next time. In this case `path` is empty,
    /// and the UI explains why and points to the separate toolkit.
    pub appimage: bool,
    /// Absolute path of the bundled `zeppbridge-cli`. Found apart from the MCP. Empty for old installers, dev builds and AppImage.
    pub cli_path: Option<String>,
}

fn not_bundled(appimage: bool) -> McpSidecar {
    McpSidecar {
        path: None,
        data_dir_env: None,
        stale_clients: Vec::new(),
        scope_stale_clients: Vec::new(),
        task_only: false,
        outdated: false,
        appimage,
        cli_path: None,
    }
}

#[cfg(not(all(windows, feature = "store")))]
pub(crate) fn locate_sidecar(exe_dir: &Path, data_dir: &Path) -> McpSidecar {
    let cli = exe_dir.join(format!("{CLI_STEM}{}", std::env::consts::EXE_SUFFIX));
    let cli_path = cli.is_file().then(|| cli.to_string_lossy().into_owned());
    let candidate = exe_dir.join(format!("{SIDECAR_STEM}{}", std::env::consts::EXE_SUFFIX));
    if !candidate.is_file() {
        return McpSidecar {
            cli_path,
            ..not_bundled(false)
        };
    }
    let data_dir_env =
        (exe_dir.join("data") != data_dir).then(|| data_dir.to_string_lossy().into_owned());
    McpSidecar {
        path: Some(candidate.to_string_lossy().into_owned()),
        data_dir_env,
        stale_clients: Vec::new(),
        scope_stale_clients: Vec::new(),
        task_only: false,
        outdated: false,
        appimage: false,
        cli_path,
    }
}

/// The AppImage runtime sets `APPIMAGE` (the path of the package itself) for the process.
#[cfg(not(all(windows, feature = "store")))]
fn running_from_appimage() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_some()
}

#[cfg(all(windows, feature = "store"))]
fn current_sidecar(data_dir: &Path) -> McpSidecar {
    // Register MCP clients against the package execution alias. A WindowsApps
    // path is stable across Store updates; the package install directory is
    // versioned and changes on each update. Explicitly pass the package-local
    // data directory as a fallback for MCP hosts that start an alias without
    // propagating package identity to the child process.
    let Ok(windows_apps) = zeppbridge_core::paths::windows_apps_alias_dir() else {
        return not_bundled(false);
    };
    let path = windows_apps.join(format!("{STORE_SIDECAR_ALIAS_STEM}.exe"));
    let cli_path = windows_apps.join(format!("{STORE_CLI_ALIAS_STEM}.exe"));
    McpSidecar {
        path: Some(path.to_string_lossy().into_owned()),
        data_dir_env: Some(data_dir.to_string_lossy().into_owned()),
        stale_clients: Vec::new(),
        scope_stale_clients: Vec::new(),
        task_only: false,
        outdated: false,
        appimage: false,
        cli_path: Some(cli_path.to_string_lossy().into_owned()),
    }
}

#[cfg(not(all(windows, feature = "store")))]
fn current_sidecar(data_dir: &Path) -> McpSidecar {
    if running_from_appimage() {
        return not_bundled(true);
    }
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    match exe_dir {
        Some(dir) => locate_sidecar(&dir, data_dir),
        None => not_bundled(false),
    }
}

/// The "shared tasks only" switch. If it cannot be read, use the default "off" (read the full database).
async fn task_only(data_dir: &Path) -> bool {
    super::spawn_independent_read(data_dir.to_path_buf(), |db| db.mcp_task_scope_only())
        .await
        .unwrap_or(false)
}

/// Asks the sidecar for its own version. Older programs do not know `--version` (exit code 2), so they also count as old.
/// If it cannot start (permissions, antivirus block), draw no conclusion.
fn sidecar_outdated(path: &Path) -> bool {
    let mut command = std::process::Command::new(path);
    command
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: the sidecar is a console program. Do not flash a black window in front of the user.
        command.creation_flags(0x0800_0000);
    }
    match command.output() {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout);
            text.trim() != format!("zeppbridge-mcp {}", env!("CARGO_PKG_VERSION"))
        }
        Err(_) => false,
    }
}

#[tauri::command]
pub async fn get_mcp_sidecar(state: tauri::State<'_, AppState>) -> Result<McpSidecar, AppError> {
    let mut sidecar = current_sidecar(&state.data_dir);
    sidecar.task_only = task_only(&state.data_dir).await;
    if let Some(path) = sidecar.path.clone() {
        let task_only = sidecar.task_only;
        // .claude.json can be several MB, so read files and start the sidecar on a blocking thread.
        let (stale, scope_stale, outdated) = tokio::task::spawn_blocking(move || {
            let roots = stale::ClientConfigRoots::from_env();
            let path = Path::new(&path);
            (
                stale::stale_clients(&roots, path),
                stale::scope_stale_clients(&roots, path, task_only),
                sidecar_outdated(path),
            )
        })
        .await
        .unwrap_or_default();
        sidecar.stale_clients = stale;
        sidecar.scope_stale_clients = scope_stale;
        sidecar.outdated = outdated;
    }
    Ok(sidecar)
}

/// The `.mcpb` manifest.
///
/// The package includes a copy of the sidecar so that `entry_point` points to a real file (the spec requires the binary type
/// to include its own executable). But the copy that really starts is the one in the install directory that `command` points to:
/// when the app upgrades, the MCP upgrades with it, and an old program never reads a newer database.
pub(crate) fn bundle_manifest(
    sidecar_path: &str,
    data_dir_env: Option<&str>,
    task_only: bool,
) -> serde_json::Value {
    let entry_point = format!("server/{SIDECAR_STEM}{}", std::env::consts::EXE_SUFFIX);
    // By default, read the full database. Add `--scope task` only when "shared tasks only" is on.
    let args: &[&str] = if task_only { &["--scope", "task"] } else { &[] };
    let mut mcp_config = serde_json::json!({
        "command": sidecar_path,
        "args": args,
    });
    if let Some(dir) = data_dir_env {
        mcp_config["env"] = serde_json::json!({ "ZEPPBRIDGE_DATA_DIR": dir });
    }
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    };
    serde_json::json!({
        "manifest_version": "0.3",
        "name": "zeppbridge",
        "display_name": "ZeppBridge",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Zepp health and training data from the ZeppBridge desktop app, read on this computer.",
        "author": { "name": "ZeppBridge" },
        "homepage": "https://github.com/lingcang728/ZeppBridge",
        "server": {
            "type": "binary",
            "entry_point": entry_point,
            "mcp_config": mcp_config,
        },
        "compatibility": { "platforms": [platform] },
    })
}

#[cfg(test)]
pub(crate) fn build_bundle(
    sidecar: &Path,
    data_dir_env: Option<&str>,
    task_only: bool,
) -> Result<Vec<u8>, AppError> {
    build_bundle_with_command(sidecar, &sidecar.to_string_lossy(), data_dir_env, task_only)
}

fn build_bundle_with_command(
    sidecar: &Path,
    command: &str,
    data_dir_env: Option<&str>,
    task_only: bool,
) -> Result<Vec<u8>, AppError> {
    let failed = |detail: String| {
        AppError::new(
            "err.mcp.bundle_failed",
            format!("Failed to build the Claude Desktop extension package: {detail}"),
        )
    };
    let manifest = bundle_manifest(command, data_dir_env, task_only);
    let binary = std::fs::read(sidecar).map_err(|error| failed(error.to_string()))?;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|error| failed(error.to_string()))?;
    let entry_point = manifest["server"]["entry_point"]
        .as_str()
        .unwrap_or_default();
    for (name, bytes) in [
        ("manifest.json", manifest_bytes.as_slice()),
        (entry_point, binary.as_slice()),
    ] {
        writer
            .start_file(name, options)
            .and_then(|()| writer.write_all(bytes).map_err(Into::into))
            .map_err(|error| failed(error.to_string()))?;
    }
    let cursor = writer.finish().map_err(|error| failed(error.to_string()))?;
    Ok(cursor.into_inner())
}

/// Writes the `.mcpb` into `directory` and returns the full path (the UI uses it to select the file in File Explorer).
#[tauri::command]
pub async fn save_mcp_bundle(
    state: tauri::State<'_, AppState>,
    directory: String,
) -> Result<String, AppError> {
    let directory = PathBuf::from(directory.trim());
    if directory.as_os_str().is_empty() || !directory.is_absolute() || !directory.is_dir() {
        return Err(AppError::new(
            "err.export.path_not_absolute",
            "The save location must be an absolute path",
        ));
    }
    let sidecar = current_sidecar(&state.data_dir);
    let Some(command_path) = sidecar.path else {
        return Err(AppError::new(
            "err.mcp.sidecar_missing",
            "This version does not include zeppbridge-mcp",
        ));
    };
    #[cfg(all(windows, feature = "store"))]
    let payload_path = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .map(|dir| dir.join(format!("{SIDECAR_STEM}.exe")))
        .filter(|path| path.is_file());
    #[cfg(not(all(windows, feature = "store")))]
    let payload_path = Some(PathBuf::from(command_path.clone()));
    let Some(payload_path) = payload_path else {
        return Err(AppError::new(
            "err.mcp.sidecar_missing",
            "This version does not include zeppbridge-mcp",
        ));
    };
    let task_only = task_only(&state.data_dir).await;
    let bytes = tokio::task::spawn_blocking(move || {
        build_bundle_with_command(
            &payload_path,
            &command_path,
            sidecar.data_dir_env.as_deref(),
            task_only,
        )
    })
    .await
    .map_err(|_| AppError::new("err.mcp.bundle_failed", "Failed to build the Claude Desktop extension package"))??;
    let target = directory.join(BUNDLE_FILE);
    write_file_atomically(&target, &bytes).map_err(|error| {
        AppError::new(
            "err.mcp.bundle_failed",
            format!("Failed to build the Claude Desktop extension package: {error}"),
        )
    })?;
    Ok(target.to_string_lossy().into_owned())
}

mod stale;

#[cfg(test)]
mod tests;
