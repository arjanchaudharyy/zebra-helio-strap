//! Live data straight from the Amazfit Helio Strap over Bluetooth.
//!
//! The strap broadcasts standard BLE heart rate (service 0x180D, characteristic
//! 0x2A37) without any pairing key, about once a second, and exposes battery
//! (0x2A19) and firmware (0x2A28). This module connects, streams heart rate to
//! the page as `zebra://strap` events, and stops on request. It writes nothing
//! to the strap.

use crate::ipc_error::AppError;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Manager, Peripheral};
use futures::StreamExt;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

const HR_SERVICE: Uuid = Uuid::from_u128(0x0000180d_0000_1000_8000_00805f9b34fb);
const HR_MEASUREMENT: Uuid = Uuid::from_u128(0x00002a37_0000_1000_8000_00805f9b34fb);
const BATTERY_LEVEL: Uuid = Uuid::from_u128(0x00002a19_0000_1000_8000_00805f9b34fb);
const FIRMWARE: Uuid = Uuid::from_u128(0x00002a28_0000_1000_8000_00805f9b34fb);

static TASK: Mutex<Option<tauri::async_runtime::JoinHandle<()>>> = Mutex::new(None);
/// Last device info and status, re-sent to any page that asks, so it shows at once.
static LAST: Mutex<Vec<StrapEvent>> = Mutex::new(Vec::new());
static DATA_DIR: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum StrapEvent {
    Status { state: String, detail: Option<String> },
    Info { name: String, battery: Option<u8>, firmware: Option<String> },
    Hr { bpm: u16, at: i64, rr: Vec<u16> },
}

fn emit(app: &AppHandle, event: StrapEvent) {
    match &event {
        StrapEvent::Hr { bpm, at, rr } => {
            // Keep every reading: one JSON line per beat report in <data>/strap/live-hr.jsonl.
            if let Ok(dir) = DATA_DIR.lock() {
                if let Some(dir) = dir.as_ref() {
                    use std::io::Write;
                    let _ = std::fs::create_dir_all(dir);
                    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("live-hr.jsonl")) {
                        let _ = writeln!(f, "{}", serde_json::json!({ "t": at, "bpm": bpm, "rr": rr }));
                    }
                }
            }
        }
        StrapEvent::Status { .. } | StrapEvent::Info { .. } => {
            if let Ok(mut last) = LAST.lock() {
                last.retain(|e| std::mem::discriminant(e) != std::mem::discriminant(&event));
                last.push(event.clone());
            }
        }
    }
    let _ = app.emit("zebra://strap", event);
}

fn status(app: &AppHandle, state: &str, detail: Option<String>) {
    emit(app, StrapEvent::Status { state: state.to_string(), detail });
}

/// Heart Rate Measurement: flags, then 8- or 16-bit bpm, optional energy, then RR intervals (1/1024 s).
fn parse_hr(data: &[u8]) -> Option<(u16, Vec<u16>)> {
    let flags = *data.first()?;
    let mut i = 1;
    let bpm = if flags & 0x01 != 0 {
        let v = u16::from_le_bytes([*data.get(1)?, *data.get(2)?]);
        i += 2;
        v
    } else {
        i += 1;
        *data.get(1)? as u16
    };
    if flags & 0x08 != 0 {
        i += 2;
    }
    let mut rr = Vec::new();
    if flags & 0x10 != 0 {
        while i + 1 < data.len() {
            let raw = u16::from_le_bytes([data[i], data[i + 1]]);
            rr.push(((raw as u32 * 1000) / 1024) as u16);
            i += 2;
        }
    }
    Some((bpm, rr))
}

