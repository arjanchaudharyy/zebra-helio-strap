#[test]
fn the_repository_fallback_lands_on_the_repository_root_not_a_crate_folder() {
    // After core became a workspace member, the manifest folder is two levels
    // deeper. This fallback must still point at data/ in the repository root,
    // or development quietly switches to an empty database.
    let dir = repository_data_dir().expect("the repository root must be found");
    assert!(dir.ends_with("data"), "{}", dir.display());
    let root = dir.parent().unwrap();
    assert!(root.join("package.json").is_file(), "{}", root.display());
    assert!(root.join("src-tauri").is_dir(), "{}", root.display());
    assert!(
        !root.ends_with("crates"),
        "the fallback must not stop at the crates folder: {}",
        root.display()
    );
}

use super::*;
use std::fs;

#[test]
fn explicit_library_never_discovers_or_moves_a_default_library() {
    let root = std::env::temp_dir().join(format!(
        "explicit-library-isolation-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    let source = root.join("default");
    let destination = root.join("isolated");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(source.join("zepp.db"), b"default library sentinel").unwrap();
    fs::write(source.join("auth.json"), b"default credential sentinel").unwrap();
    let candidates = default_library_sources(Some(destination.clone().into_os_string()), || {
        panic!("must not discover default locations for an explicit library")
    });
    for candidate in candidates {
        relocate_from(&candidate, &destination).unwrap();
    }
    assert_eq!(
        fs::read(source.join("zepp.db")).unwrap(),
        b"default library sentinel"
    );
    assert_eq!(
        fs::read(source.join("auth.json")).unwrap(),
        b"default credential sentinel"
    );
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn absent_or_empty_override_keeps_default_discovery_and_invalid_override_fails_closed() {
    let source = PathBuf::from("default-library-candidate");
    assert_eq!(
        default_library_sources(None, || vec![source.clone()]),
        vec![source.clone()]
    );
    assert_eq!(
        default_library_sources(Some("".into()), || vec![source.clone()]),
        vec![source]
    );
    assert!(
        default_library_sources(Some("relative/library".into()), || {
            panic!("invalid overrides must not fall back to a real library")
        })
        .is_empty()
    );
}

#[test]
fn app_bundle_takes_priority_even_under_a_build_cache() {
    let path = Path::new("/tmp/target/release/bundle/ZeppBridge.app/Contents/MacOS");
    assert!(is_build_artifact_dir(path));
    assert!(is_inside_app_bundle(path));
    assert!(is_inside_app_bundle(Path::new(
        "/Applications/ZeppBridge.APP/Contents/MacOS"
    )));
    assert!(!is_inside_app_bundle(Path::new(
        "/Applications/Contents/MacOS"
    )));
}

#[test]
fn replacing_the_bundle_preserves_migrated_account_devices_and_library() {
    let root = std::env::temp_dir().join(format!(
        "bundle-update-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    let bundle = root.join("ZeppBridge.app");
    let source = bundle.join("Contents/MacOS/data");
    let executable = bundle.join("Contents/MacOS/zeppbridge");
    let destination = root.join("Application Support/data");
    fs::create_dir_all(&source).unwrap();
    fs::write(&executable, b"old executable").unwrap();
    let db = rusqlite::Connection::open(source.join("zepp.db")).unwrap();
    db.execute_batch("CREATE TABLE sample(value); INSERT INTO sample VALUES (42);")
        .unwrap();
    drop(db);
    fs::write(source.join("auth.json"), b"account metadata").unwrap();
    fs::write(source.join("auth.user-id"), b"test-account").unwrap();
    fs::write(source.join("devices.json"), b"paired devices").unwrap();
    assert!(validate_update_data_location(&source, &executable).is_err());
    migrate_bundle_data(&source, &destination).unwrap();
    validate_update_data_location(&destination, &executable).unwrap();
    fs::remove_dir_all(&bundle).unwrap();
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, b"new executable").unwrap();
    migrate_bundle_data(&source, &destination).unwrap();
    validate_update_data_location(&destination, &executable).unwrap();
    for (name, expected) in [
        ("auth.json", "account metadata"),
        ("auth.user-id", "test-account"),
        ("devices.json", "paired devices"),
    ] {
        assert_eq!(
            fs::read_to_string(destination.join(name)).unwrap(),
            expected
        );
    }
    let db = rusqlite::Connection::open(destination.join("zepp.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT value FROM sample", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        42
    );
    drop(db);
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn updater_rejects_a_data_symlink_into_the_bundle() {
    let root = std::env::temp_dir().join(format!("bundle-update-symlink-{}", std::process::id()));
    let executable = root.join("ZeppBridge.app/Contents/MacOS/zeppbridge");
    let data = executable.parent().unwrap().join("data");
    fs::create_dir_all(&data).unwrap();
    fs::write(&executable, b"exe").unwrap();
    let alias = root.join("external-data");
    std::os::unix::fs::symlink(&data, &alias).unwrap();
    assert!(validate_update_data_location(&alias, &executable).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bundle_migration_copies_live_wal_and_preserves_existing_library() {
    let root = std::env::temp_dir().join(format!(
        "bundle-copy-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    let source = root.join("ZeppBridge.app/Contents/MacOS/data");
    let destination = root.join("support/data");
    fs::create_dir_all(&source).unwrap();
    let db = rusqlite::Connection::open(source.join("zepp.db")).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=WAL; CREATE TABLE sample(value); INSERT INTO sample VALUES (42);",
    )
    .unwrap();
    fs::write(source.join("auth.json"), b"test-account").unwrap();
    fs::create_dir(source.join("backups")).unwrap();
    fs::write(source.join("backups/saved"), b"backup").unwrap();
    migrate_bundle_data(&source, &destination).unwrap();
    let copied = rusqlite::Connection::open(destination.join("zepp.db")).unwrap();
    assert_eq!(
        copied
            .query_row("SELECT value FROM sample", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        42
    );
    assert!(source.join("zepp.db").exists());
    assert_eq!(
        fs::read(destination.join("backups/saved")).unwrap(),
        b"backup"
    );
    fs::write(source.join("auth.json"), b"different-account").unwrap();
    migrate_bundle_data(&source, &destination).unwrap();
    assert_eq!(
        fs::read(destination.join("auth.json")).unwrap(),
        b"test-account"
    );
    drop(copied);
    drop(db);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bundle_migration_refuses_to_mix_partial_destination() {
    let root = std::env::temp_dir().join(format!("bundle-conflict-{}", std::process::id()));
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("auth.json"), b"existing-account").unwrap();
    assert!(migrate_bundle_data(&source, &destination).is_err());
    assert_eq!(
        fs::read(destination.join("auth.json")).unwrap(),
        b"existing-account"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bundle_migration_tolerates_finder_junk_and_leftover_logs() {
    // `.DS_Store` / `logs/` / `webview/` / half-done staging are not user data.
    // They must not block the migration. On macOS that startup failure showed no dialog.
    let root = std::env::temp_dir().join(format!(
        "bundle-junk-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("auth.json"), b"account").unwrap();
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join(".DS_Store"), b"junk").unwrap();
    fs::create_dir_all(destination.join("logs")).unwrap();
    fs::write(destination.join("logs/zeppbridge.log"), b"last crash log").unwrap();
    fs::create_dir_all(destination.join("webview")).unwrap();
    fs::create_dir_all(root.join(".bundle-migration-999-0")).unwrap();
    migrate_bundle_data(&source, &destination).unwrap();
    assert_eq!(fs::read(destination.join("auth.json")).unwrap(), b"account");
    // The old folder is not deleted. It is moved aside, so the crash log in logs is still there.
    let aside: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".pre-migration-")
        })
        .collect();
    assert_eq!(aside.len(), 1, "the old target must move aside as a whole, not be deleted");
    assert_eq!(
        fs::read(aside[0].path().join("logs/zeppbridge.log")).unwrap(),
        b"last crash log"
    );
    assert!(
        !root.join(".bundle-migration-999-0").exists(),
        "the half-done folder from the last crash must be removed"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bundle_migration_falls_back_to_file_copy_when_the_source_db_wont_open() {
    // A corrupt source database must not fail the whole migration. A file copy
    // moves the database as is, and the open_resilient quarantine flow takes over.
    let root = std::env::temp_dir().join(format!(
        "bundle-corrupt-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    let source = root.join("source");
    let destination = root.join("destination");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("zepp.db"), b"this is not sqlite").unwrap();
    fs::write(source.join("auth.json"), b"account").unwrap();
    migrate_bundle_data(&source, &destination).unwrap();
    assert_eq!(
        fs::read(destination.join("zepp.db")).unwrap(),
        b"this is not sqlite"
    );
    assert_eq!(fs::read(destination.join("auth.json")).unwrap(), b"account");
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn bundle_migration_recreates_symlinks_instead_of_failing() {
    // Older versions allowed links like `data/backups -> /external-disk/backups`.
    // Such a link must not fail the startup. The link itself moves, and its target stays.
    let root = std::env::temp_dir().join(format!(
        "bundle-link-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    let source = root.join("source");
    let destination = root.join("destination");
    let external = root.join("external-backups");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("saved"), b"backup").unwrap();
    std::os::unix::fs::symlink(&external, source.join("backups")).unwrap();
    fs::write(source.join("auth.json"), b"account").unwrap();
    migrate_bundle_data(&source, &destination).unwrap();
    let link = destination.join("backups");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink(),
        "the link itself must move, not a copied folder"
    );
    assert_eq!(fs::read_link(&link).unwrap(), external);
    assert_eq!(fs::read(link.join("saved")).unwrap(), b"backup");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_app_suffixed_directory_without_contents_is_not_a_bundle() {
    // On Windows, a path like `D:\tools.app\release\` has an ancestor whose name ends
    // in .app. It is not a macOS bundle and must not cause a "data inside the bundle" refusal.
    let root = std::env::temp_dir().join(format!(
        "fake-bundle-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    let exe_dir = root.join("tools.app").join("release");
    let data = exe_dir.join("data");
    fs::create_dir_all(&data).unwrap();
    let executable = exe_dir.join("zeppbridge.exe");
    fs::write(&executable, b"exe").unwrap();
    fs::write(data.join("zepp.db"), b"db").unwrap();
    validate_update_data_location(&data, &executable)
        .expect("an .app folder without Contents/ is not a bundle and must not be refused");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_blocked_empty_data_dir_falls_back_instead_of_failing() {
    // The folder next to the install folder is not writable and holds no database
    // (a fresh .msi install into Program Files looks like this). It must fall back to
    // the user folder, not pass the error up to the `?` in `setup()` as a silent exit.
    let blocked = std::env::temp_dir().join("zeppbridge-blocked-empty");
    let _ = fs::remove_dir_all(&blocked);
    let resolved = fall_back_to_user_data_dir(
        &blocked,
        io::Error::new(io::ErrorKind::PermissionDenied, "access is denied"),
    )
    .expect("an unwritable empty folder must fall back to the user folder");
    assert_eq!(resolved, user_data_dir().unwrap());
    assert_ne!(resolved, blocked);
}

#[test]
fn a_blocked_data_dir_that_already_holds_a_library_reports_instead_of_moving() {
    // The opposite case: the folder already holds a database. A silent switch to an
    // empty folder makes the user think the data is lost, which is worse than an
    // error. The error text must contain the path and the environment variable name,
    // so the startup dialog can tell the user what to do next.
    let blocked = std::env::temp_dir().join("zeppbridge-blocked-with-db");
    fs::create_dir_all(&blocked).unwrap();
    fs::write(blocked.join(SQLITE_GROUP[0]), b"not really sqlite").unwrap();
    let error = fall_back_to_user_data_dir(
        &blocked,
        io::Error::new(io::ErrorKind::PermissionDenied, "access is denied"),
    )
    .expect_err("with a database inside, the folder must not be switched");
    let text = error.to_string();
    assert!(text.contains(&blocked.display().to_string()), "{text}");
    assert!(text.contains(DATA_DIR_ENV), "{text}");
    let _ = fs::remove_dir_all(&blocked);
}

#[test]
fn an_explicit_data_dir_must_be_absolute() {
    // A relative path expanded against the working directory makes "where the data
    // is" depend on who started the process and from where. The deployer cannot see
    // the working directory of cron or a container entrypoint.
    let error = super::data_dir_from_env_value(Some(std::ffi::OsString::from("data")))
        .expect_err("a relative path must be refused");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains(DATA_DIR_ENV), "{error}");
}

#[test]
fn an_unset_or_empty_data_dir_falls_through() {
    assert!(super::data_dir_from_env_value(None).unwrap().is_none());
    // An empty value counts as unset. In docker-compose, `ZEPPBRIDGE_DATA_DIR=` means
    // "use the default", not "use the root folder".
    assert!(
        super::data_dir_from_env_value(Some(std::ffi::OsString::new()))
            .unwrap()
            .is_none()
    );
}

#[test]
fn an_absolute_data_dir_is_taken_as_given() {
    let raw = if cfg!(windows) { r"C:\zepp" } else { "/data" };
    let dir = super::data_dir_from_env_value(Some(std::ffi::OsString::from(raw)))
        .unwrap()
        .expect("an absolute path must be accepted");
    assert_eq!(dir, PathBuf::from(raw));
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn packaged_linux_prefixes_do_not_own_user_data() {
    // deb/rpm install into /usr/bin, and the Flatpak sandbox uses /app/bin. Neither
    // may count as "data can go next to the install folder", even when root can write there.
    assert!(is_shared_prefix_dir(Path::new("/usr/bin")));
    assert!(is_shared_prefix_dir(Path::new("/usr")));
    assert!(is_shared_prefix_dir(Path::new("/app/bin")));
    assert!(is_shared_prefix_dir(Path::new("/opt/zeppbridge")));
    assert!(is_shared_prefix_dir(Path::new(
        "/snap/zeppbridge/current/bin"
    )));

    // AppImage mount points and unpacked tarballs belong to the user, so keep the install-folder layout.
    assert!(!is_shared_prefix_dir(Path::new("/tmp/.mount_ZeppBrXXXXXX")));
    assert!(!is_shared_prefix_dir(Path::new("/home/alice/zeppbridge")));
    assert!(!is_shared_prefix_dir(Path::new("/data")));
    // The prefix match compares whole folder names: `/usrlocal` is not under `/usr`.
    assert!(!is_shared_prefix_dir(Path::new("/usrlocal/bin")));
}

#[test]
fn build_cache_dirs_are_detected() {
    assert!(is_build_artifact_dir(Path::new(
        r"G:\build_cache\cargo-target\release"
    )));
    assert!(is_build_artifact_dir(Path::new(
        r"G:\build_cache\cargo-target-v3\debug"
    )));
    assert!(is_build_artifact_dir(Path::new(
        r"C:\proj\src-tauri\target\debug"
    )));
    assert!(!is_build_artifact_dir(Path::new(
        r"C:\Users\example\Apps\ZeppBridge\release"
    )));
    assert!(!is_build_artifact_dir(Path::new(r"D:\ZeppBridge")));
}

#[test]
fn relocates_sqlite_group_without_overwriting() {
    let root = std::env::temp_dir().join(format!(
        "zeppbridge-path-test-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    let source = root.join("appdata");
    let dest = root.join("install").join("data");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(source.join("zepp.db"), b"db").unwrap();
    fs::write(source.join("zepp.db-wal"), b"wal").unwrap();
    fs::write(source.join("auth.json"), b"{}").unwrap();
    fs::write(source.join("restore-pending.json"), b"{\"id\":1}").unwrap();
    fs::write(source.join("local-api.json"), b"{\"enabled\":true}").unwrap();
    fs::write(source.join("credentials.json"), b"{}").unwrap();
    fs::create_dir_all(source.join("exports")).unwrap();
    fs::write(source.join("exports").join("a.json"), b"[]").unwrap();

    relocate_from(&source, &dest).unwrap();
    assert_eq!(fs::read(dest.join("zepp.db")).unwrap(), b"db");
    assert_eq!(fs::read(dest.join("auth.json")).unwrap(), b"{}");
    assert_eq!(
        fs::read(dest.join("restore-pending.json")).unwrap(),
        b"{\"id\":1}"
    );
    assert_eq!(
        fs::read(dest.join("local-api.json")).unwrap(),
        b"{\"enabled\":true}"
    );
    assert_eq!(fs::read(dest.join("credentials.json")).unwrap(), b"{}");
    assert_eq!(
        fs::read(dest.join("exports").join("a.json")).unwrap(),
        b"[]"
    );
    assert!(!source.join("zepp.db").exists());

    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("zepp.db"), b"newer").unwrap();
    relocate_from(&source, &dest).unwrap();
    assert_eq!(fs::read(dest.join("zepp.db")).unwrap(), b"db");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn does_not_attach_foreign_wal_to_existing_destination_db() {
    let root = std::env::temp_dir().join(format!(
        "zeppbridge-path-wal-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    let source = root.join("appdata");
    let dest = root.join("install").join("data");
    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("zepp.db"), b"live").unwrap();
    fs::write(source.join("zepp.db-wal"), b"foreign-wal").unwrap();
    fs::write(source.join("zepp.db-shm"), b"foreign-shm").unwrap();

    relocate_from(&source, &dest).unwrap();
    assert_eq!(fs::read(dest.join("zepp.db")).unwrap(), b"live");
    assert!(!dest.join("zepp.db-wal").exists());
    assert!(!dest.join("zepp.db-shm").exists());

    let _ = fs::remove_dir_all(root);
}

/// When `ZEPPBRIDGE_DATA_DIR` or a dev build points elsewhere, the installed app's database stays where it is.
#[test]
fn the_live_library_is_never_relocated_into_another_data_dir() {
    let flat = PathBuf::from("/Users/a/Library/Application Support/com.zeppbridge.ZeppBridge");
    let live = flat.join("data");
    let custom = PathBuf::from("/tmp/zeppbridge-custom");
    let candidates = vec![flat.clone(), live.clone()];

    assert_eq!(
        relocation_sources(&custom, candidates.clone(), Some(&live)),
        vec![flat.clone()]
    );
    // Windows: the AppData copy still moves next to the install folder.
    assert_eq!(
        relocation_sources(&custom, candidates.clone(), None),
        vec![flat.clone(), live.clone()]
    );
    assert_eq!(
        relocation_sources(&live, candidates, Some(&live)),
        vec![flat]
    );
}

#[test]
fn atomic_write_replaces_an_existing_file() {
    let dir = std::env::temp_dir().join(format!(
        "zeppbridge-atomic-{}-{}",
        std::process::id(),
        crate::unique_suffix()
    ));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.json");
    fs::write(&path, b"old").unwrap();
    write_file_atomically(&path, b"new-content").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"new-content");
    let leftovers: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".out.json.tmp-")
        })
        .collect();
    assert!(leftovers.is_empty(), "temp file was left behind");
    let _ = fs::remove_dir_all(dir);
}

fn fresh_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("zb-fresh-{label}-{}", crate::unique_suffix()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn fresh_start_moves_the_whole_library_aside_and_keeps_every_file() {
    use std::time::Duration;
    let root = fresh_root("move");
    let data = root.join("data");
    std::fs::create_dir_all(data.join("webview")).unwrap();
    std::fs::write(data.join("zepp.db"), b"library").unwrap();
    std::fs::write(data.join("official.json"), b"{}").unwrap();

    assert_eq!(
        fresh_start::apply_with_wait(&data, Duration::ZERO).unwrap(),
        None,
        "no marker, no move"
    );
    request_fresh_start(&data).unwrap();
    let aside = fresh_start::apply_with_wait(&data, Duration::ZERO)
        .unwrap()
        .expect("moved");

    assert!(data.is_dir());
    assert_eq!(
        std::fs::read_dir(&data).unwrap().count(),
        0,
        "fresh library is empty"
    );
    assert_eq!(std::fs::read(aside.join("zepp.db")).unwrap(), b"library");
    assert!(aside.join("official.json").is_file() && aside.join("webview").is_dir());
    assert!(
        !aside.join(fresh_start::MARKER).exists(),
        "renaming it back must not move it again"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn fresh_start_never_reuses_an_existing_folder() {
    let root = fresh_root("reuse");
    std::fs::create_dir_all(root.join("data-before-x")).unwrap();
    assert_eq!(
        fresh_start::aside_path(&root.join("data"), "x").unwrap(),
        root.join("data-before-x-2")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn masked_paths_never_carry_the_user_name() {
    if let Some(dirs) = directories::BaseDirs::new() {
        let home = dirs.home_dir().to_path_buf();
        let shown = mask_user_path(&home.join("ZeppBridge").join("data"));
        let placeholder = if cfg!(windows) { "%USERPROFILE%" } else { "~" };
        assert!(shown.starts_with(placeholder), "{shown}");
        assert!(shown.ends_with("data"), "{shown}");
    }
    // When the path is not under the home folder (other disk, other account), mask the name part by common shapes.
    assert_eq!(
        mask_user_segment(r"D:\Users\alice\ZeppBridge\data"),
        r"D:\Users\***\ZeppBridge\data"
    );
    assert_eq!(
        mask_user_segment("/home/bob/.local/share/zeppbridge/data"),
        "/home/***/.local/share/zeppbridge/data"
    );
    assert_eq!(
        mask_user_segment(r"E:\ZeppBridge\data"),
        r"E:\ZeppBridge\data"
    );
    assert_eq!(strip_home_prefix(r"C:\Users\al2\x", r"C:\Users\al"), None);
}
