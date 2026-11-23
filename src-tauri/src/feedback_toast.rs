//! System notification for the post-workout rating (the token and database writes are in `core::storage::feedback_toast`).
//!
//! - Windows: native WinRT notification with five rating buttons. Each button is a `zeppbridge://feedback?t=...&r=...` link.
//!   When clicked, the system starts this program with it. When `main.rs` sees this argument, it only saves the rating and exits at once, with no window.
//!   Clicking the notification body opens `zeppbridge://open`, a normal start (the single-instance plugin brings the open window to the front).
//! - Other platforms: a plain notification with only a title and body. Open the app to rate.
//!
//! The UI builds the text in the UI language and passes it in. The backend has no text of its own.

use std::time::Duration;

use serde::Deserialize;
use zeppbridge_core::storage::feedback_toast::{parse_feedback_link, ToastRating};
use zeppbridge_core::storage::write_lock::{self, WritePurpose};
use zeppbridge_core::storage::Database;

use crate::paths;

/// Prefix of the notification button links. `main.rs` uses it to detect a start that only saves a rating.
pub const FEEDBACK_LINK_PREFIX: &str = "zeppbridge://feedback";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackToastText {
    pub token: String,
    pub title: String,
    pub body: String,
    /// Five levels, from "too easy" to "too hard".
    pub labels: Vec<String>,
}

/// Started from a notification button: save the rating and stop. Best effort: a bad token or a busy library only means nothing is written.
pub fn handle_feedback_link(link: &str) {
    let Some((token, rating)) = parse_feedback_link(link) else {
        return;
    };
    let Ok(data_dir) = paths::resolve_data_dir() else {
        return;
    };
    let Ok(_guard) = write_lock::acquire_with_timeout(
        &data_dir,
        WritePurpose::Metadata,
        Duration::from_secs(20),
    ) else {
        return;
    };
    let Ok(db) = Database::open_without_migration(data_dir.join("zepp.db")) else {
        return;
    };
    if let Ok(ToastRating::Saved) = db.rate_from_toast(&token, rating, chrono::Utc::now()) {
        crate::diagnostics::init(Some(&data_dir));
        crate::diagnostics::log("Post-workout rating: a notification button saved a rating");
    }
}

#[cfg(windows)]
pub fn show(text: &FeedbackToastText) -> Result<(), String> {
    windows_toast::show(text)
}

