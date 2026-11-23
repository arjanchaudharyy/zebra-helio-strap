//! In-place replacement for the portable build (silent update).
//!
//! The portable build is one folder: `ZeppBridge.exe`, `zeppbridge-mcp.exe`, `zeppbridge-cli.exe`, and next to them
//! `data\`. Its update package is a signed zip that holds only these programs (built by CI `scripts/release/package-portable.ps1`,
//! the `windows-x86_64-portable-cli` entry in `latest.json`; the older `windows-x86_64-portable` has no CLI and stays for older apps). The updater plugin checks the signature against the built-in public key during download,
//! so the bytes that reach this module are already verified.
//!
//! Windows does not allow overwriting a running exe, but it allows a rename. So no helper process has to wait for the main process to exit:
//!
//! 1. Extract the package to a staging directory next to the exe (same volume, so later renames do not become cross-disk copies).
//! 2. Rename each existing file to `*.old`, then rename the new file into place. If any step fails, undo all renames in reverse order,
//!    and the folder returns to its pre-update state.
//! 3. The caller launches the new exe (with [`AFTER_UPDATE_ARG`]) and exits. Before it sets up the single instance, the new process waits until
//!    it can delete `ZeppBridge.exe.old`. A successful delete means the old process has exited ([`wait_for_previous`]).
//!
//! An AI client can still hold `zeppbridge-mcp.exe.old` open (and a script can still run `zeppbridge-cli.exe.old`).
//! If the delete fails, keep the file and delete it at the next startup ([`sweep`]).

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Name of the main program in the package. The swap replaces **the exe that runs now**, whatever name the user gave it.
const APP_ENTRY: &str = "ZeppBridge.exe";
/// Name of the MCP in the package (it matches the externalBin output of tauri.sidecar.conf.json).
const MCP_ENTRY: &str = "zeppbridge-mcp.exe";
/// Name of the CLI in the package (it ships and updates with the app). Older packages do not have it.
const CLI_ENTRY: &str = "zeppbridge-cli.exe";
const STAGING_DIR: &str = ".update-staging";
const OLD_SUFFIX: &str = "old";
/// Size limit for one file: a real exe is tens of MB. A larger entry is not a package we built.
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

/// Launch argument for the new process: wait for the old process to exit before setup.
pub(crate) const AFTER_UPDATE_ARG: &str = "--after-update";

/// Extracted files that wait to go into place: (staging path, target path).
#[derive(Debug)]
pub(crate) struct Staged {
    staging: PathBuf,
    files: Vec<(PathBuf, PathBuf)>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn exe_dir(exe: &Path) -> io::Result<&Path> {
    exe.parent().ok_or_else(|| invalid("program path has no parent directory"))
}

/// Extracts the update package to the staging directory. The package can only hold `ZeppBridge.exe` (required), `zeppbridge-mcp.exe` and `zeppbridge-cli.exe`,
/// flat, with no directories. Any other entry is rejected, so nothing unexpected is written to the user's folder.
pub(crate) fn stage(exe: &Path, payload: &[u8]) -> io::Result<Staged> {
    let dir = exe_dir(exe)?;
    let mut archive = zip::ZipArchive::new(io::Cursor::new(payload))
        .map_err(|error| invalid(format!("update package is not a valid zip: {error}")))?;
    let mut plan = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| invalid(format!("cannot read entry {index} of the update package: {error}")))?;
        let name = entry.name().to_string();
        let dest = if name.eq_ignore_ascii_case(APP_ENTRY) {
            exe.to_path_buf()
        } else if name.eq_ignore_ascii_case(MCP_ENTRY) {
            dir.join(MCP_ENTRY)
        } else if name.eq_ignore_ascii_case(CLI_ENTRY) {
            dir.join(CLI_ENTRY)
        } else {
            return Err(invalid(format!("unexpected entry in the update package: {name}")));
        };
        if entry.size() == 0 || entry.size() > MAX_ENTRY_BYTES {
            return Err(invalid(format!("wrong size for {name} in the update package")));
        }
        if plan
            .iter()
            .any(|(_, existing): &(usize, PathBuf)| existing == &dest)
        {
            return Err(invalid(format!("{name} appears twice in the update package")));
        }
        plan.push((index, dest));
    }
    if !plan.iter().any(|(_, dest)| dest == exe) {
        return Err(invalid("update package has no main program"));
    }

    let staging = dir.join(STAGING_DIR);
    remove_dir_if_present(&staging)?;
    std::fs::create_dir_all(&staging)?;
    let mut files = Vec::new();
    for (index, dest) in plan {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| invalid(format!("cannot read entry {index} of the update package: {error}")))?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .by_ref()
            .take(MAX_ENTRY_BYTES)
            .read_to_end(&mut bytes)?;
        let name = dest.file_name().ok_or_else(|| invalid("target has no file name"))?;
        let staged = staging.join(name);
        std::fs::write(&staged, bytes)?;
        files.push((staged, dest));
    }
    Ok(Staged { staging, files })
}

