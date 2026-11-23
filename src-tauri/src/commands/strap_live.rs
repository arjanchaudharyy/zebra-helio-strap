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

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum StrapEvent {
    Status { state: String, detail: Option<String> },
    Info { name: String, battery: Option<u8>, firmware: Option<String> },
    Hr { bpm: u16, at: i64, rr: Vec<u16> },
}

fn emit(app: &AppHandle, event: StrapEvent) {
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
pub fn strap_live_start(app: AppHandle) -> Result<(), AppError> {
    let mut guard = TASK.lock().map_err(|_| AppError::new("err.strap.lock", "busy"))?;
    if let Some(task) = guard.take() {
        task.abort();
    }
    let handle = app.clone();
    *guard = Some(tauri::async_runtime::spawn(async move {
        if let Err(message) = run(handle.clone()).await {
            status(&handle, "error", Some(message));
        }
    }));
    Ok(())
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
