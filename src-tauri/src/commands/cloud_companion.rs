//! Beta Labs > "Cloud companion" (off by default).
//!
//! ZeppBridge Cloud is a component that the user **installs separately** (a separate repository). It calls the
//! read-only export of the bundled `zeppbridge-cli`, encrypts it and uploads it to the user's own site, for cloud assistants that cannot read local files.
//! The upload code is not in the main app, and this boundary does not change. This module does only four things:
//!
//! - Read its state folder (`%LOCALAPPDATA%\ZeppBridgeCloud`): config, last run result, issued read-only keys;
//! - Change its `config.json` (only known fields, keep the rest as is; if validation fails, write nothing);
//! - Start its own scripts (`run.ps1` uploads once, `setup.ps1` manages read-only keys);
//! - Open its site, logs and folder.
//!
//! Keys never go through the main app: the component reads read-only keys from the credential manager and puts them on the clipboard itself.
//! This module gets only names and creation times. Users who did not turn on the Labs switch have no UI entry that calls this module.

use crate::ipc_error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use zeppbridge_core::paths::write_file_atomically;

const STATE_DIR_NAME: &str = "ZeppBridgeCloud";
/// The two scheduled tasks that the component registers (see its `setup.ps1 task-commands`).
const TASK_FOLLOW: &str = "ZeppBridge Cloud Follow";
const TASK_UPLOAD: &str = "ZeppBridge Cloud Upload";
/// The component's own defaults (DEFAULTS in `uploader/lib/config.mjs`).
const DEFAULT_RETENTION_DAYS: u32 = 90;
const DEFAULT_SUMMARY_DAYS: u32 = 30;
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const SETUP_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const QUERY_TIMEOUT: Duration = Duration::from_secs(15);
/// Maximum number of characters of the last run log line that we send to the UI.
const LOG_LINE_MAX: usize = 400;

