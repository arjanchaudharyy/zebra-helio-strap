use std::io::{self, Write};
use std::path::{Path, PathBuf};

mod fresh_start;
#[path = "paths/store.rs"]
mod store;
pub use fresh_start::{apply_pending_fresh_start, request_fresh_start};
pub use store::{
    current_executable_is_windows_apps_alias, current_package_identity_mismatch,
    import_store_library, mark_store_import_reviewed, package_local_data_dir,
    prepare_configured_store_data_dir, prepare_package_local_data_dir,
    recover_interrupted_store_import, require_store_import_decision, store_import_candidates,
    store_import_required, store_import_reviewed, windows_apps_alias_dir,
};

const SQLITE_GROUP: [&str; 3] = ["zepp.db", "zepp.db-wal", "zepp.db-shm"];
const LEGACY_FILES: [&str; 8] = [
    "auth.json",
    "auth.user-id",
    "devices.json",
    "device.json",
    "zeppbridge-ca.cer",
    "restore-pending.json",
    "local-api.json",
    "credentials.json",
];
const LEGACY_OPTIONAL_FILES: [&str; 1] = ["zeppbridge-ca.pem"];
const LEGACY_DIRS: [&str; 2] = ["exports", "backups"];

/// The environment variable that sets the data directory explicitly.
///
/// A container has no "next to the install folder": the image is read-only
/// (or must be treated as read-only), and the user's mounted volume can be at
/// any path. systemd units and NAS task schedulers have the same problem. This
/// one variable makes "where the data is" a deploy-time setting, not a rule to
/// work out.
pub const DATA_DIR_ENV: &str = "ZEPPBRIDGE_DATA_DIR";

/// Install-local data directory: `{exe_dir}/data`.
///
/// `ZEPPBRIDGE_DATA_DIR` overrides everything below when it is set to an
/// absolute path.
///
/// Build-cache binaries (`cargo-target`, rustc `target/`) never own user data.
/// Those fall back to the repository `data/` folder so `tauri dev` does not
/// drop a 1GB library into `G:\build_cache`.
///
/// macOS: `.app` bundles are replaced during updates, so user data lives in
/// the user Application Support
/// directory (`~/Library/Application Support/com.zeppbridge.ZeppBridge/data`).
///
/// Linux: a packaged build is installed into a shared, root-owned prefix
/// (`/usr/bin` for deb/rpm, `/app/bin` inside a Flatpak sandbox). There is no
/// "next to the executable" there, so those go straight to the XDG data
/// directory (`~/.local/share/zeppbridge/data`, which a Flatpak redirects into
/// `~/.var/app/com.zeppbridge.app/`). An AppImage or an unpacked tarball keeps
/// the install-local layout, because for those the folder really is the
/// user's.
pub fn resolve_data_dir() -> io::Result<PathBuf> {
    // A packaged Store process must always use its package's real LocalState.
    // Ignore ZEPPBRIDGE_DATA_DIR here so a sidecar cannot accidentally attach to
    // the GitHub library or a redirected AppData view. Until the desktop app has
    // recorded an import/skip choice, CLI and MCP fail closed instead of
    // creating a fresh empty database on first use.
    #[cfg(feature = "store")]
    {
        if let Some(dir) = prepare_package_local_data_dir()? {
            require_store_import_decision(&dir)?;
            return Ok(dir);
        }

        let explicit = data_dir_from_env()?;
        if let Some(dir) = explicit {
            if prepare_configured_store_data_dir(&dir)? {
                require_store_import_decision(&dir)?;
                return Ok(dir);
            }
            if let Some(family) = current_package_identity_mismatch()? {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!(
                        "Store build expected package family LincStudio.ZeppBridge_pwftgd5x5w3va, but this process has {family}. Only a validated ZeppBridge Store LocalState path may be used."
                    ),
                ));
            }
            if current_executable_is_windows_apps_alias() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Store execution aliases require package identity or their Store LocalState path. Open ZeppBridge from the Start menu or use its generated MCP configuration.",
                ));
            }
            ensure_writable_dir(&dir)?;
            return Ok(dir);
        }
        if let Some(family) = current_package_identity_mismatch()? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "Store build expected package family LincStudio.ZeppBridge_pwftgd5x5w3va, but this process has {family}. Launch it with the Store identity or its validated Store LocalState path."
                ),
            ));
        }
        if current_executable_is_windows_apps_alias() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Store execution aliases require package identity or their Store LocalState path. Open ZeppBridge from the Start menu or use its generated MCP configuration.",
            ));
        }
    }

    #[cfg(not(feature = "store"))]
    if let Some(dir) = data_dir_from_env()? {
        ensure_writable_dir(&dir)?;
        return Ok(dir);
    }

    // The Store build runs with package identity. Keep its user library in the
    // package's LocalState so it is separate from the GitHub portable build and
    // is removed with the package. Unpackaged developer runs still use the
    // ordinary install-local layout below.
    let exe = std::env::current_exe()?;
    let exe_dir = exe.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "executable has no parent directory",
        )
    })?;

    // Pick the location before any write, not "try /usr/bin/data, then fall back".
    // In a container the CLI often runs as root, so that write **succeeds**. The
    // data then goes into the image layer and is lost with the container, while
    // the mounted volume stays empty. Running as root must not move the data.
    #[cfg(all(unix, not(target_os = "macos")))]
    if !is_build_artifact_dir(exe_dir) && is_shared_prefix_dir(exe_dir) {
        let base = user_data_dir()?;
        ensure_writable_dir(&base)?;
        return Ok(base);
    }

    // macOS: `/Applications` is writable for the admin group, so "writable" does
    // not mean "the right place". An update or reinstall replaces the whole
    // `.app` bundle, and data inside it is lost.
    #[cfg(target_os = "macos")]
    if is_inside_app_bundle(exe_dir) {
        let base = user_data_dir()?;
        migrate_bundle_data(&exe_dir.join("data"), &base)?;
        ensure_writable_dir(&base)?;
        return Ok(base);
    }

    let data_dir = if is_build_artifact_dir(exe_dir) {
        repository_data_dir().unwrap_or_else(|| exe_dir.join("data"))
    } else {
        exe_dir.join("data")
    };
    match ensure_writable_dir(&data_dir) {
        Ok(()) => Ok(data_dir),
        Err(error) => fall_back_to_user_data_dir(&data_dir, error),
    }
}

