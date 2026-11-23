//! Links and folders handed to the system to open (system browser, file manager).
//!
//! The extra step is only for AppImage. Before the AppImage runtime starts us, it points variables such as `LD_LIBRARY_PATH`,
//! `GIO_MODULE_DIR`, `GDK_PIXBUF_MODULE_FILE` and `GTK_PATH` into the mount folder,
//! and puts the mount folder at the front of `PATH` and `XDG_DATA_DIRS`. A browser started by `xdg-open` inherits them unchanged,
//! loads our bundled old GTK / GIO modules, and exits silently. But `xdg-open` itself returns success,
//! so the app thinks the browser opened and the UI stays on "Waiting for browser authorization" (Linux AppImage: sign-in did not open the browser).
//!
//! So inside an AppImage we call `xdg-open` ourselves, after we restore these variables to the host values.
//!
//! deb and Flatpak also call it ourselves (in deb, Flatpak and AppImage, "Connect" did not open the browser).
//! The opener plugin only starts the open program. It reports success even when that program exits non-zero (no default browser, Flatpak portal refused),
//! so the app waits for an authorization page that never opens. Here we try `xdg-open`, then `gio open`,
//! and check the exit code. If both fail, we report the error, and the UI moves "Copy authorization link" to the front. Other platforms still use the opener plugin.

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

/// Variables set by the AppImage runtime or its bundled GTK / GStreamer / Python hooks: remove them completely.
const APPIMAGE_ONLY_VARS: &[&str] = &[
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "GIO_MODULE_DIR",
    "GIO_EXTRA_MODULES",
    "GDK_PIXBUF_MODULE_FILE",
    "GDK_PIXBUF_MODULEDIR",
    "GTK_PATH",
    "GTK_DATA_PREFIX",
    "GTK_EXE_PREFIX",
    "GTK_IM_MODULE_FILE",
    "GTK_THEME",
    "GSETTINGS_SCHEMA_DIR",
    "GST_PLUGIN_PATH",
    "GST_PLUGIN_PATH_1_0",
    "GST_PLUGIN_SYSTEM_PATH",
    "GST_PLUGIN_SYSTEM_PATH_1_0",
    "GST_PLUGIN_SCANNER",
    "GST_PLUGIN_SCANNER_1_0",
    "PYTHONHOME",
    "PYTHONPATH",
    "PERLLIB",
    "QT_PLUGIN_PATH",
    "APPDIR",
    "APPIMAGE",
    "ARGV0",
    "OWD",
];

/// Colon-separated variables where AppImage put the mount folder in front: remove only the mount folder parts.
const PATH_LIKE_VARS: &[&str] = &["PATH", "XDG_DATA_DIRS", "XDG_CONFIG_DIRS"];

/// Environment changes for the child process: `None` = remove the variable, `Some(value)` = set it to this value.
///
/// A pure function, so it can be tested on any platform. `appdir` is the AppImage mount folder (`APPDIR`).
pub(crate) fn appimage_env_fixes(
    vars: impl IntoIterator<Item = (String, String)>,
    appdir: &str,
) -> Vec<(String, Option<String>)> {
    let appdir = appdir.trim_end_matches('/');
    let mut fixes = Vec::new();
    for (name, value) in vars {
        if APPIMAGE_ONLY_VARS.contains(&name.as_str()) {
            fixes.push((name, None));
        } else if PATH_LIKE_VARS.contains(&name.as_str()) && !appdir.is_empty() {
            let kept: Vec<&str> = value
                .split(':')
                .filter(|part| {
                    !part.is_empty() && *part != appdir && !part.starts_with(&format!("{appdir}/"))
                })
                .collect();
            let joined = kept.join(":");
            if joined != value {
                fixes.push((name, (!joined.is_empty()).then_some(joined)));
            }
        }
    }
    fixes
}

/// Are we inside an AppImage? If yes, returns the mount folder.
fn appimage_dir() -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    std::env::var_os("APPIMAGE")?;
    Some(
        std::env::var("APPDIR")
            .ok()
            .filter(|dir| !dir.is_empty())
            .unwrap_or_default(),
    )
}

