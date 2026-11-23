use super::*;

#[test]
fn cli_error_fallback_redacts_request_urls() {
    for url in [
        "https://api-mifit.huami.com/users/private-user?token=private-token",
        "http://localhost/users/private-user?token=private-token",
    ] {
        for error in [
            ZeppBridgeError::ConfigError(format!("request failed ({url})")),
            ZeppBridgeError::AuthError(format!("request failed ({url})")),
            ZeppBridgeError::DataUnavailable(format!("request failed ({url})")),
            ZeppBridgeError::Unknown(format!("request failed ({url})")),
            ZeppBridgeError::HttpStatus {
                status: 503,
                message: url.into(),
            },
        ] {
            let text = user_text(&error);
            assert!(!text.is_empty());
            for secret in [url, "private-user", "private-token"] {
                assert!(!text.contains(secret), "{text}");
            }
        }
    }
}

#[test]
fn sync_report_output_reports_failed_streams_without_discarding_results() {
    use zeppbridge_core::models::CapabilityStatus;
    use zeppbridge_core::sync::{StreamReport, StreamStatus};
    for status in [
        StreamStatus::Failed,
        StreamStatus::Unavailable,
        StreamStatus::Unverified,
        StreamStatus::Success,
    ] {
        let success = status != StreamStatus::Failed;
        let report = SyncReport {
            cleanup_failed: false,
            success,
            core_ok: true,
            records_written: 12,
            message: Some("report detail".into()),
            streams: vec![
                StreamReport {
                    stream: "sleep".into(),
                    status,
                    records_written: 0,
                    raw_records: 1,
                    capability: CapabilityStatus::Verified,
                    needs_reauth: false,
                    message: Some("stream detail".into()),
                },
                StreamReport {
                    stream: "heart_rate".into(),
                    status: StreamStatus::Success,
                    records_written: 12,
                    raw_records: 1,
                    capability: CapabilityStatus::Verified,
                    needs_reauth: false,
                    message: None,
                },
            ],
        };
        let (code, payload, human) = sync_report_output(&report, "incremental", None);
        assert_eq!(code, if success { 0 } else { 8 });
        assert_eq!(payload["ok"], success);
        assert_eq!(payload["success"], success);
        assert_eq!(payload["recordsWritten"], 12);
        assert_eq!(payload["streams"][0]["message"], "stream detail");
        assert_eq!(payload["streams"][1]["recordsWritten"], 12);
        assert_eq!(human.contains("partly failed"), !success);
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// Every error must render as one line, and **the rendering itself must finish**.
///
/// Before, the fallback arm of `user_text` was `user_text(other)`: it passed the same value back to
/// itself, an unconditional self-recursion. Release builds turn the tail call into a loop, so the CLI did not crash. It
/// spun at 100% CPU and never returned: `export --types food`, an invalid date, a `--workout` that does not
/// exist, every error thrown after the database opened, all hung here. The exit codes
/// are a documented contract, so scheduler scripts would hang forever.
///
/// Only the `Headless` arm returned, so `status` reporting "the database is still v20" looked fine.
/// That is also why it got past manual acceptance. This case uses the **non**-Headless arms.
#[test]
fn every_error_renders_and_terminates() {
    // If it hangs, this case hangs instead of failing. That is also a signal: a CI timeout means a regression.
    let cases = vec![
        ZeppBridgeError::ConfigError("Select at least one data type to export".into()),
        ZeppBridgeError::AuthError("Could not read the token".into()),
        ZeppBridgeError::DataUnavailable("This workout is not in the local database".into()),
        ZeppBridgeError::Cancelled,
        ZeppBridgeError::HttpStatus {
            status: 503,
            message: "Under maintenance".into(),
        },
    ];
    for error in &cases {
        let text = user_text(error);
        assert!(
            !text.trim().is_empty(),
            "{error:?} rendered as an empty string; the CLI would print a blank line"
        );
    }
}

#[test]
fn a_misspelled_flag_is_a_usage_error_not_a_silent_default() {
    let flags = Flags::parse(&args(&["--form", "csv"])).unwrap();
    assert!(flags.reject_unknown(&["format"]).is_err());
}

#[test]
fn export_defaults_include_daily_activity_and_keep_json_summary() {
    let options =
        parse_export_args(&args(&["--from", "2026-01-01", "--to", "2026-01-31"])).unwrap();
    assert_eq!(options.format, "json");
    assert_eq!(options.out, None);
    assert_eq!(
        options.selection.data_types,
        ["workouts", "daily_activity", "sleep"]
    );
    assert_eq!(options.selection.detail, ExportDetail::Summary);
    assert_eq!(
        options.selection.scope,
        Some(ExportScope::date_range("2026-01-01", "2026-01-31"))
    );
    for data_type in &options.selection.data_types {
        assert!(EXPORT_DATA_TYPES.contains(&data_type.as_str()));
    }
}

#[test]
fn archival_formats_always_request_full_detail() {
    for format in ["csv", "gpx", "fit"] {
        for detail in [None, Some("summary"), Some("full")] {
            let mut input = args(&[
                "--workout",
                "run-1",
                "--format",
                format,
                "--out",
                "export-output",
            ]);
            if let Some(detail) = detail {
                input.extend(args(&["--detail", detail]));
            }
            let options = parse_export_args(&input).unwrap();
            assert_eq!(options.selection.detail, ExportDetail::Full, "{format}");
            assert_eq!(
                options.selection.scope,
                Some(ExportScope::Workout {
                    workout_id: "run-1".into(),
                })
            );
        }
    }
    let options = parse_export_args(&args(&[
        "--workout",
        "run-1",
        "--format",
        "json",
        "--detail",
        "full",
    ]))
    .unwrap();
    assert_eq!(options.selection.detail, ExportDetail::Full);
}

#[test]
fn export_type_validation_uses_the_core_names_and_normalization() {
    let options = parse_export_args(&args(&[
        "--workout=run-1",
        "--types= WORKOUTS , HeArt_RaTe ",
    ]))
    .unwrap();
    assert_eq!(options.selection.data_types, ["workouts", "heart_rate"]);
    for types in ["workouts,typo", "daily", "", "  ", ", ,"] {
        let input = args(&["--workout", "run-1", "--types", types, "--json"]);
        let message = parse_export_args(&input).unwrap_err();
        assert!(message.contains("--types"), "{message}");
        assert_eq!(cmd_export(&input), EXIT_USAGE);
    }
}

#[test]
fn export_refuses_to_pick_a_winner_between_date_range_and_single_workout() {
    // Both ranges at once is a contradictory request. A priority rule would only lead people to write
    // scripts that think "if I pass --workout, it exports only that one".
    let code = cmd_export(&args(&[
        "--from",
        "2026-01-01",
        "--to",
        "2026-01-31",
        "--workout",
        "run-1",
        "--json",
    ]));
    assert_eq!(code, EXIT_USAGE);
}

#[test]
fn sync_days_only_makes_sense_for_history_mode() {
    assert_eq!(
        cmd_sync(&args(&["--mode", "incremental", "--days", "30", "--json"])),
        EXIT_USAGE
    );
    assert_eq!(cmd_sync(&args(&["--mode", "nope", "--json"])), EXIT_USAGE);
    assert_eq!(
        cmd_sync(&args(&["--mode", "history", "--days", "99999", "--json"])),
        EXIT_USAGE
    );
}

#[test]
fn reprocess_rejects_flags_it_does_not_understand() {
    // `--force` looks a lot like `--all`. If we silently ignore it, the user thinks a full-database
    // replay just ran, when nothing ran at all.
    let flags = Flags::parse(&args(&["--force"])).unwrap();
    assert!(flags.reject_unknown(&["json", "all"]).is_err());
    assert_eq!(cmd_reprocess(&args(&["--force", "--json"])), EXIT_USAGE);
}

#[test]
fn sync_accepts_the_escape_hatch_that_skips_the_replay() {
    // People with a large database and a small cron window need a way to move the replay to another time. A misspelled switch
    // must be an error, or "I turned it off" and "it ran four minutes again" are both true.
    let flags = Flags::parse(&args(&["--no-reprocess"])).unwrap();
    assert!(flags
        .reject_unknown(&["json", "mode", "days", "no-reprocess"])
        .is_ok());
    assert!(flags.has("no-reprocess"));
    let typo = Flags::parse(&args(&["--no-reprocesss"])).unwrap();
    assert!(typo
        .reject_unknown(&["json", "mode", "days", "no-reprocess"])
        .is_err());
}

#[test]
fn the_help_text_names_every_command_that_can_be_run() {
    // A command missing from the help is the same as a command that does not exist: a non-interactive program has no
    // other place where people can discover it.
    for command in [
        "status",
        "sync",
        "reprocess",
        "export",
        "contract",
        "shoes",
        "races",
    ] {
        assert!(HELP.contains(command), "help does not mention {command}");
    }
}

#[test]
fn unknown_command_and_empty_invocation_both_report_usage() {
    assert_eq!(run(&args(&["frobnicate"])), EXIT_USAGE);
    assert_eq!(run(&[]), EXIT_USAGE);
    assert_eq!(run(&args(&["version"])), EXIT_OK);
    assert_eq!(run(&args(&["contract"])), EXIT_OK);
}

#[test]
fn every_documented_exit_code_is_distinct() {
    // Exit codes are a contract with scheduler scripts: if two meanings share one number,
    // nobody can write retry logic.
    let codes = [
        EXIT_OK,
        EXIT_FAILED,
        EXIT_USAGE,
        EXIT_NOT_CONFIGURED,
        EXIT_BUSY,
        EXIT_CLOUD,
        EXIT_DATABASE,
        EXIT_SCHEMA,
        EXIT_INCOMPLETE_SYNC,
    ];
    let mut unique = codes.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), codes.len(), "exit codes must be unique");
}

