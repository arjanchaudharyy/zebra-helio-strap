use super::*;
use serde_json::json;
use std::collections::HashSet;

#[allow(clippy::too_many_arguments)]
fn synthetic_62_col_row(
    idx: usize,
    sec: i64,
    hr: i64,
    kcal: f64,
    weight: f64,
    reps: i32,
    unit: i32,
    action: i32,
    duration_ms: i64,
) -> String {
    let mut cols = vec!["-1".to_string(); 62];
    cols[0] = idx.to_string();
    cols[1] = sec.to_string();
    cols[2] = "0".to_string();
    cols[3] = "s00000000000".to_string();
    cols[4] = hr.to_string();
    cols[5] = "0".to_string();
    cols[6] = "-20000".to_string();
    cols[9] = "0.0".to_string();
    cols[10] = "0.0".to_string();
    cols[12] = "0.0".to_string();
    cols[13] = "0".to_string();
    cols[14] = "0".to_string();
    cols[15] = kcal.to_string();
    cols[16] = "0".to_string();
    cols[18] = "0".to_string();
    cols[19] = "0".to_string();
    cols[20] = "0".to_string();
    cols[21] = format!("{weight:.1}");
    cols[22] = reps.to_string();
    cols[23] = unit.to_string();
    cols[24] = "0".to_string();
    cols[25] = "0.0".to_string();
    cols[26] = "0".to_string();
    cols[28] = action.to_string();
    cols[43] = duration_ms.to_string();
    cols.join(",")
}

#[test]
fn strength_catalog_has_125_sorted_unique_entries_and_honest_unknowns() {
    assert_eq!(STRENGTH_ACTION_CATALOG.len(), 125);
    let allowed: HashSet<&str> = STRENGTH_MUSCLE_GROUPS.into_iter().collect();
    let mut keys = HashSet::new();
    let mut prev = -1;
    for &(code, key, muscle) in &STRENGTH_ACTION_CATALOG {
        assert!(code > prev, "catalog must stay strictly sorted at {code}");
        prev = code;
        assert!(!key.is_empty());
        assert!(keys.insert(key), "duplicate exercise_key: {key}");
        if let Some(group) = muscle {
            assert!(allowed.contains(group), "unexpected muscle_group: {group}");
        }
        assert_eq!(resolve_strength_action(code), (Some(key), muscle));
    }
    for unknown in [-1, 27, 28, 29, 31, 36, 59, 131, 1030, 1451] {
        assert_eq!(resolve_strength_action(unknown), (None, None));
    }
}

#[test]
fn parses_mixed_kg_and_lb_sets_and_preserves_unknown_action_codes() {
    let lap = format!(
        "{};{};{};",
        synthetic_62_col_row(0, 22, 92, 4.0, 20.0, 10, 0, 65, 22_450),
        synthetic_62_col_row(1, 24, 96, 5.0, 20.0, 10, 1, 14, 24_120),
        synthetic_62_col_row(2, 30, 0, -1.0, 0.0, 15, 0, 1030, -1),
    );
    let data = json!({
        "lap": lap,
        "strengthSets": "[]",
    });
    let sets = parse_strength_sets(data.as_object().unwrap());
    assert_eq!(sets.len(), 3);

    assert_eq!(sets[0].set_index, 1);
    assert_eq!(sets[0].action_code, 65);
    assert_eq!(sets[0].exercise_key.as_deref(), Some("biceps_curl"));
    assert_eq!(sets[0].muscle_group.as_deref(), Some("biceps"));
    assert_eq!(sets[0].weight_value, Some(20.0));
    assert_eq!(sets[0].weight_unit, "kg");
    assert_eq!(sets[0].weight_kg, Some(20.0));
    assert_eq!(sets[0].duration_ms, Some(22_450));
    assert_eq!(sets[0].avg_hr, Some(92));
    assert_eq!(sets[0].calories_kcal, Some(4.0));

    assert_eq!(sets[1].set_index, 2);
    assert_eq!(sets[1].action_code, 14);
    assert_eq!(sets[1].exercise_key.as_deref(), Some("lateral_raises"));
    assert_eq!(sets[1].muscle_group.as_deref(), Some("shoulders"));
    assert_eq!(sets[1].weight_value, Some(20.0));
    assert_eq!(sets[1].weight_unit, "lb");
    assert_eq!(sets[1].weight_kg, Some(9.072));

    // Never guess a name for the off-table code 1030. A 0 heart rate and the negative kcal sentinel become None. Missing milliseconds fall back to whole seconds * 1000.
    assert_eq!(sets[2].action_code, 1030);
    assert_eq!(sets[2].exercise_key, None);
    assert_eq!(sets[2].muscle_group, None);
    assert_eq!(sets[2].weight_value, None);
    assert_eq!(sets[2].weight_kg, None);
    assert_eq!(sets[2].duration_ms, Some(30_000));
    assert_eq!(sets[2].avg_hr, None);
    assert_eq!(sets[2].calories_kcal, None);

    let detail =
        build_strength_workout_detail(sets, true, Some(3), Some(35), Some(45), None).unwrap();
    assert_eq!(detail.total_sets, 3);
    assert_eq!(detail.total_reps, 35);
    // 10 * 20.0 + 10 * 9.072 = 290.72 kg
    assert_eq!(detail.total_volume_kg, Some(290.72));
    assert_eq!(detail.rest_seconds, Some(45));
    assert_eq!(detail.exercise_groups.len(), 3);
}