/// Open programs to try on Linux, in order. `xdg-open` is the standard (in Flatpak it forwards to the desktop portal).
/// If it is missing or fails, try `gio open`, which GNOME / GTK desktops have.
const LINUX_OPENERS: &[(&str, &[&str])] = &[("xdg-open", &[]), ("gio", &["open"])];

/// The longest wait for the open program to exit. Normally it hands off to the browser and exits within a few hundred milliseconds. If it has not exited by then, it most likely
/// runs the browser in its own process (a fallback path of some `xdg-open` versions), so count it as opened.
const OPENER_EXIT_WAIT: Duration = Duration::from_secs(3);

/// Waits for a result from the open program: exit code 0 is success, non-zero is failure. If it never exits, count it as success and reap it on another thread.
fn wait_for_opener(mut child: Child) -> std::io::Result<()> {
    let deadline = Instant::now() + OPENER_EXIT_WAIT;
    loop {
        if let Some(status) = child.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other(format!(
                    "opener exited with {status}"
                )))
            };
        }
        if Instant::now() >= deadline {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Linux: tries `LINUX_OPENERS` in order. The first success wins. Inside an AppImage, restores the host environment first.
fn linux_open(target: &std::ffi::OsStr) -> std::io::Result<()> {
    let appdir = appimage_dir();
    let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, "no opener available");
    for (program, args) in LINUX_OPENERS {
        let mut command = Command::new(program);
        command
            .args(*args)
            .arg(target)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(appdir) = &appdir {
            for (name, value) in appimage_env_fixes(std::env::vars(), appdir) {
                match value {
                    Some(value) => command.env(name, value),
                    None => command.env_remove(name),
                };
            }
        }
        match command.spawn().and_then(wait_for_opener) {
            Ok(()) => return Ok(()),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// Opens a link in the system browser. Returns an error if it cannot open (on Linux, judged by the open program's exit code).
pub(crate) fn open_url(app: &AppHandle, url: &str) -> Result<(), String> {
    if cfg!(target_os = "linux") {
        return linux_open(url.as_ref()).map_err(|error| error.to_string());
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| error.to_string())
}

/// Opens a folder in the file manager (on Linux with `xdg-open`, then `gio open` if that fails).
#[cfg(target_os = "linux")]
pub(crate) fn open_folder(path: &std::path::Path) -> std::io::Result<()> {
    linux_open(path.as_os_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixes(vars: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
        appimage_env_fixes(
            vars.iter()
                .map(|(name, value)| (name.to_string(), value.to_string())),
            "/tmp/.mount_ZeppBrAbc",
        )
    }

    #[test]
    fn the_browser_gets_the_host_environment_back() {
        let out = fixes(&[
            ("LD_LIBRARY_PATH", "/tmp/.mount_ZeppBrAbc/usr/lib"),
            (
                "GIO_MODULE_DIR",
                "/tmp/.mount_ZeppBrAbc/usr/lib/gio/modules",
            ),
            (
                "PATH",
                "/tmp/.mount_ZeppBrAbc/usr/bin:/usr/local/bin:/usr/bin",
            ),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_ZeppBrAbc/usr/share:/usr/share",
            ),
            ("XDG_CONFIG_DIRS", "/tmp/.mount_ZeppBrAbc/etc/xdg"),
            ("HOME", "/home/me"),
            ("DISPLAY", ":0"),
        ]);
        assert_eq!(
            out,
            [
                ("LD_LIBRARY_PATH".to_string(), None),
                ("GIO_MODULE_DIR".to_string(), None),
                (
                    "PATH".to_string(),
                    Some("/usr/local/bin:/usr/bin".to_string())
                ),
                ("XDG_DATA_DIRS".to_string(), Some("/usr/share".to_string())),
                ("XDG_CONFIG_DIRS".to_string(), None),
            ]
        );
    }

    #[test]
    fn a_path_without_the_mount_is_left_alone() {
        assert!(fixes(&[("PATH", "/usr/bin:/bin"), ("LANG", "de_DE.UTF-8")]).is_empty());
        // A path with the same prefix that is not the mount folder itself does not count.
        assert!(fixes(&[("PATH", "/tmp/.mount_ZeppBrAbcdef/bin:/usr/bin")]).is_empty());
    }
}
