//! The last step of the AI handoff: the `.md` goes onto the system clipboard as a "file", so Ctrl+V attaches it in any browser.
//! With "full auto" on (Windows only), it waits for the AI web page to come to the front, then pastes the file and the opening message and presses Enter for the user.
//!
//! Each full-auto step reads the page UI tree (`crate::web_composer`) before it acts: paste only when the input box has focus, count the file as attached only when the attachment card shows,
//! and count the message as sent only when the opening message leaves the input box. If the user presses Esc, switches windows, or the page never comes, stop. The file stays on the clipboard for the user to paste.
//! When in doubt, stop on the safe side: do not press Enter for the user if the attachment is not confirmed.

use crate::app_state::AppState;
use crate::clipboard_files::{self, ClipboardError};
use crate::ipc_error::AppError;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::AppHandle;

/// Size limit for the handed-off `.md` (it is trimmed to the token budget, so it is normally far below this).
const MAX_HANDOFF_BYTES: u64 = 16 * 1024 * 1024;

/// Title keywords that identify the AI web pages (case-sensitive: before the page loads, the title is the lowercase URL, which does not count).
const TITLES: &[(&str, &[&str])] = &[
    ("chatgpt", &["ChatGPT"]),
    ("claude", &["Claude"]),
    ("gemini", &["Gemini"]),
    ("kimi", &["Kimi"]),
    // "\u{8c46}\u{5305}" is the Doubao name in Chinese, as it shows in the browser window title.
    ("doubao", &["\u{8c46}\u{5305}", "Doubao"]),
    ("deepseek", &["DeepSeek"]),
    ("grok", &["Grok"]),
    ("qwen", &["Qwen"]),
];

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Put the `.md` on the clipboard together with the full track next to it. The track is written only when "precise route" is on at "detailed" level
/// (ai_tasks/export/route_gpx.rs in zeppbridge_core). The same function derives the file name, so it is not passed separately.
fn copy_handoff(md: &Path) -> Result<(), ClipboardError> {
    let route = md
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| md.with_file_name(zeppbridge_core::ai_tasks::export::route_file_name(name)))
        .filter(|path| path.is_file());
    match route {
        Some(route) => clipboard_files::copy_files(&[md, route.as_path()]),
        None => clipboard_files::copy_file(md),
    }
}

/// Hand off only the kind of file that `ai_task_prepare` writes: a `.md` in the output directory.
fn handoff_file(state: &AppState, raw: &str) -> Result<PathBuf, AppError> {
    let missing = || AppError::new("err.ai_handoff.file", "Cannot find the file to hand off");
    let path = Path::new(raw).canonicalize().map_err(|_| missing())?;
    let is_md = path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
    let roots = [
        directories::UserDirs::new()
            .and_then(|dirs| dirs.desktop_dir().map(|path| path.join("ZeppBridge AI"))),
        Some(state.data_dir.join("exports").join("ai-tasks")),
    ];
    let inside = roots
        .iter()
        .flatten()
        .filter_map(|root| root.canonicalize().ok())
        .any(|root| path.starts_with(root));
    let size = std::fs::metadata(&path).map_err(|_| missing())?.len();
    if !is_md || !inside || size > MAX_HANDOFF_BYTES {
        return Err(missing());
    }
    Ok(path)
}

fn clipboard_error(error: ClipboardError) -> AppError {
    match error {
        ClipboardError::Unsupported => AppError::new(
            "err.ai_handoff.unsupported",
            "This system cannot put files on the clipboard yet",
        ),
        ClipboardError::Busy => AppError::new(
            "err.ai_handoff.clipboard",
            "Another program is using the clipboard. Try again later",
        ),
    }
}

