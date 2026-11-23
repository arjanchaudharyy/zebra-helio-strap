use super::*;

#[test]
fn tool_failures_are_results_but_bad_envelopes_remain_rpc_errors() {
    for code in [ERR_DATABASE, ERR_NOT_CONFIGURED] {
        let result = call_tool_with_db(
            &json!({"name":"list_workouts"}),
            &AccessScope::FullReadOnly,
            || Err((code, "Local data is unavailable".into())),
        )
        .unwrap();
        assert_eq!(result["isError"], true);
        assert_eq!(result["content"][0]["text"], "Local data is unavailable");
        // Even when the library cannot open, scope is still reported openly (grants not read yet, so 0).
        assert_eq!(
            result["scope"],
            json!({"mode": "full-readonly", "grants": 0})
        );
    }
    let library = TestLibrary::empty();
    let invalid = call_tool_with_db(
        &json!({"name":"get_metric_series","arguments":{"metrics":[]}}),
        &AccessScope::FullReadOnly,
        || {
            Ok((
                Database::open_read_only(library.0.join("zepp.db")).unwrap(),
                0,
            ))
        },
    )
    .unwrap();
    assert_eq!(invalid["isError"], true);
    for params in [
        json!({}),
        json!({"name":"unknown"}),
        json!({"name":"list_workouts","arguments":[]}),
    ] {
        assert!(call_tool_with_db(&params, &AccessScope::FullReadOnly, || {
            panic!("invalid request must not open the database")
        })
        .is_err());
    }
}

#[test]
fn request_reader_enforces_limit_and_resumes_after_bad_lines() {
    let mut input = vec![b'x'; MAX_REQUEST_BYTES + 10];
    input.extend_from_slice(b"\n\xff\n{\"jsonrpc\":\"2.0\",\"id\":9,\"method\":\"ping\"}\n");
    let mut reader = io::BufReader::with_capacity(13, input.as_slice());
    let mut output = Vec::new();
    serve(&mut reader, &mut output, &AccessScope::FullReadOnly).unwrap();
    let responses: Vec<Value> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 3);
    assert_eq!(responses[0]["error"]["code"], -32600);
    assert_eq!(responses[1]["error"]["code"], -32700);
    assert_eq!(responses[2]["id"], 9);
    assert_eq!(responses[2]["result"], json!({}));
    let at_limit = vec![b' '; MAX_REQUEST_BYTES];
    let mut reader = io::Cursor::new(at_limit);
    assert!(
        matches!(read_frame(&mut reader).unwrap(), Some(RequestFrame::Message(bytes)) if bytes.len()==MAX_REQUEST_BYTES)
    );
}

#[test]
fn every_tool_declares_units_and_the_missing_value_rule() {
    // A health data tool that does not state units pushes unit conversion onto the model as a guess.
    for tool in tool_definitions() {
        let description = tool["description"].as_str().unwrap_or_default();
        let name = tool["name"].as_str().unwrap_or_default();
        assert!(
            description.contains("never filled with 0"),
            "the description of {name} does not state the missing-value rule"
        );
        assert!(
            tool["inputSchema"]["additionalProperties"] == json!(false),
            "{name} must reject unknown arguments, so the caller does not think some switch took effect"
        );
    }
}

#[test]
fn only_the_declared_writers_write_and_none_of_them_deletes() {
    // Read-only health data is the reason this process exists. Only the user's own ledgers are written (training plans, coach profile,
    // history and workout annotations) plus two actions, and the annotations must state it openly; adding a writing tool means updating this list first.
    const WRITERS: [&str; 11] = [
        "draft_training_plan",
        "publish_training_plan",
        "propose_coach_note",
        "save_injury",
        "save_life_event",
        "save_race",
        "save_shoe",
        "annotate_workout",
        "set_workout_type",
        "sync_now",
        "export_data",
    ];
    let tools = tool_definitions();
    for tool in &tools {
        let name = tool["name"].as_str().unwrap_or_default();
        let read_only = tool["annotations"]["readOnlyHint"].as_bool();
        for verb in ["delete", "remove", "drop", "purge", "clear"] {
            assert!(!name.contains(verb), "{name}: the local AI has no delete tool");
        }
        if WRITERS.contains(&name) {
            assert_eq!(read_only, Some(false), "{name} writes the library, so its annotation must say so");
            continue;
        }
        assert_eq!(read_only, Some(true), "{name} must declare read-only");
        for verb in [
            "sync", "delete", "write", "set", "update", "import", "restore", "publish", "draft",
        ] {
            assert!(
                !name.contains(verb),
                "{name} looks like it changes data and must not be here"
            );
        }
    }
    // Publishing replaces the plan on the watch: it must be marked destructive, so the client asks the user first.
    let publish = tools
        .iter()
        .find(|tool| tool["name"] == "publish_training_plan")
        .unwrap();
    assert_eq!(publish["annotations"]["destructiveHint"], true);
    // Remembering a note overwrites nothing (undo is on the desktop).
    let propose = tools
        .iter()
        .find(|tool| tool["name"] == "propose_coach_note")
        .unwrap();
    assert_eq!(propose["annotations"]["destructiveHint"], false);
    assert_eq!(tools.len(), 29);
}