/// Run only one component script at a time: upload and key issue both change the same state.
static BUSY: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CloudConfig {
    /// The value as the user typed it (a domain or a full https URL).
    pub site: String,
    pub bin_dir: Option<String>,
    pub data_dir: Option<String>,
    pub sync: bool,
    pub retention_days: u32,
    pub summary_days: u32,
    pub full_resolution_workouts: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloudLastRun {
    pub at: String,
    /// `upload` (cleanup run / manual) or `follow` (follows sync).
    pub mode: String,
    /// Stable result code from the component: `ok` / `skipped_unchanged` / `skipped_busy` / `skipped_locked` /
    /// `config_error` / `version_mismatch` / `failed`. The UI chooses its text by this code.
    pub result: String,
    pub exit_code: i64,
    /// The raw log line that the component wrote (only counts, sizes and reasons, no data or keys). Show it only as raw diagnostic text.
    pub log_line: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloudReader {
    pub label: String,
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CloudCompanionState {
    /// The component exists only for Windows (PowerShell + Credential Manager).
    pub supported: bool,
    /// The state folder has a readable `config.json`.
    pub configured: bool,
    /// `config.json` exists but is not a valid JSON object.
    pub config_unreadable: bool,
    pub state_dir: Option<String>,
    /// The component's `uploader` folder (it holds run.ps1 / setup.ps1 / upload.mjs). If not found, the UI asks the user to select it.
    pub uploader_dir: Option<String>,
    pub config: Option<CloudConfig>,
    /// The site URL computed with the same rules as the component (`https://health.example.com`).
    pub site_url: Option<String>,
    pub last_run: Option<CloudLastRun>,
    pub readers: Vec<CloudReader>,
    pub task_follow: bool,
    pub task_upload: bool,
    pub running: bool,
    /// Set when this ZeppBridge install folder has zeppbridge-cli. The UI uses it as the default binDir.
    pub default_bin_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CloudConfigInput {
    pub site: String,
    pub bin_dir: Option<String>,
    pub data_dir: Option<String>,
    pub sync: bool,
    pub retention_days: u32,
    pub summary_days: u32,
    pub full_resolution_workouts: bool,
    pub uploader_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloudSetupOutcome {
    pub state: CloudCompanionState,
    /// Only `task-commands` has this: two commands that register scheduled tasks. The UI puts them on the clipboard, and the user reviews and runs them.
    pub output: Option<String>,
}

fn unsupported() -> AppError {
    AppError::new("err.cloud.unsupported", "The cloud companion is available only on Windows")
}

/// `%LOCALAPPDATA%\ZeppBridgeCloud`. Other platforms do not have this variable or this component.
fn state_dir() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    std::env::var_os("LOCALAPPDATA")
        .filter(|value| !value.is_empty())
        .map(|value| PathBuf::from(value).join(STATE_DIR_NAME))
}

/* ------------------------------ Site URL ------------------------------ */

fn looks_like_ip(host: &str) -> bool {
    host.starts_with('[') || host.parse::<std::net::Ipv4Addr>().is_ok()
}

/// Same rules as the component's `uploader/lib/site.mjs`: a bare domain becomes `https://health.<domain>`,
/// a full https URL is used as is. Reject http, user and password, query, fragment and bare IP. Return the site URL without a trailing slash.
pub(crate) fn normalize_site(input: &str) -> Option<String> {
    let raw = input.trim();
    if raw.is_empty()
        || raw.chars().any(char::is_whitespace)
        || raw.contains('?')
        || raw.contains('#')
    {
        return None;
    }
    let url = if raw.contains("://") {
        tauri::Url::parse(raw).ok()?
    } else {
        if raw.contains(['/', '@', '\\']) {
            return None;
        }
        let mut host = raw.to_ascii_lowercase();
        if host.ends_with('.') {
            host.pop();
        }
        let name = host.split(':').next().unwrap_or_default();
        if looks_like_ip(name) || !name.contains('.') {
            return None;
        }
        if !name.starts_with("health.") {
            host = format!("health.{host}");
        }
        tauri::Url::parse(&format!("https://{host}")).ok()?
    };
    let host = url.host_str()?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || looks_like_ip(host)
        || !host.contains('.')
    {
        return None;
    }
    let authority = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix("/v1").unwrap_or(path);
    Some(format!("https://{authority}{path}"))
}

/* ------------------------------ Read the state folder ------------------------------ */

pub(crate) enum ConfigFile {
    Missing,
    Unreadable,
    Found(Map<String, Value>),
}

pub(crate) fn read_config_file(state_dir: &Path) -> ConfigFile {
    let Ok(text) = std::fs::read_to_string(state_dir.join("config.json")) else {
        return ConfigFile::Missing;
    };
    match serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(map)) => ConfigFile::Found(map),
        _ => ConfigFile::Unreadable,
    }
}

fn text_field(map: &Map<String, Value>, name: &str) -> Option<String> {
    map.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn days_field(map: &Map<String, Value>, name: &str, fallback: u32) -> u32 {
    map.get(name)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| (1..=365).contains(value))
        .unwrap_or(fallback)
}

fn config_view(map: &Map<String, Value>) -> CloudConfig {
    let retention_days = days_field(map, "retentionDays", DEFAULT_RETENTION_DAYS);
    CloudConfig {
        // The component also accepts the old field `endpoint`.
        site: text_field(map, "site")
            .or_else(|| text_field(map, "endpoint"))
            .unwrap_or_default(),
        bin_dir: text_field(map, "binDir"),
        data_dir: text_field(map, "dataDir"),
        sync: map.get("sync").and_then(Value::as_bool).unwrap_or(false),
        retention_days,
        summary_days: days_field(map, "summaryDays", DEFAULT_SUMMARY_DAYS).min(retention_days),
        full_resolution_workouts: map
            .get("fullResolutionWorkouts")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn read_last_run(state_dir: &Path) -> Option<CloudLastRun> {
    let text = std::fs::read_to_string(state_dir.join("last-run.json")).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    Some(CloudLastRun {
        at: value.get("at")?.as_str()?.to_string(),
        mode: value
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("upload")
            .to_string(),
        result: value.get("result")?.as_str()?.to_string(),
        exit_code: value.get("exit_code").and_then(Value::as_i64).unwrap_or(0),
        log_line: value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .chars()
            .take(LOG_LINE_MAX)
            .collect(),
    })
}

/// `readers.json` stores only names, key hashes and creation times. The UI does not use the hashes, so we do not send them.
fn read_readers(state_dir: &Path) -> Vec<CloudReader> {
    let Ok(text) = std::fs::read_to_string(state_dir.join("readers.json")) else {
        return Vec::new();
    };
    let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            Some(CloudReader {
                label: item.get("label")?.as_str()?.to_string(),
                created_at: item
                    .get("created_at")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

/* ------------------------------ Find the component ------------------------------ */

pub(crate) fn is_uploader_dir(dir: &Path) -> bool {
    dir.is_absolute()
        && ["run.ps1", "setup.ps1", "upload.mjs"]
            .iter()
            .all(|name| dir.join(name).is_file())
}

/// Find a quoted `…\<script>` path in a text and return its folder.
/// The scheduled task XML (`-File "…\run.ps1"`, quotes can be `&quot;`) and the component's generated
/// `open-dashboard.ps1` (`-File '…\setup.ps1'`) both have this shape.
pub(crate) fn script_dir_in(text: &str, script: &str) -> Option<PathBuf> {
    let text = text.replace("&quot;", "\"");
    let end = text.find(&format!("\\{script}"))?;
    let start = text[..end].rfind(['"', '\''])? + 1;
    let dir = text[start..end].trim();
    (!dir.is_empty()).then(|| PathBuf::from(dir))
}

fn locate_uploader(
    state_dir: &Path,
    config: Option<&Map<String, Value>>,
    follow_task_xml: Option<&str>,
) -> Option<PathBuf> {
    let from_config = config
        .and_then(|map| text_field(map, "uploaderDir"))
        .map(PathBuf::from);
    let from_task = follow_task_xml.and_then(|xml| script_dir_in(xml, "run.ps1"));
    let from_launcher = std::fs::read_to_string(state_dir.join("open-dashboard.ps1"))
        .ok()
        .and_then(|text| script_dir_in(&text, "setup.ps1"));
    [from_config, from_task, from_launcher]
        .into_iter()
        .flatten()
        .find(|dir| is_uploader_dir(dir))
}

/* ------------------------------ Child process ------------------------------ */

struct Finished {
    code: Option<i32>,
    stdout: String,
}

enum RunError {
    Spawn,
    TimedOut,
}

/// Run a program with no window, and stop it at the time limit. Standard output is short (scheduled task XML, two commands), so a separate thread reads it.
fn run_hidden(program: &str, args: &[OsString], timeout: Duration) -> Result<Finished, RunError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: do not flash a black window in front of the user or take focus.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|_| RunError::Spawn)?;
    let reader = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            String::from_utf8_lossy(&bytes).into_owned()
        })
    });
    let deadline = Instant::now() + timeout;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(120)),
            Err(_) => return Err(RunError::Spawn),
        }
    };
    let stdout = reader
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    Ok(Finished { code, stdout })
}