async fn find_strap(app: &AppHandle) -> Result<Peripheral, String> {
    let manager = Manager::new().await.map_err(|e| e.to_string())?;
    let adapter = manager
        .adapters()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .next()
        .ok_or("No Bluetooth adapter. Turn Bluetooth on.")?;
    status(app, "scanning", None);
    let mut events = adapter.events().await.map_err(|e| e.to_string())?;
    adapter
        .start_scan(ScanFilter { services: vec![HR_SERVICE] })
        .await
        .map_err(|e| e.to_string())?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
    loop {
        let next = tokio::time::timeout_at(deadline, events.next()).await;
        let Ok(Some(event)) = next else {
            let _ = adapter.stop_scan().await;
            return Err("Strap not found. Wear it and keep it near the Mac.".into());
        };
        if let CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) = event {
            if let Ok(p) = adapter.peripheral(&id).await {
                let name = p
                    .properties()
                    .await
                    .ok()
                    .flatten()
                    .and_then(|props| props.local_name)
                    .unwrap_or_default();
                if name.starts_with("Amazfit") {
                    let _ = adapter.stop_scan().await;
                    return Ok(p);
                }
            }
        }
    }
}

async fn run(app: AppHandle) -> Result<(), String> {
    let strap = find_strap(&app).await?;
    status(&app, "connecting", None);
    strap.connect().await.map_err(|e| e.to_string())?;
    strap.discover_services().await.map_err(|e| e.to_string())?;
    let chars = strap.characteristics();
    let name = strap
        .properties()
        .await
        .ok()
        .flatten()
        .and_then(|p| p.local_name)
        .unwrap_or_else(|| "Amazfit Helio Strap".into());
    let mut battery = None;
    let mut firmware = None;
    for c in &chars {
        if c.uuid == BATTERY_LEVEL {
            battery = strap.read(c).await.ok().and_then(|v| v.first().copied());
        } else if c.uuid == FIRMWARE {
            firmware = strap.read(c).await.ok().map(|v| String::from_utf8_lossy(&v).to_string());
        }
    }
    emit(&app, StrapEvent::Info { name, battery, firmware });
    let hr = chars
        .iter()
        .find(|c| c.uuid == HR_MEASUREMENT)
        .ok_or("This strap does not expose heart rate.")?
        .clone();
    strap.subscribe(&hr).await.map_err(|e| e.to_string())?;
    status(&app, "live", None);
    let mut stream = strap.notifications().await.map_err(|e| e.to_string())?;
    while let Some(n) = stream.next().await {
        if n.uuid != HR_MEASUREMENT {
            continue;
        }
        if let Some((bpm, rr)) = parse_hr(&n.value) {
            if bpm > 0 {
                emit(
                    &app,
                    StrapEvent::Hr { bpm, at: chrono::Utc::now().timestamp_millis(), rr },
                );
            }
        }
    }
    let _ = strap.disconnect().await;
    Err("The strap disconnected.".into())
}

#[tauri::command]
pub fn strap_live_start(
    app: AppHandle,
    state: tauri::State<'_, crate::app_state::AppState>,
) -> Result<(), AppError> {
    if let Ok(mut d) = DATA_DIR.lock() {
        *d = Some(state.data_dir.join("strap"));
    }
    let mut guard = TASK.lock().map_err(|_| AppError::new("err.strap.lock", "busy"))?;
    // Already connected (or reconnecting): just re-send the last status so the page fills in at once.
    if guard.as_ref().is_some_and(|t| !t.inner().is_finished()) {
        if let Ok(last) = LAST.lock() {
            for e in last.iter() {
                let _ = app.emit("zebra://strap", e.clone());
            }
        }
        return Ok(());
    }
    let handle = app.clone();
    // Stay connected 24/7: when the strap drops out, wait and reconnect.
    *guard = Some(tauri::async_runtime::spawn(async move {
        loop {
            if let Err(message) = run(handle.clone()).await {
                status(&handle, "reconnecting", Some(message));
            }
            tokio::time::sleep(Duration::from_secs(8)).await;
        }
    }));
    Ok(())
}