#[test]
fn help_text_documents_every_exit_code_the_binary_can_return() {
    for code in [
        EXIT_OK,
        EXIT_FAILED,
        EXIT_USAGE,
        EXIT_NOT_CONFIGURED,
        EXIT_BUSY,
        EXIT_CLOUD,
        EXIT_DATABASE,
        EXIT_SCHEMA,
        EXIT_INCOMPLETE_SYNC,
    ] {
        assert!(
            HELP.contains(&code.to_string()),
            "exit code {code} is not documented in --help"
        );
    }
    // The privacy boundary must appear in the help. Users must not need to read the source to learn it.
    assert!(HELP.contains("Listens on no port"));
}
/// Issue #40 lost two full hours on this one line.
///
/// That user ran the CLI under `~/ZeppBridge/src-tauri/target/release/` and put the data
/// folder next to the exe, but the program looked in `data/` at the repo root. Neither side said anything. He
/// saw only "no local database". This test pins "an error must say where it looked".
#[test]
fn a_not_found_message_always_names_the_directory_it_looked_in() {
    let dir = std::path::Path::new("/home/x/ZeppBridge/data");
    let text = where_it_looked_from(dir, None);
    assert!(text.contains("/home/x/ZeppBridge/data"), "{text}");
    // The one thing the user can use to fix the location must also appear.
    assert!(text.contains("ZEPPBRIDGE_DATA_DIR"), "{text}");
}

