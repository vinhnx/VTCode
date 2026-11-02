use super::*;

#[test]
fn working_history_code_search_replay_stops_at_in_scope_mutation() {
    let search_args = json!({"query": "Widget", "path": "src"});
    let search_call = uni::Message::assistant_with_tools(
        "search".into(),
        vec![uni::ToolCall::function(
            "search_call".into(),
            tools::CODE_SEARCH.into(),
            serde_json::to_string(&search_args).unwrap(),
        )],
    );
    let search_result = uni::Message {
        role: uni::MessageRole::Tool,
        content: uni::MessageContent::text("{\"results\":[\"cached\"]}".into()),
        tool_call_id: Some("search_call".into()),
        ..Default::default()
    };
    let mutation = |path: &str, result: serde_json::Value| {
        let patch = format!("*** Begin Patch\n*** Update File: {path}\n@@\n-Widget\n+Gadget\n*** End Patch\n");
        vec![
            uni::Message::assistant_with_tools(
                "edit".into(),
                vec![uni::ToolCall::function(
                    "edit_call".into(),
                    tools::APPLY_PATCH.into(),
                    serde_json::to_string(&json!({"patch": patch})).unwrap(),
                )],
            ),
            uni::Message::tool_response("edit_call".into(), result.to_string()),
        ]
    };

    let mut in_scope_history = vec![search_call.clone(), search_result.clone()];
    in_scope_history.extend(mutation("src/widget.rs", json!({"success": true})));
    assert!(
        find_duplicate_in_history(&in_scope_history, tools::CODE_SEARCH, &search_args, Path::new("."),).is_none(),
        "editing src/widget.rs after searching src must force a fresh search"
    );

    let mut status_success_history = vec![search_call.clone(), search_result.clone()];
    status_success_history.extend(mutation("src/widget.rs", json!({"status": "success", "output": "patch applied"})));
    assert!(
        find_duplicate_in_history(&status_success_history, tools::CODE_SEARCH, &search_args, Path::new("."),).is_none(),
        "the established successful status shape must invalidate replay"
    );

    let mut unrelated_history = vec![search_call.clone(), search_result.clone()];
    unrelated_history.extend(mutation("tests/widget.rs", json!({"success": true})));
    assert_eq!(
        find_duplicate_in_history(&unrelated_history, tools::CODE_SEARCH, &search_args, Path::new("."),).as_deref(),
        Some("{\"results\":[\"cached\"]}"),
        "an unrelated edit may reuse the prior scoped search"
    );

    for failure in [
        json!({"success": false, "error": "patch rejected"}),
        json!({"error": {"message": "execution denied by policy"}}),
        json!({"failure_kind": "timeout"}),
        json!({"status": "failed"}),
        json!({"status": "denied"}),
        json!({"success": null}),
        json!({"output": "patch output without an outcome"}),
        json!(["non-object mutation output"]),
    ] {
        let mut failed_history = vec![search_call.clone(), search_result.clone()];
        failed_history.extend(mutation("src/widget.rs", failure));
        assert_eq!(
            find_duplicate_in_history(&failed_history, tools::CODE_SEARCH, &search_args, Path::new("."),).as_deref(),
            Some("{\"results\":[\"cached\"]}"),
            "a mutation without explicit positive success evidence must preserve reuse"
        );
    }

    let mut unexecuted_history = vec![search_call, search_result];
    let unexecuted_mutation = mutation("src/widget.rs", json!({"success": true}));
    unexecuted_history.push(unexecuted_mutation[0].clone());
    assert_eq!(
        find_duplicate_in_history(&unexecuted_history, tools::CODE_SEARCH, &search_args, Path::new("."),).as_deref(),
        Some("{\"results\":[\"cached\"]}"),
        "an unexecuted mutation call must preserve reuse"
    );
}
#[test]
fn mutation_tool_response_success_rejects_malformed_and_conflicting_shapes() {
    let response = |content: &str| uni::Message::tool_response("edit_call".into(), content.into());

    assert!(tool_response_is_success(&response(r#"{"success":true}"#)));
    assert!(tool_response_is_success(&response(r#"{"status":"success","output":"patch applied"}"#,)));

    for content in [
        "not json",
        "null",
        r#"{"success":null,"status":"success"}"#,
        r#"{"success":true,"status":"failed"}"#,
        r#"{"success":true,"failure_kind":"timeout"}"#,
        r#"{"success":true,"error":"execution denied"}"#,
    ] {
        assert!(
            !tool_response_is_success(&response(content)),
            "mutation outcome must not count as successful: {content}"
        );
    }
}
#[test]
fn duplicate_history_reuse_rejects_failed_results() {
    let args = json!({"query": "needle", "path": "src"});
    let call = || {
        uni::Message::assistant_with_tools(
            "search".into(),
            vec![uni::ToolCall::function(
                "search_call".into(),
                tools::CODE_SEARCH.into(),
                serde_json::to_string(&args).unwrap(),
            )],
        )
    };

    for failure in [
        r#"{"success":false,"output":"partial"}"#,
        r#"{"status":"timeout","output":"partial"}"#,
        r#"{"error":"permission denied"}"#,
        "Error: command failed",
        "timed out while reading",
        "failed to execute command",
        "denied by policy",
        "blocked until verification",
        "not executed",
    ] {
        let history = vec![
            call(),
            uni::Message::tool_response("search_call".into(), failure.into()),
        ];
        assert!(
            find_duplicate_in_history(&history, tools::CODE_SEARCH, &args, Path::new(".")).is_none(),
            "failed result must not be replayed: {failure}"
        );
    }

    for success in [r#"{"results":[]}"#, "[]", "plain successful output"] {
        let history = vec![
            call(),
            uni::Message::tool_response("search_call".into(), success.into()),
        ];
        assert_eq!(
            find_duplicate_in_history(&history, tools::CODE_SEARCH, &args, Path::new(".")).as_deref(),
            Some(success)
        );
    }
}
#[test]
fn working_history_code_search_replay_rejects_reused_patch_call_id() {
    let search_args = json!({"query": "Widget", "path": "src"});
    let shared_call_id = "call_0";
    let search_call = uni::Message::assistant_with_tools(
        "search".into(),
        vec![uni::ToolCall::function(
            shared_call_id.into(),
            tools::CODE_SEARCH.into(),
            serde_json::to_string(&search_args).unwrap(),
        )],
    );
    let search_result =
        uni::Message::tool_response(shared_call_id.into(), "{\"results\":[\"genuine search output\"]}".into());
    let patch = "*** Begin Patch\n*** Update File: src/widget.rs\n@@\n-Widget\n+Gadget\n*** End Patch\n";
    let patch_call = uni::Message::assistant_with_tools(
        "edit".into(),
        vec![uni::ToolCall::function(
            shared_call_id.into(),
            tools::APPLY_PATCH.into(),
            serde_json::to_string(&json!({"patch": patch})).unwrap(),
        )],
    );

    let mut successful_history = vec![
        search_call.clone(),
        search_result.clone(),
        patch_call.clone(),
        uni::Message::tool_response(
            shared_call_id.into(),
            json!({"success": true, "output": "patch output"}).to_string(),
        ),
    ];
    assert!(
        find_duplicate_in_history(&successful_history, tools::CODE_SEARCH, &search_args, Path::new("."),).is_none(),
        "a successful in-scope patch must invalidate the genuine earlier search result"
    );

    successful_history.pop();
    successful_history.push(uni::Message::tool_response(
        shared_call_id.into(),
        json!({"success": false, "error": "patch rejected", "output": "patch output"}).to_string(),
    ));
    assert_eq!(
        find_duplicate_in_history(&successful_history, tools::CODE_SEARCH, &search_args, Path::new("."),).as_deref(),
        Some("{\"results\":[\"genuine search output\"]}"),
        "a failed patch must preserve the earlier search without returning patch output"
    );
}
#[test]
fn read_extent_covers_query_rejects_larger_limit() {
    // Cached limit=200 must NOT cover query limit=220
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":220}),
    ));

    // Cached limit=200 covers query limit=200 (same)
    assert!(read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
    ));

    // Cached limit=200 covers query limit=100 (subset)
    assert!(read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":100}),
    ));

    // Different offset must not match
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        &json!({"action":"read","path":"AGENTS.md","offset":50,"limit":200}),
    ));
}
#[test]
fn read_extent_covers_query_rejects_different_raw_mode() {
    // Non-raw cached must NOT cover raw=true query
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
    ));

    // Raw cached covers raw query
    assert!(read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
    ));

    // Raw cached must NOT cover non-raw query
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200,"raw":true}),
        &json!({"action":"read","path":"AGENTS.md","offset":0,"limit":200}),
    ));
}
#[test]
fn read_extent_covers_query_handles_missing_limit() {
    // Both missing limit → matches (same default read)
    assert!(read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md"}),
        &json!({"action":"read","path":"AGENTS.md"}),
    ));

    // Cached has limit, query doesn't → mismatch
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","limit":200}),
        &json!({"action":"read","path":"AGENTS.md"}),
    ));

    // Cached has no limit, query does → mismatch
    assert!(!read_extent::extent_covers(
        &json!({"action":"read","path":"AGENTS.md","limit":200}),
        &json!({"action":"read","path":"AGENTS.md"}),
    ));
}
