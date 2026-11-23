//! System features for the handoff to AI: put files on the clipboard as files, read the foreground window title, press keys for the user.
//!
//! Why no browser extension (Safari, Edge, Arc and small browsers all differ, and one more extension is too much work for users):
//! pasting files copied in a file manager into a web page works in Chrome 66+, Edge 79+, Firefox 63+ and Safari 13.1+
//! (`clipboardData.files` on the paste event). So when the `.md` is on the clipboard as a file,
//! the user presses Ctrl+V (Cmd+V on Mac) in the chat box of any browser and it is attached. Nothing to install.
//!
//! - Windows: writes `CF_HDROP` + `Preferred DropEffect` (copy, not cut) itself. Reading the foreground title and pressing keys also exist only here.
//! - Mac / Linux: uses arboard, which is already in the dependency tree (Mac writes `NSURL`, like a file copied in Finder; Linux writes `text/uri-list` on X11 / Wayland).
//!   Automatic key presses are not done on these systems (Mac needs Accessibility permission), and the UI hides that switch.
//!   If it fails (for example with no graphical session), it returns an error and the UI falls back to "drag the file into the chat".

use std::path::Path;

#[derive(Debug)]
pub enum ClipboardError {
    /// No clipboard on this system or session (Mac or Linux without a graphical session). Never reached on Windows.
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    Busy,
}

#[cfg(windows)]
mod imp {
    use super::ClipboardError;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{GlobalFree, HWND};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_RETURN, VK_V,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};

    const CF_UNICODETEXT: u32 = 13;
    const CF_HDROP: u32 = 15;
    const DROPEFFECT_COPY: u32 = 1;
    /// Size of `DROPFILES`: pFiles(u32) + POINT(2 x i32) + fNC(BOOL) + fWide(BOOL).
    const DROPFILES_SIZE: usize = 20;

    /// A block of global memory for the clipboard. After hand-off the system owns it. If not handed off, we free it.
    fn global_bytes(bytes: &[u8]) -> Result<*mut core::ffi::c_void, ClipboardError> {
        // SAFETY: allocate, lock, copy the whole block and unlock all happen in this function. The length is the length of bytes.
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if handle.is_null() {
                return Err(ClipboardError::Busy);
            }
            let target = GlobalLock(handle);
            if target.is_null() {
                GlobalFree(handle);
                return Err(ClipboardError::Busy);
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
            GlobalUnlock(handle);
            Ok(handle)
        }
    }

    /// Opens the clipboard, empties it and puts in several formats. If another program holds the clipboard, retries a few times after a short wait.
    fn put(entries: &[(u32, Vec<u8>)]) -> Result<(), ClipboardError> {
        let mut opened = false;
        for _ in 0..10 {
            // SAFETY: opens the clipboard without a window and closes it below.
            if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        if !opened {
            return Err(ClipboardError::Busy);
        }
        let result = (|| {
            // SAFETY: this thread has the clipboard open.
            if unsafe { EmptyClipboard() } == 0 {
                return Err(ClipboardError::Busy);
            }
            for (format, bytes) in entries {
                let handle = global_bytes(bytes)?;
                // SAFETY: on success the system owns the memory. On failure we still own it and free it.
                if unsafe { SetClipboardData(*format, handle) }.is_null() {
                    unsafe { GlobalFree(handle) };
                    return Err(ClipboardError::Busy);
                }
            }
            Ok(())
        })();
        // SAFETY: pairs with OpenClipboard above.
        unsafe { CloseClipboard() };
        result
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn u16_bytes(units: &[u16]) -> Vec<u8> {
        units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
    }

    pub fn copy_files(paths: &[&Path]) -> Result<(), ClipboardError> {
        // DROPFILES header + each path (wide chars, each ending in 0) + one more 0 (the path list ends with an empty string).
        let mut drop = Vec::with_capacity(DROPFILES_SIZE + 512 * paths.len());
        drop.extend_from_slice(&(DROPFILES_SIZE as u32).to_le_bytes());
        drop.extend_from_slice(&[0u8; 12]);
        drop.extend_from_slice(&1u32.to_le_bytes());
        let mut units: Vec<u16> = Vec::new();
        for path in paths {
            units.extend(path.as_os_str().encode_wide());
            units.push(0);
        }
        units.push(0);
        drop.extend_from_slice(&u16_bytes(&units));
        // "Copy", not "cut": if the user pastes into Explorer, the original file must not move.
        // SAFETY: passes a wide string that ends in 0.
        let effect = unsafe { RegisterClipboardFormatW(wide("Preferred DropEffect").as_ptr()) };
        let mut entries = vec![(CF_HDROP, drop)];
        if effect != 0 {
            entries.push((effect, DROPEFFECT_COPY.to_le_bytes().to_vec()));
        }
        put(&entries)
    }

    pub fn copy_text(text: &str) -> Result<(), ClipboardError> {
        put(&[(CF_UNICODETEXT, u16_bytes(&wide(text)))])
    }

    pub fn foreground_window() -> Option<isize> {
        // SAFETY: only reads the foreground window handle.
        let hwnd: HWND = unsafe { GetForegroundWindow() };
        (!hwnd.is_null()).then_some(hwnd as isize)
    }

    pub fn foreground_title() -> Option<String> {
        // SAFETY: only reads the foreground window title. The buffer length is passed correctly.
        unsafe {
            let hwnd: HWND = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut buffer = [0u16; 512];
            let len = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
            (len > 0).then(|| String::from_utf16_lossy(&buffer[..len as usize]))
        }
    }

    pub fn escape_down() -> bool {
        // SAFETY: only reads the key state. The top bit means the key is down now.
        (unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE)) } as u16 & 0x8000) != 0
    }

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send(inputs: &[INPUT]) -> bool {
        // SAFETY: inputs is one contiguous run of INPUT, and the size is passed correctly.
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        sent as usize == inputs.len()
    }

    pub fn press_paste() -> bool {
        send(&[
            key(VK_CONTROL, false),
            key(VK_V, false),
            key(VK_V, true),
            key(VK_CONTROL, true),
        ])
    }

    pub fn press_enter() -> bool {
        send(&[key(VK_RETURN, false), key(VK_RETURN, true)])
    }
}

