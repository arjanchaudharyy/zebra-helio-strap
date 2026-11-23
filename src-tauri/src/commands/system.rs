//! The OS regional format (date order and 12/24-hour clock follow it), for the UI option "Follow system region".
//!
//! This is not the UI language: a Portuguese user with an English UI has the regional format `pt-PT` (day/month/year, 24-hour).
//! "Follow system region" used to follow the UI language, so these users saw only MM/DD.
//! If it cannot be read, the result is `None` and the UI falls back to the UI language, as before.

/// The regional format language tag, such as `pt-PT` or `en_GB.UTF-8`. The frontend normalizes it.
#[tauri::command]
pub async fn system_region_locale() -> Option<String> {
    tauri::async_runtime::spawn_blocking(detect)
        .await
        .ok()
        .flatten()
}

fn detect() -> Option<String> {
    #[cfg(windows)]
    {
        // `GetUserDefaultLocaleName` is the "Settings > Time & language > Region > Regional format" value.
        crate::windows_ui_locale()
    }
    #[cfg(target_os = "macos")]
    {
        // Apps started from Finder have no LANG. The region from "Language & Region" is in AppleLocale (for example en_PT).
        std::process::Command::new("/usr/bin/defaults")
            .args(["read", "-g", "AppleLocale"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| first_nonempty(&String::from_utf8_lossy(&output.stdout)))
            .or_else(from_env)
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        from_env()
    }
}

/// POSIX priority: `LC_ALL` beats `LC_TIME`, then `LANG`. `C` and `POSIX` are not regions.
#[cfg_attr(windows, allow(dead_code))]
fn from_env() -> Option<String> {
    ["LC_ALL", "LC_TIME", "LANG"].into_iter().find_map(|key| {
        let value = std::env::var(key).ok()?;
        first_nonempty(&value).filter(|value| value != "C" && value != "POSIX")
    })
}

#[cfg_attr(windows, allow(dead_code))]
fn first_nonempty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