#[test]
fn the_guide_names_exactly_the_registered_tools() {
    // The guide is for any MCP client to read: miss one tool and the AI does not know it; list an extra one and the AI calls a tool that does not exist.
    let registered: BTreeSet<String> = tool_definitions()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect();
    let prefixes = [
        "get_",
        "list_",
        "save_",
        "set_",
        "draft_",
        "publish_",
        "propose_",
        "annotate_",
        "sync_",
        "export_",
    ];
    let mentioned: BTreeSet<String> = protocol::GUIDE
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|token| {
            prefixes.iter().any(|prefix| token.starts_with(prefix))
                && token.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        })
        .map(str::to_string)
        .collect();
    assert_eq!(mentioned, registered);
}

#[test]
fn the_guide_is_listed_and_readable_as_a_resource() {
    let list = handle("resources/list", &json!({})).unwrap();
    assert_eq!(list["resources"][0]["uri"], "zeppbridge://guide");
    let read = handle("resources/read", &json!({ "uri": "zeppbridge://guide" })).unwrap();
    assert!(read["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("no delete tools"));
    assert!(handle("resources/read", &json!({ "uri": "zeppbridge://nope" })).is_err());
    let init = handle("initialize", &json!({})).unwrap();
    assert_eq!(init["capabilities"]["resources"], json!({}));
    assert!(init["instructions"]
        .as_str()
        .unwrap()
        .contains("zeppbridge://guide"));
}

#[test]
fn unknown_methods_and_tools_are_refused_rather_than_guessed() {
    let error = handle("tools/execute", &json!({})).unwrap_err();
    assert_eq!(error.code, ERR_METHOD_NOT_FOUND);
    let missing_name =
        call_tool(&json!({ "arguments": {} }), &AccessScope::FullReadOnly).unwrap_err();
    assert_eq!(missing_name.0, ERR_INVALID_PARAMS);
}

#[test]
fn initialize_tells_the_caller_the_privacy_boundary_up_front() {
    let result = handle("initialize", &json!({})).unwrap();
    let instructions = result["instructions"].as_str().unwrap();
    assert!(instructions.contains("does not listen on any port"));
    assert!(instructions.contains("never filled with 0"));
    assert_eq!(result["serverInfo"]["version"], json!(VERSION));
}

/// A modern client (no handshake) must be able to learn everything about this server from `server/discover` alone.
#[test]
fn server_discover_answers_a_modern_client_without_any_handshake() {
    let result = handle(
        "server/discover",
        &json!({
            "_meta": {
                "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientInfo": { "name": "probe", "version": "1.0" },
                "io.modelcontextprotocol/clientCapabilities": {}
            }
        }),
    )
    .unwrap();

    // Every result must carry resultType.
    assert_eq!(result["resultType"], json!("complete"));
    assert_eq!(
        result["supportedVersions"][0],
        json!(MODERN_PROTOCOL_VERSION)
    );
    assert_eq!(result["capabilities"]["tools"], json!({}));
    // Identity moved into _meta. It is no longer a top-level serverInfo.
    assert_eq!(result["_meta"][META_SERVER_INFO]["version"], json!(VERSION));
    assert!(result["instructions"]
        .as_str()
        .unwrap()
        .contains("does not listen on any port"));
    assert_eq!(result["cacheScope"], json!("public"));
}

/// `tools/list` with a version in `_meta` must answer by the new rules: resultType + cache hint.
#[test]
fn a_modern_tools_list_carries_the_required_envelope() {
    let modern = json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" } });
    let result = handle("tools/list", &modern).unwrap();
    assert_eq!(result["resultType"], json!("complete"));
    assert!(result["ttlMs"].as_i64().unwrap() > 0);
    assert_eq!(result["cacheScope"], json!("public"));
    assert_eq!(result["tools"].as_array().unwrap().len(), 29);
}

/// An unknown version must be refused clearly, **listing the versions we support**. The client uses
/// that list to pick one and retry. Quietly answering by some version is the worst result.
#[test]
fn an_unknown_protocol_version_is_refused_with_a_list_to_retry_from() {
    let error = handle(
        "tools/list",
        &json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "1900-01-01" } }),
    )
    .unwrap_err();
    assert_eq!(error.code, ERR_UNSUPPORTED_PROTOCOL_VERSION);
    let data = error.data.unwrap();
    assert_eq!(data["requested"], json!("1900-01-01"));
    assert!(data["supported"]
        .as_array()
        .unwrap()
        .contains(&json!(MODERN_PROTOCOL_VERSION)));
}