#[cfg(not(windows))]
mod imp {
    use super::ClipboardError;
    use std::path::Path;
    use std::sync::Mutex;

    /// On Linux the process that owns the clipboard content serves it: once `Clipboard` drops, paste gets nothing.
    /// So one instance stays alive while the app runs, and the content is there whenever the user presses Ctrl+V.
    static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

    fn with(
        put: impl FnOnce(&mut arboard::Clipboard) -> Result<(), arboard::Error>,
    ) -> Result<(), ClipboardError> {
        let mut guard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            *guard = Some(arboard::Clipboard::new().map_err(|_| ClipboardError::Unsupported)?);
        }
        let clipboard = guard.as_mut().ok_or(ClipboardError::Unsupported)?;
        put(clipboard).map_err(|_| ClipboardError::Busy)
    }

    pub fn copy_files(paths: &[&Path]) -> Result<(), ClipboardError> {
        with(|clipboard| clipboard.set().file_list(paths))
    }
}

pub fn copy_file(path: &Path) -> Result<(), ClipboardError> {
    imp::copy_files(&[path])
}

/// Puts several files at once (for AI handoff: the `.md` and the full track). Pasting into the chat gives that many attachments.
pub fn copy_files(paths: &[&Path]) -> Result<(), ClipboardError> {
    imp::copy_files(paths)
}

// The items below are only for full automation, which is Windows only (`auto` in commands/ai_handoff.rs).
#[cfg(windows)]
pub use imp::{
    copy_text, escape_down, foreground_title, foreground_window, press_enter, press_paste,
};