/// Open a folder in File Explorer. The component exists only for Windows, so other platforms do not get here (`state_dir` is None).
fn open_folder(path: &Path) -> std::io::Result<()> {
    if cfg!(windows) {
        Command::new("explorer").arg(path).spawn().map(|_| ())
    } else {
        Err(std::io::ErrorKind::Unsupported.into())
    }
}

/// Does the scheduled task exist? If yes, return its XML (it holds the run.ps1 location).
fn query_task(name: &str) -> Option<String> {
    let args = ["/Query", "/TN", name, "/XML"].map(OsString::from);
    match run_hidden("schtasks", &args, QUERY_TIMEOUT) {
        Ok(Finished {
            code: Some(0),
            stdout,
        }) => Some(stdout),
        _ => None,
    }
}

fn run_script(script: &Path, args: &[OsString], timeout: Duration) -> Result<Finished, AppError> {
    let mut argv: Vec<OsString> = ["-NoLogo", "-NoProfile", "-NonInteractive", "-File"]
        .map(OsString::from)
        .to_vec();
    argv.push(script.as_os_str().to_os_string());
    argv.extend_from_slice(args);
    run_hidden("pwsh", &argv, timeout).map_err(|error| match error {
        RunError::Spawn => AppError::new(
            "err.cloud.pwsh_missing",
            "PowerShell 7 (pwsh) was not found. The cloud companion needs it.",
        ),
        RunError::TimedOut => AppError::new("err.cloud.timeout", "The cloud companion did not finish within the time limit"),
    })
}

/* ------------------------------ Summary ------------------------------ */

fn default_bin_dir() -> Option<String> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    dir.join(format!("zeppbridge-cli{}", std::env::consts::EXE_SUFFIX))
        .is_file()
        .then(|| dir.to_string_lossy().into_owned())
}

