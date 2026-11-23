//! Prepare the task bundle: attachments, exclusions, export content and redaction.

use super::*;

#[test]
fn attachments_report_ok_changed_missing() {
    let dir = temp_dir("att");
    let file = dir.join("report.pdf");
    std::fs::write(&file, b"12345").unwrap();
    let file_text = file.to_string_lossy().into_owned();

    let refs = vec![
        AiTaskAttachmentRef {
            id: "ok".into(),
            path: file_text.clone(),
            display_name: "report.pdf".into(),
            kind: AiTaskAttachmentKind::Pdf,
            byte_len: Some(5),
            added_at: "2026-09-01T00:00:00Z".into(),
        },
        AiTaskAttachmentRef {
            id: "changed".into(),
            path: file_text.clone(),
            display_name: "report.pdf".into(),
            kind: AiTaskAttachmentKind::Pdf,
            byte_len: Some(999),
            added_at: "2026-09-01T00:00:00Z".into(),
        },
        AiTaskAttachmentRef {
            id: "gone".into(),
            path: dir.join("missing.pdf").to_string_lossy().into_owned(),
            display_name: "gone.pdf".into(),
            kind: AiTaskAttachmentKind::Pdf,
            byte_len: Some(5),
            added_at: "2026-09-01T00:00:00Z".into(),
        },
        AiTaskAttachmentRef {
            id: "no-baseline".into(),
            path: file_text.clone(),
            display_name: "report.pdf".into(),
            kind: AiTaskAttachmentKind::Pdf,
            byte_len: None,
            added_at: "2026-09-01T00:00:00Z".into(),
        },
    ];
    let statuses = stat_task_attachments(&refs);
    assert_eq!(statuses[0].status, AiTaskAttachmentState::Ok);
    assert_eq!(statuses[0].byte_len, Some(5));
    assert_eq!(statuses[1].status, AiTaskAttachmentState::Changed);
    assert_eq!(statuses[2].status, AiTaskAttachmentState::Missing);
    assert_eq!(statuses[2].byte_len, None);
    // A baseline with byte_len=None cannot detect change. If the file exists, it is ok.
    assert_eq!(statuses[3].status, AiTaskAttachmentState::Ok);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prepare_blocks_on_missing_attachment_and_writes_nothing() {
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let dir = temp_dir("blocked");

    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Recovery, 3, true)];
    t.attachments = vec![AiTaskAttachmentRef {
        id: "a1".into(),
        path: dir.join("ghost.pdf").to_string_lossy().into_owned(),
        display_name: "Medical report.pdf".into(),
        kind: AiTaskAttachmentKind::Pdf,
        byte_len: Some(10),
        added_at: "2026-09-01T00:00:00Z".into(),
    }];
    let result = db.ai_task_prepare(&t, "Range note", &dir).unwrap();
    assert_eq!(result.status, AiTaskPrepareStatus::Blocked);
    assert!(result.json_path.is_none() && result.prompt_path.is_none());
    assert_eq!(
        result.blocked[0].code, "err.ai_task.attachment_missing",
        "P2 final spec: a missing attachment lists this code in blocked"
    );
    // When blocked, no file may be written. Not even the folder may be created.
    assert!(!std::path::Path::new(&result.output_dir).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_workouts_means_recent_days_ending_today() {
    let db = Database::in_memory().unwrap();
    let today = Local::now().date_naive();
    insert_daily(&db, "resting_hr", today - Duration::days(1), 52.0);
    insert_daily(&db, "resting_hr", today - Duration::days(30), 60.0);
    let dir = temp_dir("noworkout");

    let mut t = task();
    t.categories = vec![range(AiTaskCategory::Recovery, 6, false)];
    let preview = db.ai_task_preview(&t).unwrap();
    assert!(preview
        .warnings
        .iter()
        .all(|w| w.code != "ui.ai_task.blocked.no_workouts"));
    assert_eq!(preview.coverage.len(), 1);
    let row = &preview.coverage[0];
    assert_eq!(row.workout_id, None);
    // With no workouts, include_workout_day has no effect: today is always in the window.
    assert_eq!(row.end_date, today.to_string());
    assert_eq!(row.start_date, (today - Duration::days(6)).to_string());
    assert_eq!(row.days_in_range, 7);
    assert_eq!(row.days_with_data, 1, "the entry from 30 days ago is not in the last 7 days");

    let result = db.ai_task_prepare(&t, "", &dir).unwrap();
    assert_eq!(result.status, AiTaskPrepareStatus::Ready);
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(result.json_path.unwrap()).unwrap()).unwrap();
    assert_eq!(doc["task"]["window_anchor"], "today");
    assert!(doc["workouts"].as_array().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn excluded_metrics_are_counted_but_not_exported() {
    let db = Database::in_memory().unwrap();
    let start = utc(2026, 9, 10, 10);
    insert_workout(&db, "w1", start);
    let anchor_day = local_day(start);
    insert_daily(&db, "resting_hr", anchor_day, 51.0);
    insert_daily(&db, "stress", anchor_day - Duration::days(1), 30.0);
    insert_sleep(&db, "s1", utc(2026, 9, 10, 6));
    let dir = temp_dir("exclude");

    let mut recovery = range(AiTaskCategory::Recovery, 3, true);
    recovery.excluded_metrics = vec!["stress".into(), " stress ".into()];
    let mut sleep = range(AiTaskCategory::Sleep, 3, true);
    sleep.excluded_metrics = vec!["rem_minutes".into()];
    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![recovery, sleep];

    // Normalization: trim spaces, dedupe.
    let normalized = normalize_task(&t).unwrap();
    assert_eq!(
        normalized.categories[0].excluded_metrics,
        vec!["stress".to_string()]
    );

    let preview = db.ai_task_preview(&t).unwrap();
    let rec = preview
        .coverage
        .iter()
        .find(|r| r.category == AiTaskCategory::Recovery)
        .unwrap();
    // Excluded metrics still give day counts (the UI shows "how many days if dragged back"), but they do not count as covered.
    assert_eq!(rec.metric_days.get("stress"), Some(&1));
    assert_eq!(rec.metric_days.get("resting_hr"), Some(&1));
    assert_eq!(rec.days_with_data, 1);
    let sl = preview
        .coverage
        .iter()
        .find(|r| r.category == AiTaskCategory::Sleep)
        .unwrap();
    assert!(sl.metric_days.get("rem_minutes").copied().unwrap_or(0) >= 1);

    let result = db.ai_task_prepare(&t, "", &dir).unwrap();
    let json_text = std::fs::read_to_string(result.json_path.unwrap()).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&json_text).unwrap();
    let context = doc["context"].as_array().unwrap();
    let metrics: Vec<String> = context
        .iter()
        .find(|c| c["category"] == "recovery")
        .unwrap()["days"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|d| d["metrics"].as_array().cloned().unwrap_or_default())
        .map(|m| m["metric"].as_str().unwrap().to_string())
        .collect();
    assert!(metrics.iter().any(|m| m == "resting_hr"));
    assert!(!metrics.iter().any(|m| m == "stress"));
    let sleep_days = context.iter().find(|c| c["category"] == "sleep").unwrap()["days"]
        .as_array()
        .unwrap();
    for day in sleep_days {
        for session in day["sleeps"].as_array().unwrap() {
            assert!(session.get("rem_minutes").is_none(), "excluded fields are not exported");
            assert!(session.get("deep_minutes").is_some(), "other fields stay as usual");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn template_direction_and_question_are_combined() {
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let dir = temp_dir("direction");
    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Sleep, 3, true)];
    t.template_id = Some("recovery_run".into());
    t.prompt = "Focus on the Wednesday session".into();

    // The frontend gave a localized direction part: it and the question are both present, in the order direction -> question -> coverage note.
    let result = db
        .ai_task_prepare_plan(
            &t,
            "Coverage part",
            &crate::ai_tasks::export::AiTaskPromptParts {
                direction: Some("Direction: recovery"),
                ..Default::default()
            },
            &dir,
        )
        .unwrap()
        .finish()
        .unwrap();
    let text = &result.prompt_text;
    let (d, q, c) = (
        text.find("Direction: recovery").unwrap(),
        text.find("Focus on the Wednesday session").unwrap(),
        text.find("Coverage part").unwrap(),
    );
    assert!(d < q && q < c, "{text}");

    // With no direction part (CLI), fall back to the template's own text. The question stays.
    let result = db.ai_task_prepare(&t, "", &dir).unwrap();
    assert!(result.prompt_text.to_lowercase().contains("recovery run"));
    assert!(result.prompt_text.contains("Focus on the Wednesday session"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn brief_leads_the_prompt_and_files_take_the_given_names() {
    use crate::ai_tasks::export::AiTaskPromptParts;
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let dir = temp_dir("brief");
    let mut t = task();
    t.categories = vec![range(AiTaskCategory::Sleep, 3, true)];

    // No question and no template: the task brief must come first and the coverage note last, so the prompt
    // itself says what the AI must do. File names follow the stem the frontend gave, with illegal characters cleaned.
    let parts = AiTaskPromptParts {
        brief: Some("Task brief: start the analysis now"),
        data_stem: Some("ZeppBridge_0914-0927_Sleep:HR"),
        prompt_stem: Some("ZeppBridge_0914-0927_Sleep:HR_prompt"),
        ..Default::default()
    };
    let result = db
        .ai_task_prepare_plan(&t, "Coverage part", &parts, &dir)
        .unwrap()
        .finish()
        .unwrap();
    let text = &result.prompt_text;
    assert!(
        text.find("Task brief").unwrap() < text.find("Coverage part").unwrap(),
        "{text}"
    );
    let json = result.json_path.unwrap();
    let prompt = result.prompt_path.unwrap();
    assert!(
        json.ends_with("ZeppBridge_0914-0927_Sleep_HR.json"),
        "{json}"
    );
    assert!(
        prompt.ends_with("ZeppBridge_0914-0927_Sleep_HR_prompt.txt"),
        "{prompt}"
    );
    assert_eq!(std::fs::read_to_string(&prompt).unwrap(), *text);

    // A hand-edited final prompt replaces brief + direction + question. The coverage note is still added after it.
    t.prompt = "The original question".into();
    let edited = AiTaskPromptParts {
        brief: Some("Task brief"),
        override_text: Some("My own edited text"),
        ..Default::default()
    };
    let result = db
        .ai_task_prepare_plan(&t, "Coverage part", &edited, &dir)
        .unwrap()
        .finish()
        .unwrap();
    assert_eq!(
        result.prompt_text,
        "My own edited text

Coverage part"
    );
    assert!(result.json_path.unwrap().ends_with("health-context.json"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prepare_writes_a_named_folder_and_copies_attachments() {
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let dir = temp_dir("folder");
    let source_dir = temp_dir("folder-src");
    let pdf = source_dir.join("a.pdf");
    let png = source_dir.join("b.png");
    std::fs::write(&pdf, b"pdf-bytes").unwrap();
    std::fs::write(&png, b"png").unwrap();
    let attachment = |id: &str, path: &std::path::Path, name: &str, len: i64| AiTaskAttachmentRef {
        id: id.into(),
        path: path.to_string_lossy().into_owned(),
        display_name: name.into(),
        kind: AiTaskAttachmentKind::Pdf,
        byte_len: Some(len),
        added_at: "2026-09-01T00:00:00Z".into(),
    };

    let mut t = task();
    t.title = "Recovery run: 9/24?".into();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Recovery, 3, true)];
    t.attachments = vec![
        attachment("a1", &pdf, "Medical report.pdf", 9),
        attachment("a2", &png, "Medical report.pdf", 3),
    ];
    let result = db.ai_task_prepare(&t, "", &dir).unwrap();
    assert_eq!(result.status, AiTaskPrepareStatus::Ready);
    let folder = std::path::Path::new(&result.output_dir);
    let name = folder.file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("Recovery run_ 9_24_"),
        "letters are kept and illegal characters fold to _: {name}"
    );
    assert_eq!(folder.parent().unwrap(), dir.as_path());
    assert_eq!(result.copied_attachments, 2);
    let copied = folder.join("attachments");
    assert_eq!(
        std::fs::read(copied.join("Medical report.pdf")).unwrap(),
        b"pdf-bytes"
    );
    assert_eq!(
        std::fs::read(copied.join("Medical report (2).pdf")).unwrap(),
        b"png"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&source_dir);
}

#[test]
fn public_export_never_contains_attachment_paths_or_device_ids() {
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let anchor_day = local_day(utc(2026, 9, 10, 10));
    insert_daily(&db, "resting_hr", anchor_day, 50.0);
    insert_sleep(&db, "s1", utc(2026, 9, 10, 22));
    insert_sample(&db, "heart_rate", utc(2026, 9, 10, 12), 62.0);

    let dir = temp_dir("privacy");
    let secret_path = dir.join("top-secret-medical-report.pdf");
    std::fs::write(&secret_path, b"abc").unwrap();

    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![
        range(AiTaskCategory::Sleep, 3, true),
        range(AiTaskCategory::HeartRate, 3, true),
        range(AiTaskCategory::Attachment, 0, true),
    ];
    t.attachments = vec![AiTaskAttachmentRef {
        id: "a1".into(),
        path: secret_path.to_string_lossy().into_owned(),
        display_name: "Medical report.pdf".into(),
        kind: AiTaskAttachmentKind::Pdf,
        byte_len: Some(3),
        added_at: "2026-09-01T00:00:00Z".into(),
    }];

    let result = db.ai_task_prepare(&t, "Coverage note", &dir).unwrap();
    assert_eq!(result.status, AiTaskPrepareStatus::Ready);
    let json_text = std::fs::read_to_string(result.json_path.unwrap()).unwrap();

    // A local path never appears in the export JSON. Even its value cannot be found.
    assert!(!json_text.contains("top-secret-medical-report"));
    let doc: serde_json::Value = serde_json::from_str(&json_text).unwrap();
    let attachment = &doc["attachments"][0];
    assert_eq!(attachment["display_name"], "Medical report.pdf");
    assert!(attachment.get("path").is_none());
    assert!(attachment.get("id").is_none());

    // Identity keys such as device_id are not in the field list from the start.
    assert!(!json_text.contains("device_id"));

    // prompt.txt is written too.
    let prompt = std::fs::read_to_string(result.prompt_path.unwrap()).unwrap();
    assert!(prompt.contains("Check my recent state"));
    assert!(prompt.contains("Coverage note"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prepare_strips_local_paths_from_user_note_and_prompt() {
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let dir = temp_dir("sanitize-user-text");

    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Recovery, 3, true)];
    t.personal_note = "The original report is at C:\\Users\\me\\secret-note.pdf now".into();
    t.prompt = "Help me analyze it, see D:\\private-dir\\old.json".into();

    let result = db.ai_task_prepare(&t, "", &dir).unwrap();
    assert_eq!(result.status, AiTaskPrepareStatus::Ready);

    // Neither the export JSON nor the returned prompt_text may still carry a local path.
    let json_text = std::fs::read_to_string(result.json_path.unwrap()).unwrap();
    assert!(
        !json_text.contains("secret-note"),
        "the file name in the note must not be exported"
    );
    let doc: serde_json::Value = serde_json::from_str(&json_text).unwrap();
    let note = doc["task"]["personal_note"].as_str().unwrap();
    assert!(!note.contains("C:\\"));
    assert!(note.contains("[local path removed]"));

    assert!(!result.prompt_text.contains("private-dir"));
    assert!(result.prompt_text.contains("[local path removed]"));
    // A URL is not a local path. Cleaning must not damage it.
    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Recovery, 3, true)];
    t.prompt = "Use the method at https://example.com/report please".into();
    let result = db.ai_task_prepare(&t, "", &dir).unwrap();
    assert!(result.prompt_text.contains("https://example.com/report"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn preview_estimated_bytes_matches_pretty_written_file() {
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let dir = temp_dir("estimate");

    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Recovery, 3, true)];

    let preview = db.ai_task_preview(&t).unwrap();
    let prepared = db.ai_task_prepare(&t, "", &dir).unwrap();
    assert_eq!(prepared.status, AiTaskPrepareStatus::Ready);
    // Estimate and actual write must match in size. The generated_at timestamp in the document can
    // differ by a few decimals between two builds (AutoSi 3/6/9 digits). The tolerance is for that; format differences can be larger.
    let drift = (preview.estimated_bytes - prepared.byte_len).abs();
    assert!(
        drift <= 16,
        "estimate and actual write may differ only in timestamp digits (drift={drift})"
    );
    let on_disk = std::fs::metadata(prepared.json_path.unwrap())
        .unwrap()
        .len() as i64;
    assert_eq!(
        prepared.byte_len, on_disk,
        "byte_len must equal the actual size of the written file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prepare_uses_template_prompt_when_user_prompt_empty() {
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let dir = temp_dir("prompt");
    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Sleep, 3, true)];
    t.prompt = String::new();
    t.template_id = Some("recovery_run".into());
    let result = db.ai_task_prepare(&t, "Notes part", &dir).unwrap();
    assert!(result.prompt_text.to_lowercase().contains("recovery run"));
    assert!(result.prompt_text.contains("Notes part"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn precise_gps_opt_in_gates_route_key() {
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let dir = temp_dir("gps");

    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Workout, 0, true)];
    t.detail_level = AiTaskDetailLevel::Detailed;

    // Off by default: the route key does not appear at all.
    let result = db.ai_task_prepare(&t, "", &dir).unwrap();
    let json_text = std::fs::read_to_string(result.json_path.unwrap()).unwrap();
    assert!(!json_text.contains("\"route\""));

    // Only an explicit switch adds it.
    t.include_precise_gps = true;
    let result = db.ai_task_prepare(&t, "", &dir).unwrap();
    let json_text = std::fs::read_to_string(result.json_path.unwrap()).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&json_text).unwrap();
    assert!(doc["workouts"][0]["series"].get("route").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------- MCP grant windows ----------

/// Batch 7: with the markdown parameter, deliver only one `.md`: the prompt first, then reading notes, then data.
/// No JSON + txt any more. The size the preview estimates with the same budget matches the actual write.
#[test]
fn markdown_handoff_writes_one_file_with_prompt_guide_and_data() {
    use crate::ai_tasks::export::{AiTaskMarkdownParts, AiTaskPromptParts};
    let db = Database::in_memory().unwrap();
    insert_workout(&db, "w1", utc(2026, 9, 10, 10));
    let dir = temp_dir("markdown");
    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![
        range(AiTaskCategory::Sleep, 3, true),
        range(AiTaskCategory::Workout, 0, true),
    ];
    let parts = AiTaskPromptParts {
        brief: Some("BRIEF"),
        data_stem: Some("handoff"),
        markdown: Some(AiTaskMarkdownParts {
            guide: Some("GUIDE"),
            token_budget: 30_000,
        }),
        ..Default::default()
    };
    let result = db
        .ai_task_prepare_plan(&t, "COVERAGE", &parts, &dir)
        .unwrap()
        .finish()
        .unwrap();
    assert_eq!(result.status, AiTaskPrepareStatus::Ready);
    assert!(result.json_path.is_none() && result.prompt_path.is_none());
    let md_path = result.md_path.clone().unwrap();
    assert!(md_path.ends_with("handoff.md"), "{md_path}");
    let text = std::fs::read_to_string(&md_path).unwrap();
    let brief = text.find("BRIEF").unwrap();
    let guide = text.find("GUIDE").unwrap();
    let data = text.find("# Data").unwrap();
    assert!(brief < guide && guide < data, "{text}");
    assert!(text.contains("### Selected workouts"));
    let estimate = result.markdown.unwrap();
    assert_eq!(estimate.byte_len, text.len() as i64);
    assert!(!estimate.over_budget);
    // The folder holds only this one file.
    let files: Vec<_> = std::fs::read_dir(std::path::Path::new(&md_path).parent().unwrap())
        .unwrap()
        .collect();
    assert_eq!(files.len(), 1);

    let preview = db.ai_task_preview_with_budget(&t, Some(30_000)).unwrap();
    let preview_md = preview.markdown.unwrap();
    assert!(preview_md.approx_tokens <= estimate.approx_tokens);
    assert!(db.ai_task_preview(&t).unwrap().markdown.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Code review R01: when stage minutes are excluded, the detailed stage timeline is not exported either,
/// because the stage slices give back the excluded minutes directly. MCP uses the same table.
#[test]
fn excluded_stage_minutes_drop_the_stage_timeline_too() {
    let db = Database::in_memory().unwrap();
    let start = utc(2026, 9, 10, 10);
    insert_workout(&db, "w1", start);
    let end = utc(2026, 9, 10, 6);
    db.insert_sleep_session(&SleepSession {
        sleep_id: "s1".into(),
        start_time: end - Duration::hours(7),
        end_time: end,
        score: Some(82),
        duration_minutes: 400,
        deep_minutes: Some(90),
        light_minutes: Some(220),
        rem_minutes: Some(70),
        awake_minutes: Some(20),
        source_scope: SourceScope::Device,
        device_id: None,
        synced_at: None,
        time_in_bed_minutes: None,
        stages: vec![crate::models::SleepStageSlice {
            stage: "deep".into(),
            start_time: end - Duration::hours(6),
            end_time: end - Duration::hours(5),
            raw_mode: None,
        }],
        wake_count: Some(2),
    })
    .unwrap();

    let sessions = |excluded: Vec<String>| {
        let mut sleep = range(AiTaskCategory::Sleep, 3, true);
        sleep.excluded_metrics = excluded;
        let mut t = task();
        t.workout_ids = vec!["w1".into()];
        t.detail_level = AiTaskDetailLevel::Detailed;
        t.categories = vec![sleep];
        let dir = temp_dir("stages");
        let result = db.ai_task_prepare(&t, "", &dir).unwrap();
        let text = std::fs::read_to_string(result.json_path.unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        doc["context"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["category"] == "sleep")
            .unwrap()["days"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|d| d["sleeps"].as_array().cloned().unwrap_or_default())
            .collect::<Vec<_>>()
    };

    let kept = sessions(Vec::new());
    assert!(
        kept.iter().any(|s| s.get("stages").is_some()),
        "control: stages exist when nothing is excluded"
    );
    let dropped = sessions(vec!["deep_minutes".into()]);
    assert!(!dropped.is_empty());
    for session in dropped {
        assert!(session.get("deep_minutes").is_none());
        assert!(session.get("stages").is_none(), "{session}");
        assert!(session.get("light_minutes").is_some());
    }
}

/// With the "Workout" category off, anchor workouts give no id / type / start day.
/// They only set the date window, and day rows and coverage rows keep no id of them. With it on, they appear as before, but
/// fields dragged out of the task are left out. When HR is excluded, HR zones and point HR go together.
#[test]
fn anchor_workouts_follow_the_workout_category_switch_and_its_exclusions() {
    let db = Database::in_memory().unwrap();
    let start = utc(2026, 9, 10, 10);
    insert_workout(&db, "w1", start);
    let export = |t: &AiTask| {
        let dir = temp_dir("anchor");
        let result = db.ai_task_prepare(t, "", &dir).unwrap();
        let text = std::fs::read_to_string(result.json_path.unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        doc
    };

    let mut off = task();
    off.workout_ids = vec!["w1".into()];
    off.detail_level = AiTaskDetailLevel::Detailed;
    off.categories = vec![range(AiTaskCategory::Recovery, 3, true)];
    let doc = export(&off);
    assert_eq!(doc["workouts"], serde_json::json!([]));
    let text = doc.to_string();
    assert!(
        !text.contains("\"w1\""),
        "the anchor workout id must not appear anywhere: {text}"
    );
    let recovery = doc["coverage"].as_array().unwrap();
    assert_eq!(recovery.len(), 1);
    assert!(recovery[0]["workout_id"].is_null());
    assert_eq!(
        recovery[0]["end_date"],
        local_day(start).to_string(),
        "the date window still anchors on the workout day"
    );

    let mut on = off.clone();
    let mut workout = range(AiTaskCategory::Workout, 0, true);
    workout.excluded_metrics = vec!["avg_hr".into()];
    on.categories.push(workout);
    let doc = export(&on);
    let anchor = doc["workouts"][0].as_object().unwrap();
    assert_eq!(anchor["workout_id"], "w1");
    assert!(anchor.get("avg_hr").is_none());
    assert!(anchor.get("hr_zones").is_none());
    assert!(anchor.get("max_hr").is_some(), "only excluded fields are removed");
    for sample in anchor["series"]["samples"].as_array().into_iter().flatten() {
        assert!(sample.get("heart_rate").is_none());
    }
}

/// 10-28: with the "Workout" category on, anchor workouts carry the two parts computed by the workout insight card and climb card (`comparison`, `climbs`).
/// When HR is excluded, the first/second half, the HR item and per-split average HR go together.
#[test]
fn anchor_workouts_carry_the_comparison_and_climbs_and_hide_heart_rate_on_request() {
    let db = Database::in_memory().unwrap();
    let start = utc(2026, 9, 10, 6);
    let seconds = 2400;
    db.insert_workout(&Workout {
        workout_id: "w1".into(),
        workout_type: "run".into(),
        normalized_type: "run".into(),
        type_source: "string_field".into(),
        effective_type: "run".into(),
        start_time: start,
        end_time: start + Duration::seconds(seconds),
        distance_meters: Some(7_200.0),
        avg_hr: Some(150),
        max_hr: Some(165),
        source_scope: SourceScope::Device,
        ..Default::default()
    })
    .unwrap();
    // Steady 3 m/s, HR rising all the way. The first 1000 seconds climb 200 m (3 km, about 6.7%), the next 1000 seconds descend, then flat.
    for second in 0..=seconds {
        let up = if second <= 1000 {
            second
        } else {
            (2000 - second).max(0)
        };
        let altitude = 50.0 + up as f64 * 0.2;
        db.conn
            .execute(
                "INSERT INTO workout_samples (workout_id, timestamp, heart_rate, speed, altitude)
                 VALUES ('w1', ?1, ?2, 3.0, ?3)",
                rusqlite::params![
                    (start + Duration::seconds(second)).to_rfc3339(),
                    (140.0 + 20.0 * second as f64 / seconds as f64).round() as i64,
                    altitude
                ],
            )
            .unwrap();
    }
    let export = |t: &AiTask| {
        let dir = temp_dir("extras");
        let result = db.ai_task_prepare(t, "", &dir).unwrap();
        let text = std::fs::read_to_string(result.json_path.unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        serde_json::from_str::<serde_json::Value>(&text).unwrap()
    };
    let mut t = task();
    t.workout_ids = vec!["w1".into()];
    t.categories = vec![range(AiTaskCategory::Workout, 0, true)];

    let doc = export(&t);
    let anchor = &doc["workouts"][0];
    assert!(
        !anchor["comparison"]["facts"].as_array().unwrap().is_empty(),
        "{anchor}"
    );
    assert!(
        anchor["comparison"]["heart_rate_drift"].is_object(),
        "{anchor}"
    );
    let segments = anchor["climbs"]["segments"].as_array().unwrap();
    assert!(
        segments.iter().any(|segment| segment["kind"] == "climb"),
        "{segments:?}"
    );

    t.categories[0].excluded_metrics = vec!["avg_hr".into()];
    let doc = export(&t);
    let anchor = &doc["workouts"][0];
    assert!(anchor["comparison"].get("heart_rate_drift").is_none());
    for fact in anchor["comparison"]["facts"].as_array().unwrap() {
        assert!(!fact["metric"].as_str().unwrap().contains("hr"), "{fact}");
    }
    for segment in anchor["climbs"]["segments"].as_array().unwrap() {
        assert!(segment.get("average_hr").is_none(), "{segment}");
    }
}