/// When run from a build directory, it must also explain why the data is not next to the exe.
#[test]
fn running_from_a_build_directory_explains_why_data_is_elsewhere() {
    let dir = std::path::Path::new("/home/x/ZeppBridge/data");
    let exe_dir = std::path::Path::new("/home/x/ZeppBridge/src-tauri/target/release");
    let text = where_it_looked_from(dir, Some(exe_dir));
    assert!(text.contains("build"), "must say that this is a build directory: {text}");
    assert!(text.contains("data/"), "must point to the repo data/ directory: {text}");

    // An installed version must not see this explanation: it would only confuse people.
    let installed = std::path::Path::new("/usr/bin");
    assert!(!where_it_looked_from(dir, Some(installed)).contains("build directory"));
}

/// A missing `auth.json` and a missing token are two different things and must not get the same line.
///
/// Before, whichever one was missing, it printed "set ZEPPBRIDGE_CREDENTIAL_STORE=env and supply
/// ZEPPBRIDGE_APP_TOKEN". But the user in issue #40 had set both. The missing piece was
/// `auth.json`. The program told him to do again what he had just done, for four rounds.
#[test]
fn a_missing_auth_file_is_reported_as_such_not_as_a_missing_token() {
    let dir = std::env::temp_dir().join("zeppbridge-cli-not-connected-test");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let missing = not_connected_message(&dir);
    assert!(
        missing.contains("auth.json"),
        "must name the missing file: {missing}"
    );
    assert!(
        missing.contains("does not exist"),
        "must say that it does not exist, not tell people to set the environment variable again: {missing}"
    );
    // The token half must still be explained: the database copies across machines, the token does not. This is the easiest trap.
    assert!(missing.contains("ZEPPBRIDGE_APP_TOKEN"), "{missing}");

    // With auth.json present, the token is what is missing. Only then print the original line.
    std::fs::write(dir.join("auth.json"), "{}").unwrap();
    let token_missing = not_connected_message(&dir);
    assert!(
        !token_missing.contains("does not exist"),
        "must not say the file does not exist when it exists: {token_missing}"
    );
    assert!(
        token_missing.contains("ZEPPBRIDGE_CREDENTIAL_STORE"),
        "{token_missing}"
    );
    // Both lines must include the directory.
    assert!(
        token_missing.contains(&dir.display().to_string()),
        "{token_missing}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
/// The line that the command line prints for people must be English.
///
/// The Linux user in issue #40 got two Chinese messages on an English command line,
/// about the system keyring and a v19 local database. The CLI's **own**
/// text was then changed to English, but `user_message()` from core was still Chinese.
#[test]
fn headless_failures_are_reported_in_english() {
    use zeppbridge_core::models::error::HeadlessProblem;

    let cases = [
        HeadlessProblem::NoCredentialStore {
            detail: "could not read the keyring".into(),
        },
        HeadlessProblem::SchemaUpgradeRequired {
            found: 19,
            required: 21,
        },
        HeadlessProblem::TokenNotInStore,
        HeadlessProblem::ProgramOutdated {
            found: 47,
            supported: 41,
            program: "%USERPROFILE%\\AppData\\Local\\ZeppBridge\\zeppbridge-mcp.exe".into(),
            version: "3.0.4".into(),
        },
    ];
    for problem in cases {
        let error = ZeppBridgeError::Headless(problem);
        let text = user_text(&error);
        assert!(
            !text
                .chars()
                .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character)),
            "the command line must not print Chinese: {text}"
        );
        // Core's fallback text is English now too. The desktop UI gets its localized text by code.
        assert!(!error
            .user_message()
            .chars()
            .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character)));
    }
}