#[cfg(not(windows))]
pub fn show(text: &FeedbackToastText) -> Result<(), String> {
    notify_rust::Notification::new()
        .appname("ZeppBridge")
        .summary(&text.title)
        .body(&text.body)
        .show()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
mod windows_toast {
    use super::FeedbackToastText;
    use windows::core::HSTRING;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};

    /// Matches the AppUserModelID on the installed shortcut (the identifier in tauri.conf.json).
    const AUMID: &str = "com.zeppbridge.app";
    const ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");

    pub fn show(text: &FeedbackToastText) -> Result<(), String> {
        if text.labels.len() != 5 || text.token.len() != 32 {
            return Err("invalid toast text".into());
        }
        register()?;
        let xml = toast_xml(text);
        let document = XmlDocument::new().map_err(|e| e.to_string())?;
        document
            .LoadXml(&HSTRING::from(xml))
            .map_err(|e| e.to_string())?;
        let toast =
            ToastNotification::CreateToastNotification(&document).map_err(|e| e.to_string())?;
        // A new notification with the same tag replaces the old one: at most one card in the notification center.
        toast
            .SetTag(&HSTRING::from("workout-feedback"))
            .map_err(|e| e.to_string())?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))
            .and_then(|notifier| notifier.Show(&toast))
            .map_err(|e| e.to_string())
    }

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    pub(super) fn toast_xml(text: &FeedbackToastText) -> String {
        let actions: String = text
            .labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                format!(
                    r#"<action content="{}" activationType="protocol" arguments="{}"/>"#,
                    escape(label),
                    escape(&format!(
                        "{}?t={}&r={}",
                        super::FEEDBACK_LINK_PREFIX,
                        text.token,
                        index + 1
                    ))
                )
            })
            .collect();
        format!(
            r#"<toast activationType="protocol" launch="zeppbridge://open"><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><actions>{actions}</actions></toast>"#,
            escape(&text.title),
            escape(&text.body)
        )
    }

    /// The portable build has no shortcut with an AppUserModelID: register a name and icon for the current user, or the system does not show the notification.
    /// Also register the `zeppbridge:` link to point at this exe (a portable exe can move, so rewrite it for each notification).
    /// The uninstaller removes both (windows/hooks.nsh).
    fn register() -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let data_dir = crate::paths::resolve_data_dir().map_err(|e| e.to_string())?;
        let icon = data_dir.join("notify-icon.png");
        if !icon.exists() {
            std::fs::write(&icon, ICON_PNG).map_err(|e| e.to_string())?;
        }
        let aumid_key = format!(r"Software\Classes\AppUserModelId\{AUMID}");
        set_value(&aumid_key, Some("DisplayName"), "ZeppBridge")?;
        set_value(&aumid_key, Some("IconUri"), &icon.to_string_lossy())?;
        set_value(r"Software\Classes\zeppbridge", None, "URL:ZeppBridge")?;
        set_value(r"Software\Classes\zeppbridge", Some("URL Protocol"), "")?;
        set_value(
            r"Software\Classes\zeppbridge\shell\open\command",
            None,
            &format!("\"{}\" \"%1\"", exe.display()),
        )
    }

    fn set_value(key: &str, name: Option<&str>, value: &str) -> Result<(), String> {
        use windows_sys::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
        let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let key = wide(key);
        let name = name.map(wide);
        let data = wide(value);
        // SAFETY: all buffers are UTF-16 ending in 0, lengths are passed in bytes, and they stay alive during the call.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(format!("registry write failed: {status}"))
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn toast_xml_escapes_text_and_carries_one_link_per_level() {
        let text = FeedbackToastText {
            token: "a".repeat(32),
            title: "Yesterday's <interval> run & feel".into(),
            body: "Tempo".into(),
            labels: ["1", "2", "3", "4", "\"5\""].map(String::from).to_vec(),
        };
        let xml = windows_toast::toast_xml(&text);
        assert!(xml.contains("&lt;interval&gt; run &amp; feel"));
        assert!(xml.contains("&quot;5&quot;"));
        for rating in 1..=5 {
            let link = format!("zeppbridge://feedback?t={}&amp;r={rating}", "a".repeat(32));
            assert!(xml.contains(&link), "{link}");
        }
        windows::Data::Xml::Dom::XmlDocument::new()
            .unwrap()
            .LoadXml(&windows::core::HSTRING::from(xml))
            .expect("well-formed toast XML");
    }
}

/// Sends a real notification, checks that the system accepted it (it is in the notification history), then removes it at once. It flashes on screen, so it does not run by default:
/// `cargo test -p zeppbridge --lib -- --ignored real_toast`.
#[cfg(all(test, windows))]
#[test]
#[ignore]
fn real_toast_is_accepted_by_windows() {
    use windows::core::HSTRING;
    use windows::UI::Notifications::ToastNotificationManager;
    let text = FeedbackToastText {
        token: "0".repeat(32),
        title: "ZeppBridge".into(),
        body: "test".into(),
        labels: ["1", "2", "3", "4", "5"].map(String::from).to_vec(),
    };
    show(&text).expect("toast shown");
    let history = ToastNotificationManager::History().unwrap();
    let aumid = HSTRING::from("com.zeppbridge.app");
    let count = history.GetHistoryWithId(&aumid).unwrap().Size().unwrap();
    history.ClearWithId(&aumid).unwrap();
    assert!(count >= 1, "Windows dropped the toast");
}