/// "Something in the target folder" does not mean "data in the target folder".
///
/// macOS puts `.DS_Store` into Application Support. The previous app also left
/// entries with no user data, such as `logs/`, `webview/` and half-done
/// `.bundle-migration-*` folders. The old code refused to start on any
/// non-empty folder, and on macOS that failure showed no dialog. Only an entry
/// outside this list (for example another account's `auth.json`) is a reason to stop.
#[cfg(any(target_os = "macos", test))]
fn destination_only_has_ignorable_entries(destination: &Path) -> io::Result<bool> {
    const IGNORABLE: [&str; 5] = [".DS_Store", ".localized", "logs", "webview", ".write-probe"];
    for entry in std::fs::read_dir(destination)? {
        let name = entry?.file_name();
        let Some(name) = name.to_str() else {
            return Ok(false);
        };
        let ignorable = IGNORABLE.contains(&name)
            || name.starts_with(".bundle-migration-")
            || name.starts_with(".pre-migration-")
            || (name.starts_with('.') && name.contains(".tmp-"));
        if !ignorable {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Publish a complete copy before switching libraries. Keep the bundle copy for
/// recovery, and never combine two accounts' databases or credentials.
#[cfg(any(target_os = "macos", test))]
fn migrate_bundle_data(source: &Path, destination: &Path) -> io::Result<()> {
    if !source.is_dir() || destination.join("zepp.db").is_file() {
        return Ok(());
    }
    if destination.exists() && !destination_only_has_ignorable_entries(destination)? {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists,
            "Application Support data is not empty. Back up both data directories before migrating the app-bundle library."));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("missing data parent"))?;
    std::fs::create_dir_all(parent)?;
    // First remove half-done folders from an earlier crash. They hold no committed
    // data (staging is invisible until the final rename). They only take space and
    // block the next check.
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name();
        if name
            .to_str()
            .is_some_and(|name| name.starts_with(".bundle-migration-"))
        {
            let _ = remove_recursive(&entry.path());
        }
    }
    let staging = parent.join(format!(
        ".bundle-migration-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    std::fs::create_dir(&staging)?;
    let staged = (|| -> io::Result<()> {
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            let name = entry.file_name();
            if SQLITE_GROUP.iter().any(|item| name == *item) {
                continue;
            }
            copy_bundle_entry(&entry.path(), &staging.join(name))?;
        }
        if source.join("zepp.db").is_file() {
            match rusqlite::Connection::open_with_flags(
                source.join("zepp.db"),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .and_then(|db| db.backup(rusqlite::DatabaseName::Main, staging.join("zepp.db"), None))
            {
                Ok(()) => {}
                Err(error) => {
                    // The online backup failed (usually a corrupt source database).
                    // A file copy moves the database as is, and the open_resilient
                    // quarantine and recovery flow takes over. That is better than a
                    // startup failure that only says "cannot open".
                    tracing::warn!("bundle migration: SQLite online backup failed ({error}), falling back to a file copy");
                    for name in SQLITE_GROUP {
                        let file = source.join(name);
                        if file.is_file() {
                            std::fs::copy(&file, staging.join(name))?;
                        }
                    }
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = staged {
        // Remove the half-done staging folder. It holds a partial copy. If it
        // stays, it confuses the next startup and more of them pile up.
        let _ = remove_recursive(&staging);
        return Err(error);
    }
    if destination.exists() {
        // Do **not** delete the target, even when only ignorable entries are left.
        // Move it aside: `logs/` can hold the crash log the user wants to send.
        let aside = parent.join(format!(
            ".pre-migration-{}-{}",
            std::process::id(),
            crate::unique_suffix()
        ));
        std::fs::rename(destination, &aside)?;
    }
    std::fs::rename(&staging, destination)?;
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn copy_bundle_entry(source: &Path, destination: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(source)?;
    if meta.file_type().is_symlink() {
        // Older versions allowed symlinks, so a `backups -> external disk` link in
        // the bundle is a real setup. Move the link itself: the target stays where
        // it is, is not copied or consumed, and the new version follows the link.
        #[cfg(unix)]
        {
            let target = std::fs::read_link(source)?;
            return std::os::unix::fs::symlink(&target, destination);
        }
        #[cfg(not(unix))]
        {
            return Err(io::Error::other(
                "Bundle data contains a symbolic link; migrate it manually before updating.",
            ));
        }
    }
    if meta.is_dir() {
        std::fs::create_dir(destination)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_bundle_entry(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(source, destination)?;
    }
    Ok(())
}

/// The fallback when the folder next to the install folder is not writable.
///
/// Before, only unix had this fallback (`/Applications/...` and read-only
/// prefixes). On Windows it is **more common**: an `.msi` installs into
/// `C:\Program Files\ZeppBridge`, where a normal user cannot write. Controlled
/// folder access, group policy and antivirus folder protection also block it.
/// The caller (`setup()` in `lib.rs`) uses `?` on this `Err`, so without a
/// fallback the app **exits silently**: no window, a dead tray icon, no log.
///
/// But the fallback must not be silent either. If the unwritable folder
/// **already holds the user's database**, a new empty folder makes the data look
/// lost. In that case report the error, so the startup dialog shows both paths.
fn fall_back_to_user_data_dir(blocked: &Path, error: io::Error) -> io::Result<PathBuf> {
    if blocked.join(SQLITE_GROUP[0]).exists() {
        return Err(io::Error::new(
            error.kind(),
            format!(
                concat!(
                    "The data directory {} already holds a database, but it cannot be written now ({}). ",
                    "A different directory would make this data look lost, so the app does not pick one. ",
                    "Give this directory write permission, or use {} to set a writable directory."
                ),
                blocked.display(),
                error,
                DATA_DIR_ENV
            ),
        ));
    }
    let base = user_data_dir()?;
    ensure_writable_dir(&base)?;
    Ok(base)
}

/// `ZEPPBRIDGE_DATA_DIR`, validated.
///
/// A relative path is a configuration error. It is not expanded against the
/// current working directory: the working directory of cron or a container
/// entrypoint is not visible to the person who writes the unit file, so the data
/// would move with how the program is started.
fn data_dir_from_env() -> io::Result<Option<PathBuf>> {
    data_dir_from_env_value(std::env::var_os(DATA_DIR_ENV))
}

/// Separate from `data_dir_from_env` only so tests can pass a value.
///
/// `set_var` in a test does not work: cargo runs the tests of one crate in
/// several threads of one process, and environment variables are per process.
/// Such tests fail at random, and the failure looks like a bug in the code under test.
fn data_dir_from_env_value(raw: Option<std::ffi::OsString>) -> io::Result<Option<PathBuf>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let raw = PathBuf::from(raw);
    if raw.as_os_str().is_empty() {
        return Ok(None);
    }
    if !raw.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{DATA_DIR_ENV} must be an absolute path, got: {}", raw.display()),
        ));
    }
    Ok(Some(raw))
}

/// User-writable data directory: `~/Library/Application Support/
/// com.zeppbridge.ZeppBridge/data` on macOS, `~/.local/share/zeppbridge/data`
/// on Linux, `%APPDATA%\zeppbridge\ZeppBridge\data` on Windows. All three keep
/// the same `data/` layout as the install-local folder, so the CLI and MCP
/// tools resolve the same place the app does.
///
/// On Windows this path is also in `legacy_source_dirs()`. So after a fallback
/// to it, `relocate_legacy_data()` does not move it onto itself (it has a
/// `source == data_dir` short circuit).
fn user_data_dir() -> io::Result<PathBuf> {
    let project = directories::ProjectDirs::from("com", "zeppbridge", "ZeppBridge")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cannot determine the user data directory"))?;
    Ok(project.data_dir().join("data"))
}

/// Whether the executable is inside some `*.app/Contents/MacOS`.
#[cfg(any(target_os = "macos", test))]
fn is_inside_app_bundle(dir: &Path) -> bool {
    dir.ends_with("Contents/MacOS")
        && dir
            .parent()
            .and_then(Path::parent)
            .and_then(Path::extension)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
}

/// A custom data directory (including a symlink into the bundle) must never
/// be removed by the updater. Resolve both paths before testing containment.
pub fn validate_update_data_location(data_dir: &Path, executable: &Path) -> io::Result<()> {
    let executable = executable.canonicalize()?;
    let data_dir = data_dir.canonicalize()?;
    // An ancestor whose name ends in `.app` is **not always** a bundle. On Windows,
    // `D:\tools.app\release\ZeppBridge.exe` would match, and a valid portable
    // folder would be refused as "data inside the bundle". A real bundle always
    // has `Contents/`.
    if let Some(bundle) = executable.ancestors().find(|ancestor| {
        ancestor
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
            && ancestor.join("Contents").is_dir()
    }) {
        if data_dir.starts_with(bundle) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,
                "User data is inside the app bundle. Quit ZeppBridge, copy the data folder to Application Support, and point ZEPPBRIDGE_DATA_DIR there before updating."));
        }
    }
    Ok(())
}

/// Whether this folder is "a shared prefix that the OS or a package manager owns".
///
/// The check is about ownership, not writability. AppImage mount points and
/// unpacked tarballs are not in this list: in those cases the folder belongs to
/// the user, and next to the install folder is the right place.
#[cfg(all(unix, not(target_os = "macos")))]
fn is_shared_prefix_dir(dir: &Path) -> bool {
    // `/app` is the install prefix in the Flatpak sandbox. `/nix/store` and `/snap`
    // are read-only content-addressed stores with hashes in the path, so data
    // written next to them is lost after the next update.
    const PREFIXES: [&str; 8] = [
        "/usr/",
        "/bin/",
        "/sbin/",
        "/opt/",
        "/app/",
        "/snap/",
        "/nix/store/",
        "/var/lib/flatpak/",
    ];
    let path = dir.to_string_lossy();
    // Add a trailing separator so `/usr` itself also matches `/usr/`.
    let path = format!("{path}/");
    PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

pub fn webview_user_data_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("webview")
}

/// Other places on this machine, apart from the current folder, that hold a `zepp.db`.
///
/// Why: the location from `resolve_data_dir()` changes with the install type.
/// NSIS installs into `%LOCALAPPDATA%` (writable, uses `{exe_dir}/data`). MSI
/// installs into `Program Files` (not writable, falls back to `%APPDATA%`). If
/// one person installs both packages, or runs once with elevation, each side
/// keeps a database. The app uses only one, so the other data looks "lost".
///
/// This function **only reads**. It does not move or delete. It follows the rule
/// of `fall_back_to_user_data_dir` (never switch folders on its own): it reports
/// the facts, and a person decides. An empty result means there is no second
/// copy, which is the usual case.
pub fn other_libraries_on_this_machine(data_dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let candidates = default_library_sources(std::env::var_os(DATA_DIR_ENV), || {
        let mut candidates = legacy_source_dirs();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                candidates.push(parent.join("data"));
            }
        }
        candidates
    });
    for candidate in candidates {
        if candidate == data_dir || !candidate.join(SQLITE_GROUP[0]).is_file() {
            continue;
        }
        if !found.contains(&candidate) {
            found.push(candidate);
        }
    }
    found
}

