use super::*;

#[test]
fn preflight_fallback_normalizes_unified_search_args() {
    let error =
        anyhow!("Invalid arguments for tool 'unified_search': \"action\" is a required property");
    let args = json!({
        "Pattern": "LLMStreamEvent::",
        "Path": "."
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_SEARCH, &args, &error)
        .expect("fallback expected for recoverable unified_search preflight");
    assert_eq!(fallback.0, tool_names::UNIFIED_SEARCH);
    assert_eq!(fallback.1["action"], "grep");
    assert_eq!(fallback.1["pattern"], "LLMStreamEvent::");
}

#[test]
fn preflight_fallback_maps_keyword_to_pattern_for_grep() {
    let error = anyhow!("Invalid arguments for tool 'unified_search': missing field `pattern`");
    let args = json!({
        "action": "grep",
        "keyword": "system prompt",
        "path": "src"
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_SEARCH, &args, &error)
        .expect("fallback expected for grep missing pattern");
    assert_eq!(fallback.0, tool_names::UNIFIED_SEARCH);
    assert_eq!(fallback.1["action"], "grep");
    assert_eq!(fallback.1["pattern"], "system prompt");
}

#[test]
fn preflight_fallback_remaps_unified_search_read_action() {
    let error = anyhow!("Tool execution failed: Invalid action: read");
    let args = json!({
        "action": "read",
        "query": "retry",
        "path": "src"
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_SEARCH, &args, &error)
        .expect("fallback expected for invalid read action");
    assert_eq!(fallback.0, tool_names::UNIFIED_SEARCH);
    assert_eq!(fallback.1["action"], "grep");
    assert_eq!(fallback.1["pattern"], "retry");
}

#[test]
fn recovery_fallback_skips_list_degradation_for_search_refinement() {
    let grep = recovery_fallback_for_tool(
        tool_names::UNIFIED_SEARCH,
        &json!({"action":"grep","path":"src","pattern":"Result<"}),
    );
    let structural = recovery_fallback_for_tool(
        tool_names::UNIFIED_SEARCH,
        &json!({"action":"structural","path":"src","pattern":"fn $NAME() {}","lang":"rust"}),
    );

    assert!(grep.is_none());
    assert!(structural.is_none());
}

#[test]
fn recovery_fallback_preserves_list_for_file_discovery_calls() {
    let fallback = recovery_fallback_for_tool(
        tool_names::UNIFIED_SEARCH,
        &json!({"action":"list","path":"src","mode":"tree"}),
    )
    .expect("list fallback expected");

    assert_eq!(fallback.0, tool_names::UNIFIED_SEARCH);
    assert_eq!(fallback.1["action"], "list");
    assert_eq!(fallback.1["path"], "src");
    assert_eq!(fallback.1["mode"], "tree");
}

#[test]
fn preflight_fallback_remaps_unified_file_command_payload_to_unified_exec() {
    let error = anyhow!("Missing action in unified_file");
    let args = json!({
        "command": "git status --short",
        "cwd": "."
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_FILE, &args, &error)
        .expect("fallback expected for unified_file command payload");
    assert_eq!(fallback.0, tool_names::UNIFIED_EXEC);
    assert_eq!(fallback.1["action"], "run");
    assert_eq!(fallback.1["command"], "git status --short");
    assert_eq!(fallback.1["cwd"], ".");
}

#[test]
fn preflight_fallback_remaps_unified_file_list_to_unified_search_list() {
    let error = anyhow!("Invalid arguments for tool 'unified_file': unknown variant `list`");
    let args = json!({
        "action": "list",
        "path": "src"
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_FILE, &args, &error)
        .expect("fallback expected for unified_file list payload");
    assert_eq!(fallback.0, tool_names::UNIFIED_SEARCH);
    assert_eq!(fallback.1["action"], "list");
    assert_eq!(fallback.1["path"], "src");
}

#[test]
fn preflight_fallback_normalizes_request_user_input_single_question_shape() {
    let error = anyhow!(
        "Invalid arguments for tool 'request_user_input': \"questions\" is a required property"
    );
    let args = json!({
        "question": "Which direction should we take?",
        "header": "Scope",
        "options": [
            {"label": "Minimal", "description": "Smallest viable change"},
            {"label": "Full", "description": "Broader implementation"}
        ]
    });
    let fallback = preflight_validation_fallback(tool_names::REQUEST_USER_INPUT, &args, &error)
        .expect("fallback expected for request_user_input shorthand");
    assert_eq!(fallback.0, tool_names::REQUEST_USER_INPUT);
    assert_eq!(fallback.1["questions"][0]["id"], "question_1");
    assert_eq!(fallback.1["questions"][0]["header"], "Scope");
    assert_eq!(
        fallback.1["questions"][0]["question"],
        "Which direction should we take?"
    );
    assert_eq!(
        fallback.1["questions"][0]["options"]
            .as_array()
            .map(|v| v.len()),
        Some(2)
    );
}

#[test]
fn preflight_fallback_normalizes_request_user_input_tabs_shape() {
    let error = anyhow!(
        "Invalid arguments for tool 'request_user_input': additional properties are not allowed"
    );
    let args = json!({
        "question": "Which area should we prioritize first?",
        "tabs": [
            {
                "id": "priority",
                "title": "Priority",
                "items": [
                    {"title": "Reliability", "subtitle": "Reduce failure modes"},
                    {"title": "UX", "subtitle": "Improve user flow"}
                ]
            }
        ]
    });
    let fallback = preflight_validation_fallback(tool_names::REQUEST_USER_INPUT, &args, &error)
        .expect("fallback expected for request_user_input tabbed payload");
    assert_eq!(fallback.0, tool_names::REQUEST_USER_INPUT);
    assert_eq!(fallback.1["questions"][0]["id"], "priority");
    assert_eq!(fallback.1["questions"][0]["header"], "Priority");
    assert_eq!(
        fallback.1["questions"][0]["question"],
        "Which area should we prioritize first?"
    );
    assert_eq!(
        fallback.1["questions"][0]["options"]
            .as_array()
            .map(|v| v.len()),
        Some(2)
    );
}

#[test]
fn validation_error_payload_includes_fallback_metadata() {
    let payload = build_validation_error_content_with_fallback(
        "Tool preflight validation failed: x".to_string(),
        "preflight",
        Some(tool_names::UNIFIED_SEARCH.to_string()),
        Some(json!({"action":"grep","pattern":"foo","path":"."})),
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&payload).expect("validation payload should be json");
    assert_eq!(parsed["error_class"], "invalid_arguments");
    assert_eq!(parsed["is_recoverable"], true);
    assert_eq!(parsed["fallback_tool"], tool_names::UNIFIED_SEARCH);
    assert_eq!(parsed["fallback_tool_args"]["action"], "grep");
    assert_eq!(
        parsed.get("next_action"),
        Some(&json!("Retry with fallback_tool_args."))
    );
    assert!(parsed.get("loop_detected").is_none());
}

#[test]
fn validation_error_payload_marks_loop_detection_without_prose_hint() {
    let payload = build_validation_error_content_with_fallback(
        "Tool 'read_file' is blocked due to excessive repetition (Loop Detected).".to_string(),
        "loop_detection",
        Some(tool_names::UNIFIED_SEARCH.to_string()),
        Some(json!({"action":"list","path":"."})),
    );
    let parsed: serde_json::Value =
        serde_json::from_str(&payload).expect("validation payload should be json");
    assert_eq!(parsed.get("loop_detected"), Some(&json!(true)));
    assert_eq!(parsed["fallback_tool"], tool_names::UNIFIED_SEARCH);
    assert_eq!(parsed["fallback_tool_args"]["action"], "list");
    assert_eq!(
        parsed.get("next_action"),
        Some(&json!("Retry with fallback_tool_args."))
    );
}

#[test]
fn reused_read_only_result_uses_canonical_guidance() {
    let mut payload = json!({
        "output": "preview",
        "content": "preview",
        "stdout": "preview",
        "stderr": "preview",
        "stderr_preview": "preview"
    });

    apply_reused_read_only_loop_metadata(
        payload
            .as_object_mut()
            .expect("payload should be an object for reuse metadata"),
    );

    assert_eq!(payload.get("reused_recent_result"), Some(&json!(true)));
    assert_eq!(payload.get("result_ref_only"), Some(&json!(true)));
    assert_eq!(payload.get("loop_detected"), Some(&json!(true)));
    assert_eq!(
        payload.get("loop_detected_note"),
        Some(&json!(
            "Loop detected on repeated read-only call; reusing recent output. Use unified_search (action='grep') or summarize before another read."
        ))
    );
    assert_eq!(
        payload.get("next_action"),
        Some(&json!(
            "Use unified_search (action='grep') or retry unified_file with a narrower offset/limit before reading again."
        ))
    );
    assert!(payload.get("output").is_none());
    assert!(payload.get("content").is_none());
    assert!(payload.get("stdout").is_none());
    assert!(payload.get("stderr").is_none());
    assert!(payload.get("stderr_preview").is_none());
}