/// Saved live readings from the last `minutes` minutes (from live-hr.jsonl).
#[tauri::command]
pub fn strap_live_recent(
    state: tauri::State<'_, crate::app_state::AppState>,
    minutes: i64,
) -> Vec<serde_json::Value> {
    let cut = chrono::Utc::now().timestamp_millis() - minutes.clamp(1, 1440) * 60_000;
    let path = state.data_dir.join("strap").join("live-hr.jsonl");
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    // Read only the tail: at about one reading a second, a day is ~86k lines.
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub((minutes.clamp(1, 1440) as usize) * 90);
    lines[start..]
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.get("t").and_then(|t| t.as_i64()).is_some_and(|t| t >= cut))
        .collect()
}

#[tauri::command]
pub fn strap_live_stop(app: AppHandle) -> Result<(), AppError> {
    if let Ok(mut guard) = TASK.lock() {
        if let Some(task) = guard.take() {
            task.abort();
        }
    }
    status(&app, "stopped", None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_hr;

    #[test]
    fn parses_8bit_heart_rate() {
        assert_eq!(parse_hr(&[0x00, 72]), Some((72, vec![])));
    }

    #[test]
    fn parses_rr_intervals() {
        // flags: 8-bit HR + RR present; RR 1024 = 1000 ms
        assert_eq!(parse_hr(&[0x10, 60, 0x00, 0x04]), Some((60, vec![1000])));
    }
}

/// Pairing key of the user's own strap, read from their Zepp account's device
/// list (needs the Advanced data connection). Saved to `<data dir>/strap.key`
/// (0600) and never returned to the page. Returns how many keys were found.
#[tauri::command]
pub async fn strap_key_fetch(
    state: tauri::State<'_, crate::app_state::AppState>,
) -> Result<usize, AppError> {
    use zeppbridge_core::connectors::zepp::ZeppConnector;
    let auth = state
        .auth
        .load_auth()
        .map_err(AppError::from)?
        .ok_or_else(|| AppError::new("err.strap.no_app_token", "Connect Advanced data in Settings first."))?;
    let connector = ZeppConnector::new(auth).map_err(AppError::from)?;
    let payload = connector.fetch_devices().await.map_err(AppError::from)?;
    let items = payload
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .or_else(|| payload.as_array().cloned())
        .unwrap_or_default();
    let mut lines = Vec::new();
    for item in &items {
        let info: serde_json::Value = item
            .get("additionalInfo")
            .and_then(|v| v.as_str())
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or(serde_json::Value::Null);
        let key = info.get("auth_key").and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        if key.len() < 32 {
            continue;
        }
        let source = item.get("deviceSource").map(|v| v.to_string()).unwrap_or_default();
        let mac = item.get("macAddress").and_then(|v| v.as_str()).unwrap_or("").to_string();
        lines.push(format!("{source}\t{mac}\t{key}"));
    }
    if lines.is_empty() {
        return Ok(0);
    }
    let path = state.data_dir.join("strap.key");
    std::fs::write(&path, lines.join("\n") + "\n")
        .map_err(|e| AppError::new("err.strap.key_write", e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(lines.len())
}

/// One day of data read straight from the strap (files written by the zebrastrap
/// client into `<data dir>/strap/`). Missing values stay `None`.
#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StrapDay {
    pub date: String,
    pub hrv_rmssd: Option<f64>,
    pub hrv_sdnn_ms: Option<f64>,
    pub skin_temp_c: Option<f64>,
    pub stress_avg: Option<f64>,
    pub spo2_avg: Option<f64>,
    pub breath_rate: Option<f64>,
    pub resting_hr: Option<f64>,
    pub readiness: Option<f64>,
}

fn local_day(ts: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(ts, 0)
        .single()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

#[tauri::command]
pub fn strap_daily(
    state: tauri::State<'_, crate::app_state::AppState>,
) -> Result<Vec<StrapDay>, AppError> {
    Ok(read_strap_days(&state.data_dir.join("strap")))
}

pub(crate) fn read_strap_days(dir: &std::path::Path) -> Vec<StrapDay> {
    use std::collections::BTreeMap;
    // day -> metric -> (sum, count)
    let mut acc: BTreeMap<String, BTreeMap<&'static str, (f64, f64)>> = BTreeMap::new();
    let files: [(&str, &str, &'static str, f64, fn(f64) -> bool); 9] = [
        ("73_hrv_rmssd.jsonl", "rmssd", "hrv", 1.0, |v| v > 0.0 && v < 250.0),
        ("78_hrv_sdnn.jsonl", "sdnn_centi_ms", "sdnn", 0.01, |v| v > 0.0 && v < 400.0),
        ("46_skin_temperature.jsonl", "skin_centi_celsius", "temp", 0.01, |v| v > 25.0 && v < 42.0),
        ("19_stress_all_day.jsonl", "stress", "stress", 1.0, |v| v > 0.0 && v <= 100.0),
        ("18_stress_single.jsonl", "stress", "stress", 1.0, |v| v > 0.0 && v <= 100.0),
        ("37_spo2_single.jsonl", "spo2", "spo2", 1.0, |v| v >= 70.0 && v <= 100.0),
        ("56_sleep_breath_rate.jsonl", "breath_rate", "breath", 1.0, |v| v > 4.0 && v < 40.0),
        ("58_resting_heart_rate.jsonl", "heart_rate", "rhr", 1.0, |v| v > 25.0 && v < 130.0),
        ("38_spo2_sleep.jsonl", "spo2", "spo2", 1.0, |v| v >= 70.0 && v <= 100.0),
    ];
    for (file, field, metric, scale, valid) in files {
        let Ok(text) = std::fs::read_to_string(dir.join(file)) else { continue };
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let (Some(ts), Some(raw)) = (
                v.get("timestamp").and_then(|x| x.as_i64()),
                v.get(field).and_then(|x| x.as_f64()),
            ) else {
                continue;
            };
            let value = raw * scale;
            if !valid(value) {
                continue;
            }
            let slot = acc.entry(local_day(ts)).or_default().entry(metric).or_insert((0.0, 0.0));
            slot.0 += value;
            slot.1 += 1.0;
        }
    }
    // Readiness: one score per day, from the strap's readiness module (field `day`).
    if let Ok(text) = std::fs::read_to_string(dir.join("readiness_08.jsonl")) {
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let (Some(day), Some(score)) = (v.get("day").and_then(|x| x.as_i64()), v.get("score").and_then(|x| x.as_f64())) else { continue };
            if score > 0.0 && score <= 100.0 {
                acc.entry(local_day(day)).or_default().insert("ready", (score, 1.0));
            }
        }
    }
    let avg = |m: &BTreeMap<&str, (f64, f64)>, k: &str| {
        m.get(k).filter(|(_, n)| *n > 0.0).map(|(s, n)| ((s / n) * 10.0).round() / 10.0)
    };
    acc
        .iter()
        .map(|(date, m)| StrapDay {
            date: date.clone(),
            hrv_rmssd: avg(m, "hrv"),
            hrv_sdnn_ms: avg(m, "sdnn"),
            skin_temp_c: avg(m, "temp"),
            stress_avg: avg(m, "stress"),
            spo2_avg: avg(m, "spo2"),
            breath_rate: avg(m, "breath"),
            resting_hr: avg(m, "rhr"),
            readiness: avg(m, "ready"),
        })
        .collect()
}

#[cfg(test)]
mod strap_file_tests {
    #[test]
    fn averages_strap_records_per_day() {
        let dir = std::env::temp_dir().join(format!("zebra-strap-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let noon = chrono::Local::now().timestamp() / 86400 * 86400 + 12 * 3600;
        std::fs::write(dir.join("73_hrv_rmssd.jsonl"), format!(
            "{{\"type\":\"hrv_rmssd\",\"timestamp\":{noon},\"tz_quarters\":23,\"rmssd\":48}}\n{{\"type\":\"hrv_rmssd\",\"timestamp\":{},\"tz_quarters\":23,\"rmssd\":52}}\n", noon + 60)).unwrap();
        std::fs::write(dir.join("46_skin_temperature.jsonl"), format!(
            "{{\"timestamp\":{noon},\"skin_centi_celsius\":3412,\"ambient_centi_celsius\":null}}\n{{\"timestamp\":{},\"skin_centi_celsius\":null,\"ambient_centi_celsius\":2500}}\n", noon + 60)).unwrap();
        let days = super::read_strap_days(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].hrv_rmssd, Some(50.0));
        assert_eq!(days[0].skin_temp_c, Some(34.1));
        assert_eq!(days[0].stress_avg, None);
    }
}

/// Download history straight from the strap with the zebrastrap client
/// (~/zebra-ble). The strap keeps only about 43 hours of minute data, so the
/// page calls this on start and every few hours. Output goes to `<data>/strap/`.
/// The key is passed in the environment and never logged.
#[tauri::command]
pub async fn strap_sync_now(
    app: AppHandle,
    state: tauri::State<'_, crate::app_state::AppState>,
) -> Result<String, AppError> {
    // The history download needs the strap to itself: pause the live link, then resume it.
    let was_live = TASK.lock().ok().and_then(|mut g| g.take()).map(|t| { t.abort(); true }).unwrap_or(false);
    if was_live {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let result = sync_inner(&state).await;
    if was_live {
        let _ = strap_live_start(app.clone(), state);
    }
    result
}

async fn sync_inner(state: &tauri::State<'_, crate::app_state::AppState>) -> Result<String, AppError> {
    let key_file = state.data_dir.join("strap.key");
    let text = std::fs::read_to_string(&key_file)
        .map_err(|_| AppError::new("err.strap.no_key", "Unlock the strap on the Live page first."))?;
    let key = text
        .lines()
        .filter_map(|l| l.split('\t').nth(2))
        .map(str::trim)
        .find(|k| k.len() >= 32)
        .ok_or_else(|| AppError::new("err.strap.no_key", "No strap key saved."))?
        .to_string();
    let root = strap_client_dir();
    let python = root.join(".venv/bin/python");
    if !python.exists() {
        return Err(AppError::new("err.strap.no_client", "The strap client is not installed in ~/zebra-ble."));
    }
    let out = state.data_dir.join("strap");
    let output = tokio::process::Command::new(python)
        .current_dir(&root)
        .args(["-m", "zebrastrap.cli", "sync", "--zepp-only", "--out"])
        .arg(&out)
        .env("ZEBRA_STRAP_KEY", &key)
        .output()
        .await
        .map_err(|e| AppError::new("err.strap.spawn", e.to_string()))?;
    let mut log = String::from_utf8_lossy(&output.stdout).to_string();
    // Health watch: rules over the fresh data; buzzes the strap once for any new alert.
    if output.status.success() {
        let python = root.join(".venv/bin/python");
        if let Ok(w) = tokio::process::Command::new(python)
            .current_dir(&root)
            .args(["-m", "zebrastrap.cli", "watch", "--out"])
            .arg(&out)
            .env("ZEBRA_STRAP_KEY", &key)
            .output()
            .await
        {
            log.push_str(&String::from_utf8_lossy(&w.stdout));
        }
    }
    let _ = std::fs::write(out.join("last-sync.log"), &log);
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let last = err.lines().last().unwrap_or("sync failed").to_string();
        return Err(AppError::new("err.strap.sync", last));
    }
    let new: usize = log
        .lines()
        .filter_map(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            parts.iter().position(|p| *p == "new").and_then(|i| parts.get(i - 1)).and_then(|n| n.parse::<usize>().ok())
        })
        .sum();
    Ok(format!("{new} new records"))
}

/// The health watch result written by `zebrastrap watch`: recent alerts and the illness status.
#[tauri::command]
pub fn strap_alerts(
    state: tauri::State<'_, crate::app_state::AppState>,
) -> Result<serde_json::Value, AppError> {
    let path = state.data_dir.join("strap").join("alerts.json");
    Ok(std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::json!({ "alerts": [], "illness": null })))
}

/// Page to open at launch, from the `ZEBRA_START_ROUTE` environment variable (testing aid).
#[tauri::command]
pub fn zebra_start_route() -> Option<String> {
    std::env::var("ZEBRA_START_ROUTE").ok().filter(|r| r.starts_with('/') && !r.starts_with("//"))
}

/// Run one zebrastrap command with the saved key and return its stdout.
async fn strap_cli(state: &tauri::State<'_, crate::app_state::AppState>, args: &[&str]) -> Result<String, AppError> {
    let text = std::fs::read_to_string(state.data_dir.join("strap.key"))
        .map_err(|_| AppError::new("err.strap.no_key", "Unlock the strap on the Live page first."))?;
    let key = text
        .lines()
        .filter_map(|l| l.split('\t').nth(2))
        .map(str::trim)
        .find(|k| k.len() >= 32)
        .ok_or_else(|| AppError::new("err.strap.no_key", "No strap key saved."))?
        .to_string();
    let root = strap_client_dir();
    let output = tokio::process::Command::new(root.join(".venv/bin/python"))
        .current_dir(&root)
        .args(["-m", "zebrastrap.cli"])
        .args(args)
        .env("ZEBRA_STRAP_KEY", &key)
        .output()
        .await
        .map_err(|e| AppError::new("err.strap.spawn", e.to_string()))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("The strap did not answer.");
        return Err(AppError::new("err.strap.cli", line.to_string()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// The strap's own health and workout settings, read over Bluetooth.
#[tauri::command]
pub async fn strap_settings(app: AppHandle, state: tauri::State<'_, crate::app_state::AppState>) -> Result<serde_json::Value, AppError> {
    let out = with_link_paused(&app, &state, strap_cli(&state, &["settings"])).await?;
    serde_json::from_str(out.trim()).map_err(|e| AppError::new("err.strap.parse", e.to_string()))
}

/// Change one choice setting on the strap, e.g. key "08:02" (high heart rate alert) to 140.
#[tauri::command]
pub async fn strap_set(app: AppHandle, state: tauri::State<'_, crate::app_state::AppState>, key: String, value: i64) -> Result<(), AppError> {
    let ok = key.len() == 5 && key.as_bytes()[2] == b':' && key.chars().filter(|c| *c != ':').all(|c| c.is_ascii_hexdigit());
    if !ok {
        return Err(AppError::new("err.strap.key", "Bad setting key."));
    }
    let v = value.to_string();
    with_link_paused(&app, &state, strap_cli(&state, &["set", &key, "--value", &v])).await.map(|_| ())
}

/// Buzz the strap with a named pattern: "find" (four long buzzes) or "breathe" (three slow breaths).
#[tauri::command]
pub async fn strap_buzz(app: AppHandle, state: tauri::State<'_, crate::app_state::AppState>, pattern: String) -> Result<(), AppError> {
    if !matches!(pattern.as_str(), "find" | "breathe" | "hello") {
        return Err(AppError::new("err.strap.pattern", "Unknown buzz pattern."));
    }
    with_link_paused(&app, &state, strap_cli(&state, &["buzz", &pattern])).await.map(|_| ())
}

/// Folder of the Python strap client: `ZEBRA_STRAP_DIR`, else `~/zebra-ble`.
fn strap_client_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("ZEBRA_STRAP_DIR") {
        if !dir.trim().is_empty() {
            return std::path::PathBuf::from(dir);
        }
    }
    std::path::Path::new(&std::env::var("HOME").unwrap_or_default()).join("zebra-ble")
}

/// Everything the strap stored for one local day, from the files the strap sync writes:
/// heart rate (from per-second records, as one-minute averages), steps and stress per minute,
/// plus the latest PAI, the recent sleep breathing (ODI) screens and last night's oxygen.
#[tauri::command]
pub fn strap_detail(
    state: tauri::State<'_, crate::app_state::AppState>,
    day: Option<String>,
) -> serde_json::Value {
    use chrono::{Local, NaiveDate, TimeZone};
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader};
    let dir = state.data_dir.join("strap");
    let date = day
        .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
        .unwrap_or_else(|| Local::now().date_naive());
    let start = Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).unwrap_or_default())
        .single()
        .map(|d| d.timestamp())
        .unwrap_or(0);
    let end = start + 86_400;
    let each = |name: &str, f: &mut dyn FnMut(&serde_json::Value)| {
        if let Ok(file) = std::fs::File::open(dir.join(name)) {
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                    f(&v);
                }
            }
        }
    };
    let ts = |v: &serde_json::Value| v.get("timestamp").and_then(|t| t.as_i64()).unwrap_or(0);
    let num = |v: &serde_json::Value, k: &str| v.get(k).and_then(|t| t.as_f64());

    // Heart rate: per-second records, averaged per minute.
    let mut hr: BTreeMap<i64, (f64, u32)> = BTreeMap::new();
    let mut hr_count = 0u32;
    each("87_sec_hr_v2.jsonl", &mut |v| {
        let t = ts(v);
        if t >= start && t < end {
            if let Some(b) = num(v, "heart_rate").filter(|b| (25.0..=230.0).contains(b)) {
                let e = hr.entry(t / 60 * 60).or_insert((0.0, 0));
                e.0 += b;
                e.1 += 1;
                hr_count += 1;
            }
        }
    });
    // Steps, intensity and wear per minute.
    let mut steps: Vec<serde_json::Value> = Vec::new();
    let mut total_steps = 0.0;
    let mut worn_minutes = 0u32;
    each("01_activity.jsonl", &mut |v| {
        let t = ts(v);
        if t >= start && t < end {
            let s = num(v, "steps").unwrap_or(0.0);
            total_steps += s;
            if v.get("is_worn").and_then(|w| w.as_bool()) == Some(true) {
                worn_minutes += 1;
            }
            if s > 0.0 {
                steps.push(serde_json::json!({ "t": t * 1000, "steps": s }));
            }
        }
    });
    // Stress per minute (255 means no reading).
    let mut stress: Vec<serde_json::Value> = Vec::new();
    each("19_stress_all_day.jsonl", &mut |v| {
        let t = ts(v);
        if t >= start && t < end {
            if let Some(s) = num(v, "stress").filter(|s| *s > 0.0 && *s <= 100.0) {
                stress.push(serde_json::json!({ "t": t * 1000, "v": s }));
            }
        }
    });
    // Skin temperature, averaged per 10 minutes.
    let mut skin: BTreeMap<i64, (f64, u32)> = BTreeMap::new();
    each("46_skin_temperature.jsonl", &mut |v| {
        let t = ts(v);
        if t >= start && t < end {
            if let Some(c) = num(v, "skin_celsius").filter(|c| (25.0..=42.0).contains(c)) {
                let e = skin.entry(t / 600 * 600).or_insert((0.0, 0));
                e.0 += c;
                e.1 += 1;
            }
        }
    });
    // What you were doing each minute (each record flags one minute), plus distance.
    let (mut sit, mut stand, mut walk, mut run, mut fat_burn, mut floors, mut distance_cm) = (0u32, 0u32, 0u32, 0u32, 0u32, 0.0, 0.0);
    each("88_today_activity_extended.jsonl", &mut |v| {
        let t = ts(v);
        if t >= start && t < end {
            let on = |k: &str| u32::from(num(v, k).unwrap_or(0.0) > 0.0);
            sit += on("sit_duration");
            stand += on("stand_duration");
            walk += on("walk_duration");
            run += on("run_duration");
            fat_burn += on("fat_burn_duration");
            floors += num(v, "floor_count").unwrap_or(0.0);
            distance_cm += num(v, "distance_cm").unwrap_or(0.0);
        }
    });
    // Latest PAI record.
    let mut pai = serde_json::Value::Null;
    each("13_pai.jsonl", &mut |v| {
        if v.get("total_pai").is_some() {
            pai = serde_json::json!({
                "at": ts(v) * 1000,
                "daily": num(v, "daily_pai"), "total": num(v, "total_pai"),
                "lowMinutes": num(v, "low_zone_minutes"), "mediumMinutes": num(v, "medium_zone_minutes"), "highMinutes": num(v, "high_zone_minutes"),
                "lowFrom": num(v, "low_zone_limit"), "mediumFrom": num(v, "medium_zone_limit"), "highFrom": num(v, "high_zone_limit"),
                "restHr": num(v, "rest_hr"), "maxHr": num(v, "max_hr"),
            });
        }
    });
    // Sleep breathing screens (oxygen dips per hour of sleep).
    let mut osa: Vec<serde_json::Value> = Vec::new();
    each("74_osa_result.jsonl", &mut |v| {
        osa.push(serde_json::json!({
            "night": ts(v) * 1000,
            "score": num(v, "odi_score"),
            "hoursChecked": num(v, "odi_valid_time").map(|s| (s / 360.0).round() / 10.0),
            "dips": num(v, "od_num_refined"),
        }));
    });
    let osa: Vec<_> = osa.into_iter().rev().take(14).collect();
    // Oxygen during the most recent night.
    let mut nights: BTreeMap<i64, Vec<f64>> = BTreeMap::new();
    // Each record carries per-sample highs and lows as hex bytes (v2.high / v2.low).
    let bytes = |h: Option<&str>| -> Vec<f64> {
        h.map(|h| (0..h.len() / 2).filter_map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()).map(f64::from).filter(|x| (50.0..=100.0).contains(x)).collect())
            .unwrap_or_default()
    };
    each("38_spo2_sleep.jsonl", &mut |v| {
        let v2 = v.get("v2");
        let mut xs = bytes(v2.and_then(|x| x.get("low")).and_then(|x| x.as_str()));
        xs.extend(bytes(v2.and_then(|x| x.get("high")).and_then(|x| x.as_str())));
        if xs.is_empty() {
            if let Some(s) = num(v, "spo2").filter(|s| (50.0..=100.0).contains(s)) {
                xs.push(s);
            }
        }
        nights.entry((ts(v) + 12 * 3600) / 86_400).or_default().extend(xs);
    });
    let night_oxygen = nights.iter().next_back().map(|(_, xs)| {
        let min = xs.iter().cloned().fold(f64::MAX, f64::min);
        let avg = xs.iter().sum::<f64>() / xs.len() as f64;
        serde_json::json!({ "min": min, "avg": (avg * 10.0).round() / 10.0, "readings": xs.len() })
    });

    serde_json::json!({
        "day": date.format("%Y-%m-%d").to_string(),
        "heartRate": hr.iter().map(|(t, (s, n))| serde_json::json!({ "t": t * 1000, "bpm": (s / *n as f64).round() })).collect::<Vec<_>>(),
        "heartRateReadings": hr_count,
        "steps": steps,
        "totalSteps": total_steps,
        "wornMinutes": worn_minutes,
        "stress": stress,
        "pai": pai,
        "breathing": osa,
        "nightOxygen": night_oxygen,
        "skinTemp": skin.iter().map(|(t, (s, n))| serde_json::json!({ "t": t * 1000, "c": (s / *n as f64 * 100.0).round() / 100.0 })).collect::<Vec<_>>(),
        "minutes": { "sit": sit, "stand": stand, "walk": walk, "run": run, "fatBurn": fat_burn },
        "floors": floors,
        "distanceM": (distance_cm / 100.0_f64).round(),
    })
}

/// Run a strap session with the live heart-rate link paused, then resume the link.
async fn with_link_paused<T>(
    app: &AppHandle,
    state: &tauri::State<'_, crate::app_state::AppState>,
    work: impl std::future::Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    let was_live = TASK.lock().ok().and_then(|mut g| g.take()).map(|t| { t.abort(); true }).unwrap_or(false);
    if was_live {
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let result = work.await;
    if was_live {
        let _ = strap_live_start(app.clone(), state.clone());
    }
    result
}

/// Daily goals (steps, calories, active minutes) from the stored Zepp daily summaries.
#[tauri::command]
pub async fn daily_goals(state: tauri::State<'_, crate::app_state::AppState>) -> Result<serde_json::Value, AppError> {
    super::spawn_independent_read(state.data_dir.clone(), |db| db.latest_daily_goals()).await
}