/// First free `*.old` name. A `.old` from the last update can still be in use (the MCP still runs).
/// If it cannot be deleted, use the next number, so it does not block this update.
fn free_old_name(dest: &Path) -> io::Result<PathBuf> {
    for attempt in 0..16 {
        let suffix = if attempt == 0 {
            OLD_SUFFIX.to_string()
        } else {
            format!("{OLD_SUFFIX}{attempt}")
        };
        let mut name = dest.as_os_str().to_owned();
        name.push(format!(".{suffix}"));
        let candidate = PathBuf::from(name);
        match std::fs::remove_file(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Err(_) => continue,
        }
    }
    Err(io::Error::other(format!(
        "cannot delete any old version file next to {}",
        dest.display()
    )))
}

enum Done {
    /// The original file was renamed to `.old`.
    Retired { dest: PathBuf, old: PathBuf },
    /// The new file was put in place.
    Placed { staged: PathBuf, dest: PathBuf },
}

fn undo(done: Vec<Done>) {
    for step in done.into_iter().rev() {
        let _ = match step {
            Done::Placed { staged, dest } => std::fs::rename(dest, staged),
            Done::Retired { dest, old } => std::fs::rename(old, dest),
        };
    }
}

/// Puts the new files in place. On failure, the folder returns to its pre-update state and an error is returned.
pub(crate) fn swap(staged: Staged) -> io::Result<()> {
    let mut done = Vec::new();
    for (new, dest) in &staged.files {
        let step = (|| -> io::Result<()> {
            if dest.exists() {
                let old = free_old_name(dest)?;
                std::fs::rename(dest, &old)?;
                done.push(Done::Retired {
                    dest: dest.clone(),
                    old,
                });
            }
            std::fs::rename(new, dest)?;
            done.push(Done::Placed {
                staged: new.clone(),
                dest: dest.clone(),
            });
            Ok(())
        })();
        if let Err(error) = step {
            undo(done);
            let _ = remove_dir_if_present(&staged.staging);
            return Err(error);
        }
    }
    let _ = remove_dir_if_present(&staged.staging);
    Ok(())
}

fn remove_dir_if_present(dir: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// At startup, removes leftovers from the last update (staging directory, `*.old*`). Files still in use stay until the next time.
pub(crate) fn sweep(exe: &Path) {
    let Ok(dir) = exe_dir(exe) else {
        return;
    };
    let _ = remove_dir_if_present(&dir.join(STAGING_DIR));
    let ours = [
        exe.file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        Some(MCP_ENTRY.to_string()),
        Some(CLI_ENTRY.to_string()),
    ];
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let leftover = ours.iter().flatten().any(|base| {
            name.len() > base.len() + 1
                && name[..base.len()].eq_ignore_ascii_case(base)
                && name[base.len()..].starts_with(&format!(".{OLD_SUFFIX}"))
        });
        if leftover {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// When the new process starts: wait for the old process to exit (its exe was renamed to `.old` and cannot be deleted while the process runs).
/// After the full `timeout`, continue anyway. At worst, the single instance merges this launch into the old process that has not fully exited.
pub(crate) fn wait_for_previous(exe: &Path, timeout: Duration) {
    let mut old = exe.as_os_str().to_owned();
    old.push(format!(".{OLD_SUFFIX}"));
    let old = PathBuf::from(old);
    let deadline = Instant::now() + timeout;
    loop {
        match std::fs::remove_file(&old) {
            Ok(()) => return,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(150)),
            Err(_) => return,
        }
    }
}

#[cfg(test)]
#[path = "portable_tests.rs"]
mod tests;
