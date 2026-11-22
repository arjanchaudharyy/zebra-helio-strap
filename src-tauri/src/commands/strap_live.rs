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