/// Copy-or-move leftover AppData libraries into the install-local folder.
/// Existing destination files are never overwritten.
///
/// Note: during 3.x development, 3.x ran beside 2.x (as ZeppBridge3), so this step
/// was skipped to not take data from a 2.x still in use. Now the product name and
/// identifier match 2.x and 3.0 is an in-place upgrade of 2.x, so the skip is gone:
/// old users still on the AppData layout also move over to 3.x.
/// Existing destination files are still never overwritten (if `zepp.db` exists, the whole group stays).
pub fn relocate_legacy_data(data_dir: &Path) -> Option<String> {
    let candidates = default_library_sources(std::env::var_os(DATA_DIR_ENV), legacy_source_dirs);
    if candidates.is_empty() {
        return None;
    }
    let mut failed = Vec::new();
    let live = live_user_data_dir();
    for source in relocation_sources(data_dir, candidates, live.as_deref()) {
        if let Err(error) = relocate_from(&source, data_dir) {
            failed.push(format!("{} ({error})", source.display()));
        }
    }
    if failed.is_empty() {
        None
    } else {
        Some(format!(
            "Legacy AppData data migration did not fully succeed ({}). The app can still start, and no existing file was overwritten.",
            failed.join(", ")
        ))
    }
}

/// An explicit library is an isolation boundary, including on Windows where
/// Known Folder APIs ignore APPDATA/USERPROFILE environment overrides. Do not
/// even discover default locations when a custom library was requested. Invalid
/// overrides also fail closed; resolve_data_dir reports their configuration error.
fn default_library_sources(
    raw: Option<std::ffi::OsString>,
    discover: impl FnOnce() -> Vec<PathBuf>,
) -> Vec<PathBuf> {
    if matches!(data_dir_from_env_value(raw), Ok(None)) {
        discover()
    } else {
        Vec::new()
    }
}