/// The half that only reads files and starts no process (tests call it directly). `follow_task_xml` is the scheduled task query result.
pub(crate) fn state_from_files(
    state_dir: &Path,
    follow_task_xml: Option<&str>,
) -> CloudCompanionState {
    let file = read_config_file(state_dir);
    let map = match &file {
        ConfigFile::Found(map) => Some(map),
        _ => None,
    };
    let config = map.map(config_view);
    CloudCompanionState {
        supported: true,
        configured: map.is_some(),
        config_unreadable: matches!(file, ConfigFile::Unreadable),
        state_dir: Some(state_dir.to_string_lossy().into_owned()),
        uploader_dir: locate_uploader(state_dir, map, follow_task_xml)
            .map(|dir| dir.to_string_lossy().into_owned()),
        site_url: config
            .as_ref()
            .and_then(|config| normalize_site(&config.site)),
        config,
        last_run: read_last_run(state_dir),
        readers: read_readers(state_dir),
        task_follow: follow_task_xml.is_some(),
        ..CloudCompanionState::default()
    }
}

fn collect_state() -> CloudCompanionState {
    let Some(dir) = state_dir() else {
        return CloudCompanionState::default();
    };
    let follow = query_task(TASK_FOLLOW);
    let mut state = state_from_files(&dir, follow.as_deref());
    state.task_upload = query_task(TASK_UPLOAD).is_some();
    state.running = BUSY.load(Ordering::SeqCst);
    state.default_bin_dir = default_bin_dir();
    state
}

/// The site URL that the component has configured (`https://health.example.com`). Push settings use it to decide if a one-click takeover is possible.
pub(crate) fn companion_site_url() -> Option<String> {
    let dir = state_dir()?;
    match read_config_file(&dir) {
        ConfigFile::Found(map) => normalize_site(&config_view(&map).site),
        _ => None,
    }
}

/// Do the component's two scheduled tasks still exist? (After push moves into the main app, their runs are duplicate uploads.)
pub(crate) fn companion_tasks_present() -> bool {
    state_dir().is_some()
        && (query_task(TASK_FOLLOW).is_some() || query_task(TASK_UPLOAD).is_some())
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, AppError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| AppError::new("err.cloud.run_failed", "The cloud companion run failed. See its log for details."))
}

/// Hold the "only one at a time" slot. It is released automatically on drop.
struct BusyGuard;
impl BusyGuard {
    fn take() -> Result<Self, AppError> {
        BUSY.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| Self)
            .map_err(|_| AppError::new("err.cloud.busy", "The previous cloud operation has not finished"))
    }
}
impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
    }
}

fn require_uploader(state: &CloudCompanionState) -> Result<PathBuf, AppError> {
    state
        .uploader_dir
        .as_deref()
        .map(PathBuf::from)
        .ok_or_else(|| {
            AppError::new(
                "err.cloud.uploader_not_found",
                "Cannot find the cloud companion upload scripts (run.ps1, setup.ps1, upload.mjs)",
            )
        })
}

fn state_dir_arg(state_dir: &Path) -> [OsString; 2] {
    [
        OsString::from("-StateDir"),
        state_dir.as_os_str().to_os_string(),
    ]
}

/* ------------------------------ Change the config ------------------------------ */

fn clean_path(value: Option<&str>) -> Option<PathBuf> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(PathBuf::from)
}

fn set_or_remove(map: &mut Map<String, Value>, name: &str, value: Option<&Path>) {
    match value {
        Some(path) => {
            map.insert(
                name.to_string(),
                Value::String(path.to_string_lossy().into_owned()),
            );
        }
        None => {
            map.remove(name);
        }
    }
}