/// Old clients need no change at all. This test guards against "upgrading to the new protocol and breaking the old path along the way".
#[test]
fn a_legacy_initialize_still_works_and_echoes_a_version_it_asked_for() {
    let result = handle(
        "initialize",
        &json!({ "protocolVersion": "2025-06-18", "capabilities": {} }),
    )
    .unwrap();
    assert_eq!(result["protocolVersion"], json!("2025-06-18"));
    assert_eq!(result["serverInfo"]["version"], json!(VERSION));
    // Legacy results must not carry the modern envelope.
    assert!(result.get("resultType").is_none());

    // When the client asks for a version we do not support, reply with ours, and the client decides whether to continue.
    let fallback = handle("initialize", &json!({ "protocolVersion": "1900-01-01" })).unwrap();
    assert_eq!(fallback["protocolVersion"], json!(LEGACY_PROTOCOL_VERSION));

    // tools/list without _meta takes the legacy shape.
    let listed = handle("tools/list", &json!({})).unwrap();
    assert!(listed.get("resultType").is_none());
    assert!(listed.get("ttlMs").is_none());
}

/* ---------- Access scope (--scope) ---------- */

#[test]
fn argv_parsing_is_fail_closed() {
    // Old config: no arguments -> full-readonly.
    assert_eq!(parse_scope_args(&[]).unwrap(), AccessScope::FullReadOnly);
    for args in [
        vec!["--scope".to_string(), "task".to_string()],
        vec!["--scope=task".to_string()],
    ] {
        assert!(parse_scope_args(&args).unwrap().is_task_scoped());
    }
    assert_eq!(
        parse_scope_args(&["--scope".to_string(), "full-readonly".to_string()]).unwrap(),
        AccessScope::FullReadOnly
    );
    // Bad value, unknown argument, missing value, repeated, positional argument: all refused.
    for args in [
        vec!["--scope".to_string(), "bogus".to_string()],
        vec!["--scope=bogus".to_string()],
        vec!["--bogus".to_string()],
        vec!["--scope".to_string()],
        vec![
            "--scope".to_string(),
            "task".to_string(),
            "--scope".to_string(),
            "task".to_string(),
        ],
        vec!["anything".to_string()],
    ] {
        assert!(parse_scope_args(&args).is_err(), "{args:?} must be refused");
    }
}

/// R10: every tool in the registry must have a DataRequest builder. If a new tool lacks
/// a mapping, this test fails.
#[test]
fn every_registered_tool_builds_a_data_request() {
    let minimal_args: [(&str, Value); 29] = [
        ("list_workouts", json!({})),
        ("get_workout_insight", json!({"workoutId": "w"})),
        ("get_metric_series", json!({"metrics": ["spo2_odi"]})),
        ("get_sleep_detail", json!({})),
        ("get_data_health", json!({})),
        ("get_food_data", json!({})),
        ("list_available_metrics", json!({})),
        ("get_baselines", json!({"metrics": ["resting_hr"]})),
        (
            "get_metric_records",
            json!({"metric": "steps", "source": "daily_metrics"}),
        ),
        ("list_sleep_sessions", json!({})),
        ("get_workout_detail", json!({"workoutId": "w"})),
        ("get_workout_series", json!({"workoutId": "w"})),
        ("list_life_events", json!({})),
        ("get_strap_heart_rate", json!({})),
        ("get_strap_alerts", json!({})),
        ("get_training_context", json!({})),
        ("get_athlete_profile", json!({})),
        ("get_training_plan", json!({})),
        ("draft_training_plan", json!({"plan": {"workouts": []}})),
        ("publish_training_plan", json!({"draftId": "plan-x"})),
        ("propose_coach_note", json!({"note": "n"})),
        ("save_injury", json!({})),
        ("save_life_event", json!({})),
        ("save_race", json!({})),
        ("save_shoe", json!({})),
        ("annotate_workout", json!({"workoutId": "w"})),
        ("set_workout_type", json!({"workoutId": "w", "type": null})),
        ("sync_now", json!({})),
        (
            "export_data",
            json!({"startDate": "2026-01-01", "endDate": "2026-01-02"}),
        ),
    ];
    for tool in tool_definitions() {
        let name = tool["name"].as_str().unwrap();
        let args = minimal_args
            .iter()
            .find(|(tool_name, _)| *tool_name == name)
            .unwrap_or_else(|| panic!("{name} has no minimal argument fixture"))
            .1
            .clone();
        let request = build_request(name, &args)
            .unwrap_or_else(|error| panic!("{name} has no DataRequest mapping: {error}"));
        assert_eq!(request.tool, name);
    }
}

/// The handshake and every tools/call result describe the current scope (recon: visibility convention).
#[test]
fn handshake_and_results_disclose_the_active_scope() {
    let init = handle_scoped("initialize", &json!({}), &AccessScope::TaskScoped).unwrap();
    assert_eq!(init["scope"]["mode"], json!("task"));
    assert!(init["instructions"].as_str().unwrap().contains("task:"));

    let discover = handle_scoped(
        "server/discover",
        &json!({"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}}),
        &AccessScope::TaskScoped,
    )
    .unwrap();
    assert_eq!(discover["scope"]["mode"], json!("task"));

    let full = handle("initialize", &json!({})).unwrap();
    assert_eq!(full["scope"]["mode"], json!("full-readonly"));
    assert!(full["instructions"]
        .as_str()
        .unwrap()
        .contains("full-readonly"));
}