/// Which old folders to move into `data_dir`.
///
/// `live` is the database the installed app uses (Application Support on macOS,
/// the XDG folder on Linux). It is also in `legacy_source_dirs()`, but it is not
/// a legacy leftover. If `ZEPPBRIDGE_DATA_DIR` points elsewhere, or a dev build
/// runs from `target/`, moving it would take the whole database and credentials,
/// and the installed app would open an empty database next time. On Windows pass
/// `None`: there the `%APPDATA%` database must move next to the install folder.
fn relocation_sources(
    data_dir: &Path,
    candidates: Vec<PathBuf>,
    live: Option<&Path>,
) -> Vec<PathBuf> {
    candidates
        .into_iter()
        .filter(|source| source != data_dir && Some(source.as_path()) != live)
        .collect()
}

fn live_user_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        None
    } else {
        user_data_dir().ok()
    }
}

pub fn is_build_artifact_dir(dir: &Path) -> bool {
    let norm = normalize_path(dir);
    // `cargo-target-v3` is the isolated build folder of the v3 worktree
    // (scripts\v3-gates.ps1 points CARGO_TARGET_DIR at it). Without it, a
    // `tauri dev` binary would treat the build cache as the install folder and
    // create an empty data/ next to it.
    norm.contains("\\cargo-target\\")
        || norm.contains("/cargo-target/")
        || norm.contains("\\cargo-target-v3\\")
        || norm.contains("/cargo-target-v3/")
        || norm.contains("\\target\\debug")
        || norm.contains("/target/debug")
        || norm.contains("\\target\\release")
        || norm.contains("/target/release")
        || contains_rustc_target_triple(&norm)
}