/// Validate and write back `config.json`. Change only known fields. Keep the rest (fields that the component adds later, fields the user wrote by hand) as is.
/// If any item fails validation, write nothing.
pub(crate) fn save_config(state_dir: &Path, input: &CloudConfigInput) -> Result<(), AppError> {
    let mut map = match read_config_file(state_dir) {
        ConfigFile::Found(map) => map,
        ConfigFile::Missing => Map::new(),
        ConfigFile::Unreadable => {
            return Err(AppError::new(
                "err.cloud.config_unreadable",
                "Cannot read the cloud companion config.json. Fix or delete it first.",
            ));
        }
    };
    let site = input.site.trim();
    if normalize_site(site).is_none() {
        return Err(AppError::new(
            "err.cloud.site_invalid",
            "Invalid site URL: enter only a domain (such as example.com) or a full https:// URL",
        ));
    }
    if !(1..=365).contains(&input.retention_days) || !(1..=365).contains(&input.summary_days) {
        return Err(AppError::new(
            "err.cloud.days_invalid",
            "Retention days and summary days must be between 1 and 365",
        ));
    }
    let bin_dir = clean_path(input.bin_dir.as_deref());
    if let Some(dir) = &bin_dir {
        let cli = dir.join(format!("zeppbridge-cli{}", std::env::consts::EXE_SUFFIX));
        if !dir.is_absolute() || !cli.is_file() {
            return Err(AppError::new(
                "err.cloud.bin_dir_invalid",
                "This folder does not contain zeppbridge-cli",
            ));
        }
    }
    let data_dir = clean_path(input.data_dir.as_deref());
    if data_dir
        .as_ref()
        .is_some_and(|dir| !dir.is_absolute() || !dir.is_dir())
    {
        return Err(AppError::new(
            "err.cloud.data_dir_invalid",
            "The data folder must be an absolute path to an existing folder",
        ));
    }
    let uploader_dir = clean_path(input.uploader_dir.as_deref());
    if uploader_dir
        .as_ref()
        .is_some_and(|dir| !is_uploader_dir(dir))
    {
        return Err(AppError::new(
            "err.cloud.uploader_not_found",
            "Cannot find the cloud companion upload scripts (run.ps1, setup.ps1, upload.mjs)",
        ));
    }

    map.insert("site".to_string(), Value::String(site.to_string()));
    map.remove("endpoint");
    set_or_remove(&mut map, "binDir", bin_dir.as_deref());
    set_or_remove(&mut map, "dataDir", data_dir.as_deref());
    // If no location was given, keep the stored one: an install that we can find from the scheduled task does not need one more field.
    if uploader_dir.is_some() {
        set_or_remove(&mut map, "uploaderDir", uploader_dir.as_deref());
    }
    map.insert("sync".to_string(), Value::Bool(input.sync));
    map.insert(
        "retentionDays".to_string(),
        Value::from(input.retention_days),
    );
    map.insert(
        "summaryDays".to_string(),
        Value::from(input.summary_days.min(input.retention_days)),
    );
    map.insert(
        "fullResolutionWorkouts".to_string(),
        Value::Bool(input.full_resolution_workouts),
    );

    let failed = || AppError::new("err.cloud.config_write_failed", "Could not write the cloud companion config");
    let bytes = serde_json::to_vec_pretty(&Value::Object(map)).map_err(|_| failed())?;
    std::fs::create_dir_all(state_dir).map_err(|_| failed())?;
    write_file_atomically(&state_dir.join("config.json"), &bytes).map_err(|_| failed())
}