/// "The database must be upgraded first" and "cannot get the token" need different responses from scheduler scripts, so the exit codes must differ.
///
/// Any number of retries does not help the first: it needs a reprocess first. The second needs a person to supply credentials.
#[test]
fn a_schema_upgrade_and_a_missing_token_do_not_share_an_exit_code() {
    use zeppbridge_core::models::error::HeadlessProblem;

    let (schema, _) = exit_code_for(&ZeppBridgeError::Headless(
        HeadlessProblem::SchemaUpgradeRequired {
            found: 19,
            required: 21,
        },
    ));
    let (token, _) = exit_code_for(&ZeppBridgeError::Headless(HeadlessProblem::TokenNotInStore));
    assert_eq!(schema, EXIT_SCHEMA);
    assert_eq!(token, EXIT_NOT_CONFIGURED);
    assert_ne!(schema, token);

    let (no_store, kind) = exit_code_for(&ZeppBridgeError::Headless(
        HeadlessProblem::NoCredentialStore {
            detail: "no dbus".into(),
        },
    ));
    assert_eq!(no_store, EXIT_NOT_CONFIGURED);
    assert_eq!(kind, "auth");
    assert_ne!(no_store, EXIT_FAILED);
}

/// Errors that the command line prints for people must not contain Chinese: English users cannot read it, and the desktop's
/// lookup of localized text by code does not exist on the command line. Variants with Chinese original text become the English line + error code. The inputs below are Chinese core-style text written as Unicode escapes.
#[test]
fn cli_errors_never_print_chinese() {
    let cases = vec![
        ZeppBridgeError::ConfigError("\u{8bf7}\u{81f3}\u{5c11}\u{9009}\u{62e9}\u{4e00}\u{79cd}\u{5bfc}\u{51fa}\u{6570}\u{636e}".into()),
        ZeppBridgeError::AuthError("\u{4ee4}\u{724c}\u{8bfb}\u{4e0d}\u{51fa}\u{6765}".into()),
        ZeppBridgeError::CredentialStore("\u{51ed}\u{636e}\u{7ba1}\u{7406}\u{5668}\u{62d2}\u{7edd}\u{5199}\u{5165}".into()),
        ZeppBridgeError::InvalidHost("\u{4e0d}\u{5728}\u{767d}\u{540d}\u{5355}\u{91cc}".into()),
        ZeppBridgeError::DataUnavailable("\u{672c}\u{5730}\u{5e93}\u{91cc}\u{6ca1}\u{6709}\u{8fd9}\u{6761}\u{8fd0}\u{52a8}\u{8bb0}\u{5f55}".into()),
        ZeppBridgeError::Unavailable("\u{8fd9}\u{4e2a}\u{533a}\u{57df}\u{6ca1}\u{6709}\u{8be5}\u{63a5}\u{53e3}".into()),
        ZeppBridgeError::Busy("\u{684c}\u{9762}\u{5e94}\u{7528}\u{6b63}\u{5728}\u{540c}\u{6b65}".into()),
        ZeppBridgeError::TimedOut("\u{672c}\u{8f6e}\u{540c}\u{6b65}\u{8d85}\u{65f6}".into()),
        ZeppBridgeError::NeedsReauth("\u{4ee4}\u{724c}\u{8fc7}\u{671f}".into()),
        ZeppBridgeError::ParseError("JSON \u{5904}\u{7406}\u{5931}\u{8d25}".into()),
        ZeppBridgeError::Unknown("\u{672a}\u{77e5}".into()),
        ZeppBridgeError::Cancelled,
        ZeppBridgeError::AccountMismatch,
        ZeppBridgeError::TrainingPlan {
            code: "err.training_plan.draft_not_found",
            message: "\u{8349}\u{7a3f}\u{4e0d}\u{5b58}\u{5728}".into(),
        },
        ZeppBridgeError::CloudRejected {
            code: 401,
            message: "\u{767b}\u{5f55}\u{5df2}\u{8fc7}\u{671f}".into(),
        },
        ZeppBridgeError::RetryExhausted {
            status: 503,
            message: "\u{7ef4}\u{62a4}\u{4e2d}".into(),
        },
    ];
    for error in &cases {
        let text = user_text(error);
        assert!(!text.chars().any(is_cjk), "{error:?} -> {text}");
    }
    // When the original text is Chinese, the error code stays in the output in its place, so feedback is searchable.
    let text = user_text(&ZeppBridgeError::ConfigError(
        "\u{8bf7}\u{81f3}\u{5c11}\u{9009}\u{62e9}\u{4e00}\u{79cd}\u{5bfc}\u{51fa}\u{6570}\u{636e}".into(),
    ));
    assert!(text.contains("err.core.config"), "{text}");
    // When the original text is ASCII, it is kept. The address is still hidden, and the placeholder is English too.
    let text = user_text(&ZeppBridgeError::ConfigError(
        "unknown data type 'food' (https://api-mifit.huami.com/x)".into(),
    ));
    assert!(text.contains("unknown data type 'food'"), "{text}");
    assert!(text.contains("[address hidden]"), "{text}");
}

/// All five export range errors come from core's real `validated()`. Core now returns English, so the CLI
/// must pass each one through unchanged, with no Chinese.
#[test]
fn every_export_scope_error_is_translated() {
    let date = |start: &str, end: &str| ExportScope::DateRange {
        start: start.into(),
        end: end.into(),
    };
    for scope in [
        date("nope", "2026-01-01"),
        date("2026-01-01", "nope"),
        date("2026-02-01", "2026-01-01"),
        date("2024-01-01", "2026-01-01"),
        ExportScope::Workout {
            workout_id: "  ".into(),
        },
    ] {
        let message = scope.validated().unwrap_err();
        let text = translate_export_scope_error(message.clone());
        assert_eq!(text, message, "{message} should pass through unchanged");
        assert!(!text.chars().any(is_cjk), "{text}");
    }
}
