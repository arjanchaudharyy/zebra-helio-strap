//! `fit-override` (#143): one command can replace, list and undo by script on every computer.
//! The corrected file is the one that `export --format fit` writes. We do not build a separate binary fixture.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::{Duration, TimeZone, Utc};
use serde_json::{json, Value};
use zeppbridge_core::decoder::workout_detail::WorkoutSample;
use zeppbridge_core::decoder::DecodedWorkout;
use zeppbridge_core::models::Workout;
use zeppbridge_core::storage::Database;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "zeppbridge-fit-override-{}-{}",
            std::process::id(),
            zeppbridge_core::unique_suffix()
        ));
        fs::create_dir(&dir).unwrap();
        let db = Database::open_migrated(&dir.join("zepp.db")).unwrap();
        let start = Utc.with_ymd_and_hms(2026, 10, 1, 6, 0, 0).unwrap();
        // The second one is one hour after the first: if we use the first one's .fit to replace it, the start time does not match.
        for (id, offset) in [("run-a", 0), ("run-b", 60)] {
            let start = start + Duration::minutes(offset);
            let end = start + Duration::minutes(30);
            let workout: Workout = serde_json::from_value(json!({
                "workout_id": id,
                "workout_type": "outdoor_running",
                "normalized_type": "outdoor_running",
                "type_source": "string_field",
                "effective_type": "outdoor_running",
                "start_time": start,
                "end_time": end,
                "distance_meters": 5000.0,
                "source_scope": "device",
            }))
            .unwrap();
            db.insert_workout(&workout).unwrap();
            let mut decoded = DecodedWorkout {
                start_time: start,
                end_time: end,
                ..Default::default()
            };
            for second in [0, 60, 120] {
                decoded.samples.push(
                    serde_json::from_value::<WorkoutSample>(json!({
                        "timestamp": start + Duration::seconds(second),
                        "heart_rate": 140
                    }))
                    .unwrap(),
                );
            }
            db.replace_workout_series(id, &decoded).unwrap();
        }
        Self(dir)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_zeppbridge-cli"))
            .args(args)
            .env("ZEPPBRIDGE_DATA_DIR", &self.0)
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> (Output, Value) {
        let output = self.run(args);
        let value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
        (output, value)
    }

    /// Use the CLI to export run-a's .fit and treat it as "the corrected file".
    fn exported_fit(&self) -> PathBuf {
        let out = self.0.join("fit-out");
        fs::create_dir_all(&out).unwrap();
        let output = self.run(&[
            "export",
            "--format",
            "fit",
            "--workout",
            "run-a",
            "--out",
            out.to_str().unwrap(),
        ]);
        assert!(output.status.success(), "{output:?}");
        first_fit(&out)
    }
}

fn first_fit(dir: &Path) -> PathBuf {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "fit"))
        .expect("export --format fit should write a .fit")
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn replace_list_and_undo_round_trip() {
    let fixture = Fixture::new();
    let fit = fixture.exported_fit();
    let fit = fit.to_str().unwrap();

    let (output, value) =
        fixture.json(&["fit-override", "--workout", "run-a", "--fit", fit, "--json"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(value["applied"], true);
    assert_eq!(value["override"]["workoutId"], "run-a");

    let (output, value) = fixture.json(&["fit-override", "--list", "--json"]);
    assert!(output.status.success(), "{output:?}");
    let list = value["overrides"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["workoutId"], "run-a");

    let (output, value) =
        fixture.json(&["fit-override", "--workout", "run-a", "--remove", "--json"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(value["removed"], true);
    // Undo again: nothing to undo, it still succeeds. A script rerun must not fail.
    let (output, value) =
        fixture.json(&["fit-override", "--workout", "run-a", "--remove", "--json"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(value["removed"], false);
    let (_, value) = fixture.json(&["fit-override", "--list", "--json"]);
    assert_eq!(value["overrides"].as_array().unwrap().len(), 0);
}

#[test]
fn refuses_a_fit_that_is_not_the_same_workout_and_bad_usage() {
    let fixture = Fixture::new();
    let fit = fixture.exported_fit();
    let fit = fit.to_str().unwrap();

    let (output, value) =
        fixture.json(&["fit-override", "--workout", "run-b", "--fit", fit, "--json"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(value["errorKind"], "fit_start_mismatch");

    let (output, value) = fixture.json(&[
        "fit-override",
        "--workout",
        "missing",
        "--fit",
        fit,
        "--json",
    ]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(value["errorKind"], "workout_not_found");

    // Mixing both forms, or giving nothing, is a usage error (2), not a "failure".
    for args in [
        &["fit-override", "--list", "--remove", "--json"][..],
        &["fit-override", "--workout", "run-a", "--json"][..],
    ] {
        let (output, value) = fixture.json(args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert_eq!(value["errorKind"], "usage");
    }
    // The failed attempts wrote nothing.
    let (_, value) = fixture.json(&["fit-override", "--list", "--json"]);
    assert_eq!(value["overrides"].as_array().unwrap().len(), 0);
}