/// Puts the handed-off `.md` on the clipboard as a "file" (same as Ctrl+C in File Explorer).
#[tauri::command]
pub async fn ai_handoff_copy_file(
    state: tauri::State<'_, AppState>,
    md_path: String,
) -> std::result::Result<(), AppError> {
    let path = handoff_file(&state, &md_path)?;
    copy_handoff(&path).map_err(clipboard_error)
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHandoffAutoRequest {
    provider: String,
    md_path: String,
    kickoff: String,
    send: bool,
}

/// Full auto: put the file on the clipboard first, wait for the AI web page to come to the front, then paste and press Enter for the user. Returns this run's ID at once. Progress goes through the event
/// `ai-handoff://auto`. A second call (or `ai_handoff_cancel`) replaces the run that is still in progress.
#[tauri::command]
pub async fn ai_handoff_autopaste(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: AiHandoffAutoRequest,
) -> std::result::Result<u64, AppError> {
    let titles = TITLES
        .iter()
        .find(|(id, _)| *id == request.provider)
        .map(|(_, titles)| *titles)
        .ok_or_else(|| AppError::new("err.ai_handoff.provider", "The target AI is not in the allowlist"))?;
    let path = handoff_file(&state, &request.md_path)?;
    #[cfg(windows)]
    {
        copy_handoff(&path).map_err(clipboard_error)?;
        let ticket = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        auto::spawn(app, ticket, titles, path, request.kickoff, request.send);
        Ok(ticket)
    }
    // Only Windows can press keys for the user. Other systems do not show this switch in the UI.
    #[cfg(not(windows))]
    {
        let _ = (app, titles, path, request.kickoff, request.send);
        Err(clipboard_error(ClipboardError::Unsupported))
    }
}

/// Cancels the run that is still in progress (the user clicked cancel in the UI or switched to another AI).
#[tauri::command]
pub fn ai_handoff_cancel() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

#[cfg(windows)]
mod auto {
    use super::copy_handoff as copy_file;
    use super::GENERATION;
    use crate::clipboard_files::{
        copy_text, escape_down, foreground_title, foreground_window, press_enter, press_paste,
    };
    use crate::web_composer::Composer;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};
    use tauri::{AppHandle, Emitter};

    const AUTO_EVENT: &str = "ai-handoff://auto";
    /// Wait for the AI web page to come to the front: browser cold start plus first screen. 45 seconds is enough.
    const WAIT_FOREGROUND: Duration = Duration::from_secs(45);
    /// After the page is in front, wait for it to mount the input box and accept focus.
    const WAIT_INPUT: Duration = Duration::from_secs(20);
    /// When the input box first shows, the page is often not "live" yet (the server sent the page, but scripts still run). Wait a bit more before the paste.
    const WARM_UP: Duration = Duration::from_millis(1200);
    /// After the file paste, wait for the attachment card. If it does not show, that paste missed (in tests, ChatGPT needed several more seconds after the input box showed
    /// before the page really accepted a paste), so paste again. A missed paste leaves nothing. But Grok sometimes takes over 4 seconds to draw the card, and a short wait
    /// gives two copies, so the wait is 7 seconds.
    const WAIT_ATTACH: Duration = Duration::from_secs(7);
    const FILE_TRIES: usize = 4;
    /// When focus is not on the input box before a paste, the maximum time to wait for it to come back.
    const REFOCUS: Duration = Duration::from_secs(3);
    const WAIT_TEXT: Duration = Duration::from_secs(3);
    /// After Enter, wait for the message to be sent (the opening message leaves the input box; on sites where the input box cannot be read, the opening message shows in the conversation).
    /// The site blocks Enter while an upload is not done, so press it again after a short time, for at most this long.
    const WAIT_SENT: Duration = Duration::from_secs(60);
    const POLL: Duration = Duration::from_millis(250);

    #[derive(Debug, Clone, serde::Serialize)]
    struct AutoStage {
        ticket: u64,
        /// waiting -> file -> sent / filled. A stop halfway is aborted (Esc, window switch, attachment not confirmed) or timeout.
        stage: &'static str,
    }

    pub(super) fn spawn(
        app: AppHandle,
        ticket: u64,
        titles: &'static [&'static str],
        path: PathBuf,
        kickoff: String,
        send: bool,
    ) {
        std::thread::spawn(move || {
            let emit = |stage| {
                let _ = app.emit(AUTO_EVENT, AutoStage { ticket, stage });
            };
            let stage = run(ticket, titles, &path, &kickoff, send, &emit);
            // Whatever step it stops at, put the file back on the clipboard, so the user can paste it.
            if stage != "superseded" {
                let _ = copy_file(&path);
                emit(stage);
            }
        });
    }

    /// Used to match the file name on the attachment card: the full file stem (it has a timestamp, so it shows nowhere else on the page). A match on only the first part
    /// can be fooled by old conversation titles in the sidebar. In tests, ChatGPT did this and counted a missing attachment as attached.
    fn file_needle(path: &Path) -> String {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string()
    }

    /// Remove whitespace when comparing the opening message (line breaks and spaces change in the page).
    fn squash(text: &str) -> String {
        text.chars().filter(|c| !c.is_whitespace()).collect()
    }

    fn run(
        ticket: u64,
        titles: &[&str],
        path: &Path,
        kickoff: &str,
        send: bool,
        emit: &dyn Fn(&'static str),
    ) -> &'static str {
        let live = || GENERATION.load(Ordering::SeqCst) == ticket;
        emit("waiting");

        // 1. Wait for the AI web page to come to the front (title matches), and record that window.
        let deadline = Instant::now() + WAIT_FOREGROUND;
        let hwnd = loop {
            if !live() {
                return "superseded";
            }
            if escape_down() {
                return "aborted";
            }
            if Instant::now() > deadline {
                return "timeout";
            }
            let titled = foreground_title().is_some_and(|t| titles.iter().any(|k| t.contains(k)));
            if let (true, Some(hwnd)) = (titled, foreground_window()) {
                break hwnd;
            }
            std::thread::sleep(POLL);
        };
        let Some(page) = Composer::attach(hwnd) else {
            return "aborted";
        };

        // For each wait after this: stop at once on replacement, Esc, or window switch. A switch to another tab in the same browser window
        // also counts as a switch (in tests, the user switched to X to write a post): the window title follows the tab, so stop when the title no longer matches.
        let in_page = || {
            foreground_window() == Some(hwnd)
                && foreground_title().is_some_and(|t| titles.iter().any(|k| t.contains(k)))
        };
        let stopped = || -> Option<&'static str> {
            if !live() {
                Some("superseded")
            } else if escape_down() || !in_page() {
                Some("aborted")
            } else {
                None
            }
        };
        let wait_for =
            |limit: Duration, ok: &mut dyn FnMut() -> bool| -> Result<bool, &'static str> {
                let end = Instant::now() + limit;
                loop {
                    if let Some(stop) = stopped() {
                        return Err(stop);
                    }
                    if ok() {
                        return Ok(true);
                    }
                    if Instant::now() > end {
                        return Ok(false);
                    }
                    std::thread::sleep(POLL);
                }
            };

        // 2. Wait for the input box in the page to accept focus, then wait a bit more.
        match wait_for(WAIT_INPUT, &mut || page.focus()) {
            Err(stop) => return stop,
            Ok(false) => return "aborted",
            Ok(true) => {}
        }
        if let Err(stop) = wait_for(WARM_UP, &mut || false) {
            return stop;
        }

        // 3. Paste the file, and wait for the attachment card (with the file name) to show.
        let needle = file_needle(path);
        let mut attached = false;
        for _ in 0..FILE_TRIES {
            if let Some(stop) = stopped() {
                return stop;
            }
            // When the attachment card or a popup takes focus, wait for focus to come back first. Do not use up all retries in one moment.
            if let Err(stop) = wait_for(REFOCUS, &mut || page.focus()) {
                return stop;
            }
            if !page.focus() || copy_file(path).is_err() || !press_paste() {
                continue;
            }
            match wait_for(WAIT_ATTACH, &mut || page.shows(&needle)) {
                Err(stop) => return stop,
                Ok(true) => {
                    attached = true;
                    break;
                }
                Ok(false) => {}
            }
        }
        if !attached {
            // Attachment not confirmed: do no more for the user. The file stays on the clipboard, and the user gets a hint to press Ctrl+V.
            return "aborted";
        }
        emit("file");

        // 4. Paste the opening message and confirm that it is in the input box. First check if this site's input box can be read. If it can (most sites), the message must be seen
        //    in the box, and if not, paste again (Grok once counted a failed read as pasted, and sent only the attachment).
        //    If the box cannot be read at all (Doubao), paste blind only once, so it is not pasted twice.
        let head: String = squash(kickoff).chars().take(8).collect();
        let readable = match wait_for(Duration::from_secs(1), &mut || page.text().is_some()) {
            Err(stop) => return stop,
            Ok(readable) => readable,
        };
        if !head.is_empty() {
            let mut typed = false;
            for _ in 0..2 {
                if let Some(stop) = stopped() {
                    return stop;
                }
                if let Err(stop) = wait_for(REFOCUS, &mut || page.focus()) {
                    return stop;
                }
                if !page.focus() || copy_text(kickoff).is_err() || !press_paste() {
                    continue;
                }
                let landed = &mut || match page.text() {
                    Some(text) => squash(&text).contains(&head),
                    None => !readable,
                };
                match wait_for(WAIT_TEXT, landed) {
                    Err(stop) => return stop,
                    Ok(true) => {
                        typed = true;
                        break;
                    }
                    Ok(false) => {}
                }
            }
            let _ = copy_file(path);
            if !typed {
                return "aborted";
            }
        }
        if !send {
            return "filled";
        }

        // 5. Press Enter until the opening message leaves the input box. The site blocks Enter while an upload is not done, so press again after a short time.
        if head.is_empty() {
            return if page.focus() && press_enter() {
                "sent"
            } else {
                "filled"
            };
        }
        let bubble: String = kickoff.trim().chars().take(12).collect();
        let end = Instant::now() + WAIT_SENT;
        loop {
            if let Some(stop) = stopped() {
                return stop;
            }
            if page.focus() {
                press_enter();
            }
            // If the input box can be read, the message is sent when it leaves the box. If not (Doubao), it counts only when the message shows in the conversation.
            let gone = &mut || match page.text() {
                Some(text) => !squash(&text).contains(&head),
                None => page.shows(&bubble),
            };
            match wait_for(Duration::from_secs(2), gone) {
                // After the send, sites often change the title (ChatGPT uses the conversation name), which looks like a "window switch". Before stopping, check once more if the message was sent.
                Err(stop) => return if gone() { "sent" } else { stop },
                Ok(true) => return "sent",
                Ok(false) if Instant::now() > end => return "filled",
                Ok(false) => {}
            }
        }
    }
}