/// The read-only key name goes into command-line arguments and credential entry names: accept only letters, digits, `-` and `_`, and it must start with a letter or digit.
pub(crate) fn valid_label(label: &str) -> bool {
    let mut chars = label.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && label.len() <= 32
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/* ------------------------------ Commands ------------------------------ */

#[tauri::command]
pub async fn cloud_companion_state() -> Result<CloudCompanionState, AppError> {
    blocking(collect_state).await
}

#[tauri::command]
pub async fn cloud_companion_save_config(
    input: CloudConfigInput,
) -> Result<CloudCompanionState, AppError> {
    let dir = state_dir().ok_or_else(unsupported)?;
    blocking(move || {
        save_config(&dir, &input)?;
        Ok(collect_state())
    })
    .await?
}

/// Upload once now: start the component's own `run.ps1 -NoSync` (upload only, no Zepp sync for the desktop app) and wait for it to finish.
/// The result comes from the component's `last-run.json`. If that file did not change, the script failed before the upload.
#[tauri::command]
pub async fn cloud_companion_upload() -> Result<CloudCompanionState, AppError> {
    let dir = state_dir().ok_or_else(unsupported)?;
    blocking(move || {
        let guard = BusyGuard::take()?;
        let before = collect_state();
        if !before.configured {
            return Err(AppError::new(
                "err.cloud.not_configured",
                "The cloud companion has no config yet. Enter the site and save first.",
            ));
        }
        let uploader = require_uploader(&before)?;
        let mut args = vec![OsString::from("-NoSync")];
        args.extend(state_dir_arg(&dir));
        let finished = run_script(&uploader.join("run.ps1"), &args, UPLOAD_TIMEOUT)?;
        drop(guard);
        let after = collect_state();
        let stamp = |state: &CloudCompanionState| state.last_run.as_ref().map(|run| run.at.clone());
        if stamp(&after) == stamp(&before) {
            return Err(
                AppError::new("err.cloud.run_failed", "The cloud companion run failed. See its log for details.")
                    .with_params(serde_json::json!({ "exit_code": finished.code })),
            );
        }
        Ok(after)
    })
    .await?
}

/// The actions in the component's `setup.ps1` that the UI can use: issue / copy / revoke read-only keys, create a tenant, print scheduled task commands.
/// The component puts keys on the clipboard itself. We do not read its output (except `task-commands`: two commands with no keys).
#[tauri::command]
pub async fn cloud_companion_setup(
    command: String,
    label: Option<String>,
) -> Result<CloudSetupOutcome, AppError> {
    let dir = state_dir().ok_or_else(unsupported)?;
    let needs_label = matches!(
        command.as_str(),
        "copy-reader" | "add-reader" | "revoke-reader"
    );
    if !needs_label && !matches!(command.as_str(), "init" | "task-commands") {
        return Err(AppError::new(
            "err.cloud.run_failed",
            "The cloud companion run failed. See its log for details.",
        ));
    }
    let label = label.unwrap_or_default();
    if needs_label && !valid_label(&label) {
        return Err(AppError::new(
            "err.cloud.label_invalid",
            "The name can use only letters, digits, - and _, with a maximum of 32 characters",
        ));
    }
    blocking(move || {
        let guard = BusyGuard::take()?;
        let uploader = require_uploader(&collect_state())?;
        let mut args = vec![OsString::from(&command)];
        if needs_label {
            args.push(OsString::from("-Label"));
            args.push(OsString::from(&label));
        }
        args.extend(state_dir_arg(&dir));
        let finished = run_script(&uploader.join("setup.ps1"), &args, SETUP_TIMEOUT)?;
        drop(guard);
        if finished.code != Some(0) {
            return Err(
                AppError::new("err.cloud.run_failed", "The cloud companion run failed. See its log for details.")
                    .with_params(serde_json::json!({ "exit_code": finished.code })),
            );
        }
        Ok(CloudSetupOutcome {
            state: collect_state(),
            output: (command == "task-commands").then(|| finished.stdout.trim().to_string()),
        })
    })
    .await?
}

/// Open the site (with the browser read-only key, the component signs in automatically), the log folder or the component folder.
#[tauri::command]
pub async fn cloud_companion_open(app: tauri::AppHandle, target: String) -> Result<(), AppError> {
    let dir = state_dir().ok_or_else(unsupported)?;
    blocking(move || {
        let state = collect_state();
        let failed = || AppError::new("err.cloud.open_failed", "Could not open");
        match target.as_str() {
            "site" => {
                let has_browser_key = state.readers.iter().any(|reader| reader.label == "browser");
                if let (true, Some(uploader)) = (has_browser_key, state.uploader_dir.as_deref()) {
                    let mut args = vec![OsString::from("open-dashboard")];
                    args.extend(state_dir_arg(&dir));
                    let script = Path::new(uploader).join("setup.ps1");
                    if matches!(
                        run_script(&script, &args, QUERY_TIMEOUT * 2),
                        Ok(Finished { code: Some(0), .. })
                    ) {
                        return Ok(());
                    }
                }
                let url = state.site_url.ok_or_else(failed)?;
                crate::external_open::open_url(&app, &url).map_err(|_| failed())
            }
            "logs" => {
                let logs = dir.join("logs");
                open_folder(if logs.is_dir() { &logs } else { &dir }).map_err(|_| failed())
            }
            "folder" => {
                let uploader = require_uploader(&state)?;
                open_folder(&uploader).map_err(|_| failed())
            }
            _ => Err(failed()),
        }
    })
    .await?
}

#[cfg(test)]
mod tests;
