use super::*;
use std::io::Write;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zeppbridge-portable-{name}-{}-{}",
        std::process::id(),
        zeppbridge_core::unique_suffix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Builds an update package. When `deflate` is true, compress it like CI does (Compress-Archive outputs deflate).
fn payload(entries: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    let method = if deflate {
        zip::CompressionMethod::Deflated
    } else {
        zip::CompressionMethod::Stored
    };
    let options = zip::write::SimpleFileOptions::default().compression_method(method);
    for (name, bytes) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn both_programs_are_replaced_and_the_leftovers_are_swept_next_start() {
    let dir = temp_dir("swap");
    let exe = dir.join("ZeppBridge.exe");
    std::fs::write(&exe, b"old app").unwrap();
    std::fs::write(dir.join(MCP_ENTRY), b"old mcp").unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("data").join("zepp.db"), b"library").unwrap();

    // The old folder has no CLI yet: the update package adds it for the first time.
    let bytes = payload(
        &[
            (APP_ENTRY, b"new app"),
            (MCP_ENTRY, b"new mcp"),
            (CLI_ENTRY, b"new cli"),
        ],
        true,
    );
    swap(stage(&exe, &bytes).unwrap()).unwrap();

    assert_eq!(read(&exe), b"new app");
    assert_eq!(read(&dir.join(MCP_ENTRY)), b"new mcp");
    assert_eq!(read(&dir.join(CLI_ENTRY)), b"new cli");
    assert_eq!(read(&dir.join("ZeppBridge.exe.old")), b"old app");
    assert_eq!(read(&dir.join("data").join("zepp.db")), b"library");
    assert!(!dir.join(STAGING_DIR).exists());

    sweep(&exe);
    assert_eq!(
        names(&dir),
        ["ZeppBridge.exe", "data", CLI_ENTRY, MCP_ENTRY]
    );
    wait_for_previous(&exe, Duration::from_millis(10));
    let _ = std::fs::remove_dir_all(dir);
}

/// Failure halfway (here the MCP step cannot find its staged file): the main program that was already swapped goes back to the old one,
/// `.old` gets its original name again, and the folder is the same as before the update.
#[test]
fn a_failure_halfway_puts_every_file_back() {
    let dir = temp_dir("rollback");
    let exe = dir.join("ZeppBridge.exe");
    std::fs::write(&exe, b"old app").unwrap();
    std::fs::write(dir.join(MCP_ENTRY), b"old mcp").unwrap();

    let bytes = payload(&[(APP_ENTRY, b"new app"), (MCP_ENTRY, b"new mcp")], false);
    let staged = stage(&exe, &bytes).unwrap();
    std::fs::remove_file(staged.staging.join(MCP_ENTRY)).unwrap();
    assert!(swap(staged).is_err());

    assert_eq!(read(&exe), b"old app");
    assert_eq!(read(&dir.join(MCP_ENTRY)), b"old mcp");
    assert_eq!(names(&dir), ["ZeppBridge.exe", MCP_ENTRY]);
    let _ = std::fs::remove_dir_all(dir);
}

/// The package can only hold the two programs: any other entry (mainly one with a path) is rejected, and nothing is written.
#[test]
fn unexpected_entries_and_a_missing_main_program_are_refused_before_writing() {
    let dir = temp_dir("refuse");
    let exe = dir.join("ZeppBridge.exe");
    std::fs::write(&exe, b"old app").unwrap();

    for bytes in [
        payload(&[(APP_ENTRY, b"new"), ("data/zepp.db", b"evil")], false),
        payload(
            &[(APP_ENTRY, b"new"), ("../ZeppBridge.exe", b"evil")],
            false,
        ),
        payload(&[(MCP_ENTRY, b"new mcp")], false),
        payload(&[(APP_ENTRY, b"")], false),
        b"not a zip".to_vec(),
    ] {
        let error = stage(&exe, &bytes).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData, "{error}");
    }
    assert_eq!(names(&dir), ["ZeppBridge.exe"]);
    assert_eq!(read(&exe), b"old app");
    let _ = std::fs::remove_dir_all(dir);
}

/// The user renamed the exe (for example, a second download became "ZeppBridge (1).exe"): the swap replaces the file that runs now.
/// An old portable build without the MCP gets the MCP next to it after the update.
#[test]
fn the_running_exe_is_replaced_whatever_it_is_called() {
    let dir = temp_dir("renamed");
    let exe = dir.join("ZeppBridge (1).exe");
    std::fs::write(&exe, b"old app").unwrap();

    let bytes = payload(&[(APP_ENTRY, b"new app"), (MCP_ENTRY, b"new mcp")], true);
    swap(stage(&exe, &bytes).unwrap()).unwrap();
    sweep(&exe);

    assert_eq!(read(&exe), b"new app");
    assert_eq!(read(&dir.join(MCP_ENTRY)), b"new mcp");
    assert_eq!(names(&dir), ["ZeppBridge (1).exe", MCP_ENTRY]);
    let _ = std::fs::remove_dir_all(dir);
}
