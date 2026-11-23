use super::*;

/// One empty folder for each test case (same as the ai_clients tests; we do not add tempfile for these few tests).
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("zb-cloud-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn input(site: &str) -> CloudConfigInput {
    CloudConfigInput {
        site: site.to_string(),
        bin_dir: None,
        data_dir: None,
        sync: false,
        retention_days: 90,
        summary_days: 30,
        full_resolution_workouts: false,
        uploader_dir: None,
    }
}

/// The site rules must match the component's own: the URL shown in the UI and the URL that "Open site" opens both come from here.
/// If the result is wrong, the user goes to a site that is not theirs.
#[test]
fn site_rules_match_the_uploader() {
    for (given, expected) in [
        ("example.com", Some("https://health.example.com")),
        ("health.example.com", Some("https://health.example.com")),
        ("Example.COM.", Some("https://health.example.com")),
        (
            "https://data.example.com/",
            Some("https://data.example.com"),
        ),
        ("https://health.x.com/v1", Some("https://health.x.com")),
        (
            "https://health.x.com:8443/api/",
            Some("https://health.x.com:8443/api"),
        ),
        ("http://example.com", None),
        ("https://user:pw@example.com", None),
        ("https://example.com/?a=1", None),
        ("example.com/path", None),
        ("192.168.1.2", None),
        ("https://192.168.1.2", None),
        ("localhost", None),
        ("", None),
    ] {
        assert_eq!(normalize_site(given).as_deref(), expected, "site {given:?}");
    }
}

/// Fields that the component adds later or that the user writes by hand must survive "Save". If validation fails, the original file does not change at all.
#[test]
fn saving_keeps_unknown_fields_and_refuses_bad_input_without_writing() {
    let dir = Scratch::new("save");
    let path = dir.path().join("config.json");
    let original =
        r#"{"endpoint":"https://health.old.example/v1","snapshotBin":"C:\\snap.exe","sync":true}"#;
    std::fs::write(&path, original).unwrap();

    for bad in [
        input("http://example.com"),
        CloudConfigInput {
            retention_days: 0,
            ..input("example.com")
        },
    ] {
        assert!(save_config(dir.path(), &bad).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
    }
    let missing_cli = CloudConfigInput {
        bin_dir: Some(dir.path().to_string_lossy().into_owned()),
        ..input("example.com")
    };
    assert_eq!(
        save_config(dir.path(), &missing_cli).unwrap_err().code,
        "err.cloud.bin_dir_invalid"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);

    let good = CloudConfigInput {
        summary_days: 120,
        ..input("example.com")
    };
    save_config(dir.path(), &good).unwrap();
    let ConfigFile::Found(saved) = read_config_file(dir.path()) else {
        panic!("config should be readable after saving");
    };
    assert_eq!(saved["snapshotBin"], "C:\\snap.exe");
    assert_eq!(saved["site"], "example.com");
    assert!(!saved.contains_key("endpoint"), "the old field gives way to site");
    assert_eq!(saved["sync"], false);
    // Summary days cannot be more than retention days (the component has the same rule).
    assert_eq!(saved["summaryDays"], 90);
}

/// A broken config.json must not be overwritten as if it were "not configured".
#[test]
fn an_unreadable_config_is_reported_and_never_overwritten() {
    let dir = Scratch::new("broken");
    let path = dir.path().join("config.json");
    std::fs::write(&path, "{ not json").unwrap();
    let state = state_from_files(dir.path(), None);
    assert!(state.config_unreadable);
    assert!(!state.configured);
    assert_eq!(
        save_config(dir.path(), &input("example.com"))
            .unwrap_err()
            .code,
        "err.cloud.config_unreadable"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}

/// Status carries only names and times: key hashes do not leave the main app. A user who never installed it is "not configured", not an error.
#[test]
fn state_reads_the_component_files_without_leaking_reader_hashes() {
    let empty = Scratch::new("empty");
    let none = state_from_files(empty.path(), None);
    assert!(!none.configured && !none.config_unreadable && none.last_run.is_none());

    let dir = Scratch::new("state");
    std::fs::write(
        dir.path().join("config.json"),
        "\u{feff}{\"site\":\"example.com\",\"retentionDays\":30}",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("last-run.json"),
        r#"{"at":"2026-11-10T23:41:46.312Z","mode":"follow","result":"skipped_busy","exit_code":0,"message":"yield"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("readers.json"),
        r#"[{"label":"hark","hash":"deadbeefcafe","created_at":"2026-11-04T03:37:00Z"}]"#,
    )
    .unwrap();
    let state = state_from_files(dir.path(), None);
    assert!(state.configured);
    assert_eq!(
        state.site_url.as_deref(),
        Some("https://health.example.com")
    );
    let config = state.config.clone().unwrap();
    assert_eq!((config.retention_days, config.summary_days), (30, 30));
    assert_eq!(state.last_run.as_ref().unwrap().result, "skipped_busy");
    assert_eq!(state.readers[0].label, "hark");
    assert!(!serde_json::to_string(&state).unwrap().contains("deadbeef"));
}

/// Find the component from the scheduled task XML, but accept only a folder that really has the three scripts.
#[test]
fn the_uploader_is_found_from_the_scheduled_task_only_when_it_exists() {
    let dir = Scratch::new("locate");
    let uploader = dir.path().join("uploader");
    std::fs::create_dir_all(&uploader).unwrap();
    let xml = |folder: &Path| {
        format!(
            "<Arguments>--headless &quot;C:\\Program Files\\PowerShell\\7\\pwsh.exe&quot; -NoProfile -File &quot;{}\\run.ps1&quot; -Follow</Arguments>",
            folder.display()
        )
    };
    assert_eq!(
        script_dir_in(&xml(&uploader), "run.ps1").as_deref(),
        Some(uploader.as_path())
    );
    // The task still points to a folder that was moved: do not accept it.
    assert!(state_from_files(dir.path(), Some(&xml(&uploader)))
        .uploader_dir
        .is_none());
    for name in ["run.ps1", "setup.ps1", "upload.mjs"] {
        std::fs::write(uploader.join(name), "").unwrap();
    }
    let state = state_from_files(dir.path(), Some(&xml(&uploader)));
    assert_eq!(
        state.uploader_dir.as_deref(),
        Some(uploader.to_string_lossy().as_ref())
    );
    assert!(state.task_follow);
}

/// The name goes into the command line and credential entry names: reject names with spaces, semicolons, or a leading `-`.
#[test]
fn reader_labels_cannot_smuggle_arguments() {
    for ok in ["hark", "muse-2", "Phone_1"] {
        assert!(valid_label(ok), "{ok}");
    }
    for bad in [
        "",
        "-StateDir",
        "a b",
        "a;b",
        "\u{540d}\u{5b57}", // non-ASCII name ("name" in Chinese)
        "x\"y",
        &"a".repeat(33),
    ] {
        assert!(!valid_label(bad), "{bad}");
    }
}