#[test]
fn running_laps_never_look_like_strength_sets() {
    let data = json!({
        "lap": "0,300,1000,s00000000000,148,300,-20000;1,295,1000,s00000000000,154,595,-20000;",
    });
    assert!(parse_strength_sets(data.as_object().unwrap()).is_empty());
}

/// Bodyweight exercises (weight and unit are -1), timed exercises (reps are -1) and rest rows (exercise, reps and weight are all -1)
/// between weighted sets: the weighted sets still carry their weight, and one row no longer clears the whole workout (#170).
#[test]
fn bodyweight_timed_and_rest_rows_do_not_wipe_the_weighted_sets() {
    let lap = [
        synthetic_62_col_row(0, 40, 101, 6.0, 60.0, 8, 0, 65, 40_200),
        synthetic_62_col_row(1, 90, 88, 1.0, -1.0, -1, -1, -1, 90_000),
        synthetic_62_col_row(2, 35, 104, 5.0, -1.0, 12, -1, 14, 35_000),
        synthetic_62_col_row(3, 45, 110, 4.0, 0.0, -1, 0, 65, 45_000),
        synthetic_62_col_row(4, 38, 106, 6.0, 62.5, 6, 0, 65, 38_400),
    ]
    .join(";");
    let data = json!({ "lap": lap });
    let sets = parse_strength_sets(data.as_object().unwrap());
    assert_eq!(sets.len(), 4, "rest rows do not count as a set");
    assert_eq!(
        sets.iter().map(|s| s.set_index).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(sets[0].weight_kg, Some(60.0));
    // Bodyweight: has reps, no weight.
    assert_eq!((sets[1].reps, sets[1].weight_kg), (12, None));
    assert_eq!(sets[1].weight_unit, "kg");
    // Timed: reps -1 become 0, the duration stays.
    assert_eq!((sets[2].reps, sets[2].duration_ms), (0, Some(45_000)));
    assert_eq!(sets[3].weight_kg, Some(62.5));

    let detail = build_strength_workout_detail(sets, true, None, None, None, None).unwrap();
    // 8 * 60 + 6 * 62.5 = 855 kg. Bodyweight and timed sets add no load.
    assert_eq!(detail.total_volume_kg, Some(855.0));
}

/// With a strength marker in the payload, rows parse even when the distance / elapsed-seconds columns are not 0. Without the marker, they are still rejected as running laps.
#[test]
fn strength_envelope_relaxes_the_running_lap_guard_only_when_present() {
    let row = |idx: usize| {
        let mut cols: Vec<String> =
            synthetic_62_col_row(idx, 30, 100, 3.0, 20.0, 10, 0, 65, 30_000)
                .split(',')
                .map(str::to_string)
                .collect();
        cols[5] = (30 * (idx + 1)).to_string();
        cols.join(",")
    };
    let lap = format!("{};{}", row(0), row(1));
    let bare = json!({ "lap": lap });
    assert!(parse_strength_sets(bare.as_object().unwrap()).is_empty());

    let marked = json!({ "lap": lap, "strengthAssess": "[{\"eq\":[]}]" });
    let sets = parse_strength_sets(marked.as_object().unwrap());
    assert_eq!(sets.len(), 2);
    assert_eq!(sets[1].weight_kg, Some(20.0));
}

/// Running laps with a distance and enough columns are still not read as strength sets.
#[test]
fn long_running_lap_rows_with_distance_are_still_rejected() {
    let row = |idx: usize| {
        let mut cols: Vec<String> =
            synthetic_62_col_row(idx, 300, 150, 60.0, 0.0, 0, 0, 0, 300_000)
                .split(',')
                .map(str::to_string)
                .collect();
        cols[2] = "1000".into();
        cols[5] = (300 * (idx + 1)).to_string();
        cols[28] = "-1".into();
        cols.join(",")
    };
    let data = json!({ "lap": format!("{};{}", row(0), row(1)) });
    assert!(parse_strength_sets(data.as_object().unwrap()).is_empty());
}
