use super::*;

#[test]
fn code_search_loop_identity_normalises_query_filters_and_limit() {
    let history = ToolExecutionHistory::new(10);
    history.set_loop_detection_limits(5, 2);

    let args = json!({
        "query": "exec_only_policy",
        "path": "crates/codegen/vtcode-core/src/core/agent/runner/tests.rs",
        "file_types": ["rust"],
        "result_types": ["definition", "usage"],
        "max_results": 5
    });

    // With MIN_READONLY_IDENTICAL_LIMIT=2, two successful calls with the
    // same limit-insensitive loop identity are enough to trigger detection.
    for _ in 0..2 {
        history.add_record(ToolExecutionRecord::success(
            tools::CODE_SEARCH.to_string(),
            tools::CODE_SEARCH.to_string(),
            false,
            None,
            args.clone(),
            json!({"query": "exec_only_policy", "filters": {}, "results": [], "returned": 0, "truncated": false, "hints": []}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));
    }

    let mut equivalent_args = args.clone();
    equivalent_args["max_results"] = json!(100);
    let loop_result = history.detect_loop(tools::CODE_SEARCH, &equivalent_args);
    assert!(
        loop_result.detected,
        "two loop-equivalent calls should trigger detection despite differing max_results"
    );

    // A third loop-equivalent call increases the repeat count.
    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        args.clone(),
        json!({"query": "exec_only_policy", "filters": {}, "results": [], "returned": 0, "truncated": false, "hints": []}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let loop_result = history.detect_loop(tools::CODE_SEARCH, &args);
    assert!(loop_result.detected);
    assert_eq!(loop_result.repeat_count, 3);
    assert_eq!(loop_result.tool_name, tools::CODE_SEARCH);

    for changed in ["query", "path", "file_types", "result_types"] {
        let mut changed_args = args.clone();
        changed_args[changed] = match changed {
            "query" => json!("different"),
            "path" => json!("vtcode-core/tests"),
            "file_types" => json!(["python"]),
            "result_types" => json!(["text"]),
            _ => unreachable!(),
        };
        assert!(!history.detect_loop(tools::CODE_SEARCH, &changed_args).detected);
    }
}
#[test]
fn code_search_replay_matches_omitted_path_and_default_limit() {
    let history = ToolExecutionHistory::new(10);
    let cached_args = json!({"query": "ToolRegistry"});
    let cached_result = json!({"results": ["cached default search"]});

    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        cached_args,
        cached_result.clone(),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let replayed = history.find_recent_successful_by_read_target(
        tools::CODE_SEARCH,
        &json!({"query": "ToolRegistry", "max_results": 20}),
        Duration::from_secs(60),
    );

    assert_eq!(replayed, Some(cached_result));
}
#[test]
fn code_search_replay_separates_different_effective_limits() {
    let history = ToolExecutionHistory::new(10);

    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        json!({"query": "ToolRegistry", "max_results": 1}),
        json!({"results": ["limited search"]}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    let replayed = history.find_recent_successful_by_read_target(
        tools::CODE_SEARCH,
        &json!({"query": "ToolRegistry", "max_results": 100}),
        Duration::from_secs(60),
    );

    assert!(replayed.is_none());
}
#[test]
fn code_search_replay_stops_after_in_scope_mutation_but_survives_unrelated_edit() {
    let search_args = json!({"query": "Widget", "path": "src"});
    let cached_result = json!({"results": ["cached Widget"]});
    let history_with_mutation = |mutation_path: &str| {
        let history = ToolExecutionHistory::new(10);
        history.add_record(ToolExecutionRecord::success(
            tools::CODE_SEARCH.to_string(),
            tools::CODE_SEARCH.to_string(),
            false,
            None,
            search_args.clone(),
            cached_result.clone(),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));
        history.add_record(ToolExecutionRecord::success(
            tools::APPLY_PATCH.to_string(),
            tools::APPLY_PATCH.to_string(),
            false,
            None,
            json!({"input": format!(
                "*** Begin Patch\n*** Update File: {mutation_path}\n@@\n-Widget\n+Gadget\n*** End Patch\n"
            )}),
            json!({"success": true}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));
        history
    };

    let in_scope = history_with_mutation("src/widget.rs");
    assert!(
        in_scope
            .find_recent_successful_by_read_target(tools::CODE_SEARCH, &search_args, Duration::from_secs(60),)
            .is_none(),
        "searching src, then editing src/widget.rs, must execute fresh"
    );

    let unrelated = history_with_mutation("tests/widget.rs");
    assert_eq!(
        unrelated.find_recent_successful_by_read_target(tools::CODE_SEARCH, &search_args, Duration::from_secs(60),),
        Some(cached_result),
        "an unrelated edit may reuse the prior scoped search"
    );
}
#[test]
fn code_search_replay_stops_after_successful_pathless_command_mutation() {
    let history = ToolExecutionHistory::new(10);
    let search_args = json!({"query": "Widget", "path": "src"});
    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        search_args.clone(),
        json!({"results": ["cached Widget"]}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));
    history.add_record(ToolExecutionRecord::success(
        tools::EXEC_COMMAND.to_string(),
        tools::EXEC_COMMAND.to_string(),
        false,
        None,
        json!({"cmd": "sed -i 's/Widget/Gadget/' src/widget.rs"}),
        json!({"exit_code": 0}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    assert!(
        history
            .find_recent_successful_by_read_target(tools::CODE_SEARCH, &search_args, Duration::from_secs(60),)
            .is_none(),
        "a successful command mutation without explicit target metadata must invalidate search replay"
    );
}
#[test]
fn code_search_replay_stops_after_move_into_searched_scope() {
    let history = ToolExecutionHistory::new(10);
    let search_args = json!({"query": "Widget", "path": "src"});
    history.add_record(ToolExecutionRecord::success(
        tools::CODE_SEARCH.to_string(),
        tools::CODE_SEARCH.to_string(),
        false,
        None,
        search_args.clone(),
        json!({"results": []}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));
    history.add_record(ToolExecutionRecord::success(
        tools::MOVE_FILE.to_string(),
        tools::MOVE_FILE.to_string(),
        false,
        None,
        json!({"path": "staging/widget.rs", "destination": "src/widget.rs"}),
        json!({"success": true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    assert!(
        history
            .find_recent_successful_by_read_target(tools::CODE_SEARCH, &search_args, Duration::from_secs(60),)
            .is_none(),
        "moving a file into the searched scope must invalidate search replay"
    );
}
#[test]
fn code_search_replay_recovers_both_paths_from_base64_public_move_patch() {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    let history = ToolExecutionHistory::new(10);
    let old_search = json!({"query": "Widget", "path": "src/old.rs"});
    let new_search = json!({"query": "Widget", "path": "src/new.rs"});
    for args in [&old_search, &new_search] {
        history.add_record(ToolExecutionRecord::success(
            tools::CODE_SEARCH.to_string(),
            tools::CODE_SEARCH.to_string(),
            false,
            None,
            args.clone(),
            json!({"results": ["cached"]}),
            make_snapshot(),
            None,
            None,
            None,
            None,
            false,
        ));
    }
    let patch =
        "*** Begin Patch\n*** Update File: src/old.rs\n*** Move to: src/new.rs\n@@\n-Widget\n+Gadget\n*** End Patch\n";
    history.add_record(ToolExecutionRecord::success(
        tools::APPLY_PATCH.to_string(),
        tools::APPLY_PATCH.to_string(),
        false,
        None,
        json!({"patch": format!("base64:{}", BASE64.encode(patch))}),
        json!({"success": true}),
        make_snapshot(),
        None,
        None,
        None,
        None,
        false,
    ));

    for args in [&old_search, &new_search] {
        assert!(
            history
                .find_recent_successful_by_read_target(tools::CODE_SEARCH, args, Duration::from_secs(60),)
                .is_none(),
            "both old and new move paths must invalidate replay: {args}"
        );
    }
}