fn contains_rustc_target_triple(norm: &str) -> bool {
    const MARKERS: [&str; 4] = [
        "\\target\\x86_64-",
        "/target/x86_64-",
        "\\target\\aarch64-",
        "/target/aarch64-",
    ];
    MARKERS.iter().any(|marker| norm.contains(marker))
}

/// When run from a cargo build folder, the data directory falls back to `data/` at the repository root.
///
/// Search upward for the repository root marker files on purpose, not "one level
/// above the manifest". After core became a workspace member, `CARGO_MANIFEST_DIR`
/// changed from `src-tauri` to `src-tauri/crates/core`. A fixed level would point
/// at `src-tauri/crates/data` and quietly create an empty database in development,
/// and the user's 200 MB of data would look lost.
fn repository_data_dir() -> Option<PathBuf> {
    let mut dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    loop {
        // The repository root has both. Either one alone can match another folder.
        if dir.join("package.json").is_file() && dir.join("src-tauri").is_dir() {
            return Some(dir.join("data"));
        }
        dir = dir.parent()?;
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn ensure_writable_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".write-probe");
    std::fs::write(&probe, b"ok")?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

fn legacy_source_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(project) = directories::ProjectDirs::from("com", "zeppbridge", "ZeppBridge") {
        dirs.push(project.data_dir().to_path_buf());
        dirs.push(project.data_dir().join("data"));
    }
    if let Ok(roaming) = std::env::var("APPDATA") {
        dirs.push(PathBuf::from(&roaming).join("com.zeppbridge.app"));
        dirs.push(
            PathBuf::from(&roaming)
                .join("zeppbridge")
                .join("ZeppBridge"),
        );
        dirs.push(
            PathBuf::from(roaming)
                .join("zeppbridge")
                .join("ZeppBridge")
                .join("data"),
        );
    }
    dirs.sort();
    dirs.dedup();
    dirs
}

