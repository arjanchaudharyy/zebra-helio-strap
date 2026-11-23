//! App self-update: whether it can update, whether it is the portable build, and a data location check before replacement.
//! The silent download and replace flow is in [`silent`]. The portable in-place file swap is in [`portable`].

use crate::ipc_error::AppError;

pub(crate) mod portable;
pub(crate) mod silent;

pub(crate) use silent::SilentUpdater;

/// Check before replacement: if the app bundle holds a custom data directory, an update replaces the bundle and the data together. Do not install in that case.
pub(crate) fn validate_data_location(data_dir: &std::path::Path) -> Result<(), AppError> {
    zeppbridge_core::paths::validate_update_data_location(data_dir, &std::env::current_exe()?)
        .map_err(|error| {
            // The low-level English reason with the path was once dropped, and only fixed text remained.
            // That left nothing to debug "why did it refuse to install".
            crate::diagnostics::log(&format!("Pre-update data location check failed: {error}"));
            AppError::new(
                "err.update.unsafe_data_location",
                "Cannot confirm that the data directory is safe to keep. Update install stopped.",
            )
        })
}

#[cfg(any(windows, test))]
use std::path::{Path, PathBuf};

/// Whether this build can update itself.
///
/// On Linux, someone else manages updates for every distribution channel: Flatpak uses `flatpak update`,
/// deb/rpm use the distro package manager. The install prefix (`/app`, `/usr/bin`) is
/// read-only for the app process. Even if it were writable, writing there would conflict with the package manager's records.
///
/// Why a switch, and not a direct `check()` call from the frontend: latest.json has no
/// linux entry, so `check()` throws "platforms in the response has no linux-x86_64".
/// That message shows on the settings page as "update failed", but nothing is broken and the user
/// can do nothing. Red text with no clear reason is worse than no check.
///
/// macOS is limited to aarch64 for the same reason: CI only builds `ZeppBridge_*_aarch64.app.tar.gz`
/// (the `macos-latest` runner is arm64). An x86_64 mac that calls `check()` also
/// hits the false failure "platforms has no darwin-x86_64".
pub(crate) fn self_update_supported() -> bool {
    // On since v3: it was off in beta because the old flow turned the portable build into an installed build in
    // `%LOCALAPPDATA%` and launched that copy. Now the portable build updates in place (portable.rs), so that path is gone. The endpoint is still this
    // repo's releases/latest: all 2.x versions are lower than 3.0, so a test build is never "updated" back to 2.x.
    let platform = cfg!(all(windows, target_arch = "x86_64"))
        || cfg!(all(target_os = "macos", target_arch = "aarch64"));
    // Do not touch dev builds that run from the cargo build cache: a portable swap would replace the build output.
    let built_here = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.parent()
                .map(zeppbridge_core::paths::is_build_artifact_dir)
        })
        .unwrap_or(false);
    platform && !built_here
}

/// Whether this exe on Windows is the portable build.
///
/// MSI installs to Program Files, NSIS installs to `%LOCALAPPDATA%\ZeppBridge\`. Neither of these
/// is portable. Any other path (`release\ZeppBridge.exe` next to `data\`) is portable.
#[cfg(any(windows, test))]
pub(crate) fn looks_like_portable_install(
    current: &Path,
    localappdata: &Path,
    program_files_roots: &[PathBuf],
) -> bool {
    if path_is_under(current, &localappdata.join("ZeppBridge")) {
        return false;
    }
    for root in program_files_roots {
        if path_is_under(current, root) {
            return false;
        }
    }
    true
}

#[cfg(any(windows, test))]
fn path_is_under(path: &Path, root: &Path) -> bool {
    let path = normalize_windows_path(path);
    let mut root = normalize_windows_path(root);
    if root.is_empty() {
        return false;
    }
    if !root.ends_with('\\') {
        root.push('\\');
    }
    path == root.trim_end_matches('\\') || path.starts_with(&root)
}

#[cfg(any(windows, test))]
fn normalize_windows_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn program_files_roots() -> Vec<PathBuf> {
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
        .into_iter()
        .filter_map(|key| std::env::var_os(key).map(PathBuf::from))
        .collect()
}

pub(crate) fn is_portable_update() -> Result<bool, AppError> {
    #[cfg(windows)]
    {
        let current = std::env::current_exe()?;
        let local = std::env::var_os("LOCALAPPDATA").ok_or_else(|| {
            AppError::new(
                "err.update.localappdata_missing",
                "Windows LOCALAPPDATA path is not available",
            )
        })?;
        Ok(looks_like_portable_install(
            &current,
            Path::new(&local),
            &program_files_roots(),
        ))
    }
    // macOS/.app and other platforms are never portable builds.
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::looks_like_portable_install;
    use std::path::{Path, PathBuf};

    fn roots() -> Vec<PathBuf> {
        vec![
            PathBuf::from(r"C:\Program Files"),
            PathBuf::from(r"C:\Program Files (x86)"),
        ]
    }

    #[test]
    fn nsis_localappdata_is_not_portable() {
        assert!(!looks_like_portable_install(
            Path::new(r"C:\Users\me\AppData\Local\ZeppBridge\ZeppBridge.exe"),
            Path::new(r"C:\Users\me\AppData\Local"),
            &roots(),
        ));
    }

    #[test]
    fn msi_program_files_is_not_portable() {
        assert!(!looks_like_portable_install(
            Path::new(r"C:\Program Files\ZeppBridge\ZeppBridge.exe"),
            Path::new(r"C:\Users\me\AppData\Local"),
            &roots(),
        ));
        assert!(!looks_like_portable_install(
            Path::new(r"C:\Program Files (x86)\ZeppBridge\ZeppBridge.exe"),
            Path::new(r"C:\Users\me\AppData\Local"),
            &roots(),
        ));
    }

    #[test]
    fn program_files_x86_does_not_match_program_files_prefix() {
        // A string prefix check alone would wrongly match `(x86)` as `Program Files\`.
        let only_64 = [PathBuf::from(r"C:\Program Files")];
        assert!(looks_like_portable_install(
            Path::new(r"C:\Program Files (x86)\Other\app.exe"),
            Path::new(r"C:\Users\me\AppData\Local"),
            &only_64,
        ));
    }

    #[test]
    fn release_next_to_data_is_portable() {
        assert!(looks_like_portable_install(
            Path::new(r"C:\Users\me\Desktop\MyProject\ZeppBridge\release\ZeppBridge.exe"),
            Path::new(r"C:\Users\me\AppData\Local"),
            &roots(),
        ));
    }
}