fn relocate_from(source_dir: &Path, data_dir: &Path) -> io::Result<()> {
    if !path_exists(source_dir)? {
        return Ok(());
    }

    relocate_sqlite_group(source_dir, data_dir)?;
    let mut names: Vec<&str> = LEGACY_FILES.to_vec();
    names.extend(LEGACY_OPTIONAL_FILES);
    for name in names {
        relocate_entry(&source_dir.join(name), &data_dir.join(name))?;
    }
    for name in LEGACY_DIRS {
        relocate_entry(&source_dir.join(name), &data_dir.join(name))?;
    }

    // Never delete a source that still holds the live SQLite group.
    if !path_exists(&source_dir.join(SQLITE_GROUP[0]))? {
        let _ = remove_empty_dir_chain(source_dir);
    }
    Ok(())
}

/// Move the main library and its WAL/SHM as one unit.
///
/// Attaching a leftover WAL from another directory onto an existing
/// `zepp.db` produces `database disk image is malformed` and a flash-crash
/// on the next launch.
fn relocate_sqlite_group(source_dir: &Path, data_dir: &Path) -> io::Result<()> {
    let dest_db = data_dir.join(SQLITE_GROUP[0]);
    let source_db = source_dir.join(SQLITE_GROUP[0]);
    if path_exists(&dest_db)? {
        return Ok(());
    }
    if !path_exists(&source_db)? {
        return Ok(());
    }
    for name in SQLITE_GROUP {
        relocate_entry(&source_dir.join(name), &data_dir.join(name))?;
    }
    Ok(())
}

/// The first step of corrupt-database recovery: **copy** the DB / WAL / SHM group
/// into a new quarantine folder, and return only after each file's bytes match.
/// No file at the original location changes.
///
/// - Each run makes a new quarantine folder (two recoveries in one second get a
///   sequence number). It never writes into an existing folder.
/// - If any file fails to copy or to match, the whole step fails, and the caller changes nothing.
pub fn preserve_sqlite_group(db_path: &Path) -> io::Result<PathBuf> {
    let data_dir = db_path.parent().unwrap_or(db_path);
    let backups = data_dir.join("backups");
    std::fs::create_dir_all(&backups)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut dest = backups.join(format!("corrupt-{stamp}"));
    let mut attempt = 1;
    loop {
        match std::fs::create_dir(&dest) {
            Ok(()) => break,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
                dest = backups.join(format!("corrupt-{stamp}-{attempt}"));
            }
            Err(error) => return Err(error),
        }
    }
    for name in SQLITE_GROUP {
        let source = data_dir.join(name);
        if !path_exists(&source)? {
            continue;
        }
        let copy = dest.join(name);
        std::fs::copy(&source, &copy)?;
        if !same_bytes(&source, &copy)? {
            return Err(io::Error::other(format!("the quarantine copy of {name} does not match the original")));
        }
    }
    Ok(dest)
}

/// After the originals are saved, delete the file group at the original location,
/// so the next open creates an empty database. Delete the main database last: a
/// failure halfway leaves "bad main database + leftover companion files" or "only
/// the bad main database", never "new database with old WAL", which breaks on open.
pub fn remove_live_sqlite_group(db_path: &Path) -> io::Result<()> {
    let data_dir = db_path.parent().unwrap_or(db_path);
    for name in SQLITE_GROUP.iter().rev() {
        let path = data_dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn same_bytes(a: &Path, b: &Path) -> io::Result<bool> {
    use std::io::Read;
    if std::fs::metadata(a)?.len() != std::fs::metadata(b)?.len() {
        return Ok(false);
    }
    let mut left = io::BufReader::new(std::fs::File::open(a)?);
    let mut right = io::BufReader::new(std::fs::File::open(b)?);
    let (mut x, mut y) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let n = left.read(&mut x)?;
        if n == 0 {
            return Ok(right.read(&mut y)? == 0);
        }
        right.read_exact(&mut y[..n])?;
        if x[..n] != y[..n] {
            return Ok(false);
        }
    }
}

fn relocate_entry(source: &Path, destination: &Path) -> io::Result<()> {
    if !path_exists(source)? {
        return Ok(());
    }
    if path_exists(destination)? {
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_recursive(source, destination)?;
            remove_recursive(source)
        }
    }
}

fn copy_recursive(source: &Path, destination: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(source)?;
    if meta.is_dir() {
        std::fs::create_dir_all(destination)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &destination.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source, destination).map(|_| ())
    }
}

fn remove_recursive(path: &Path) -> io::Result<()> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

fn remove_empty_dir_chain(start: &Path) -> io::Result<()> {
    let mut current = start.to_path_buf();
    for _ in 0..4 {
        match std::fs::remove_dir(&current) {
            Ok(()) => {
                if let Some(parent) = current.parent() {
                    current = parent.to_path_buf();
                } else {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    Ok(())
}

fn path_exists(path: &Path) -> io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Write `bytes` via a same-directory temp file + rename.
///
/// On Windows, `std::fs::rename` already replaces the destination
/// (`MoveFileExW` + `MOVEFILE_REPLACE_EXISTING`). Do not delete the
/// destination first: that is not atomic, and a crash in between leaves
/// the target missing.
pub fn write_file_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temp = parent.join(format!(
        ".{file_name}.tmp-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests;

/// A path to show the user, which can be pasted into feedback: replace the home
/// folder with a placeholder so the user name stays on this machine.
///
/// On Windows use `%USERPROFILE%` (cmd can still expand it); on other systems use `~`.
/// If the home folder is unknown, or the path is not under it, mask the name part
/// by common shapes such as `Users\<name>` / `home/<name>`. The data folder can be
/// on another disk, but disks often still contain the user name.
pub fn mask_user_path(path: &Path) -> String {
    let text = path.display().to_string();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_string();
    if let Some(home) = directories::BaseDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
        let home_text = home.display().to_string();
        if let Some(rest) = strip_home_prefix(&text, &home_text) {
            let placeholder = if cfg!(windows) { "%USERPROFILE%" } else { "~" };
            return format!("{placeholder}{rest}");
        }
    }
    mask_user_segment(&text)
}

/// If `text` starts with the home folder (case-insensitive on Windows) and breaks at a separator, return the rest.
fn strip_home_prefix<'a>(text: &'a str, home: &str) -> Option<&'a str> {
    if home.is_empty() || !text.is_char_boundary(home.len().min(text.len())) {
        return None;
    }
    let head = text.get(..home.len())?;
    let same = if cfg!(windows) {
        head.eq_ignore_ascii_case(home)
    } else {
        head == home
    };
    let rest = &text[home.len()..];
    (same && (rest.is_empty() || rest.starts_with(['/', '\\']))).then_some(rest)
}

/// Replace the name in `...\Users\<name>\...`, `/home/<name>/...` and `/Users/<name>/...` with `***`.
fn mask_user_segment(text: &str) -> String {
    let separators: &[char] = &['/', '\\'];
    let mut parts: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find(separators) {
        parts.push(rest[..index].to_string());
        parts.push(rest[index..index + 1].to_string());
        rest = &rest[index + 1..];
    }
    parts.push(rest.to_string());
    let mut mask_next = false;
    for part in parts.iter_mut() {
        if part == "/" || part == "\\" {
            continue;
        }
        if mask_next && !part.is_empty() {
            *part = "***".to_string();
            mask_next = false;
            continue;
        }
        mask_next = part.eq_ignore_ascii_case("users") || part == "home";
    }
    parts.concat()
}
