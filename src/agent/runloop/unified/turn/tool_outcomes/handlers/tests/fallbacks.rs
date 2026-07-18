#![allow(missing_docs)]
use super::*;

#[test]
fn recovery_fallback_for_code_search_uses_query_and_path() {
    let fallback = recovery_fallback_for_tool(
        tool_names::CODE_SEARCH,
        &json!({
            "query": "Widget",
            "path": "src",
            "file_types": ["rust"],
            "result_types": ["definition", "usage"],
            "max_results": 10
        }),
    )
    .expect("code_search should fall back to literal command search");

    assert_eq!(fallback.0, tool_names::EXEC_COMMAND);
    assert!(
        fallback.1["cmd"]
            .as_str()
            .is_some_and(|cmd| cmd.contains("'Widget'") && cmd.contains("'src'"))
    );
}

#[test]
fn preflight_fallback_remaps_file_operation_command_payload_to_command_session() {
    let error = anyhow!("Missing action in file_operation");
    let args = json!({
        "command": "git status --short",
        "cwd": "."
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_FILE, &args, &error)
        .expect("fallback expected for file_operation command payload");
    assert_eq!(fallback.0, tool_names::EXEC_COMMAND);
    assert_eq!(fallback.1["cmd"], "git status --short");
    assert_eq!(fallback.1["workdir"], ".");
}

#[test]
fn preflight_fallback_remaps_file_operation_list_to_exec_command() {
    let error = anyhow!("Invalid arguments for tool 'file_operation': unknown variant `list`");
    let args = json!({
        "action": "list",
        "path": "src"
    });
    let fallback = preflight_validation_fallback(tool_names::UNIFIED_FILE, &args, &error)
        .expect("fallback expected for file_operation list payload");
    assert_eq!(fallback.0, tool_names::EXEC_COMMAND);
    assert!(fallback.1["cmd"].as_str().is_some_and(|cmd| cmd.contains("'src'")));
}

#[test]
fn preflight_fallback_normalizes_request_user_input_single_question_shape() {
    let error = anyhow!("Invalid arguments for tool 'request_user_input': \"questions\" is a required property");
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
    assert_eq!(fallback.1["questions"][0]["question"], "Which direction should we take?");
    assert_eq!(fallback.1["questions"][0]["options"].as_array().map(|v| v.len()), Some(2));
}

#[test]
fn preflight_fallback_normalizes_request_user_input_tabs_shape() {
    let error = anyhow!("Invalid arguments for tool 'request_user_input': additional properties are not allowed");
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
    assert_eq!(fallback.1["questions"][0]["question"], "Which area should we prioritize first?");
    assert_eq!(fallback.1["questions"][0]["options"].as_array().map(|v| v.len()), Some(2));
}

#[test]
fn validation_error_payload_includes_fallback_metadata() {
    let payload = build_validation_error_content_with_fallback(
        "Tool preflight validation failed: x".to_string(),
        "preflight",
        Some(tool_names::EXEC_COMMAND.to_string()),
        Some(json!({"cmd":"rg --line-number --column --color=never 'foo' '.'"})),
    );
    let parsed: serde_json::Value = serde_json::from_str(&payload).expect("validation payload should be json");
    assert_eq!(parsed["error_class"], "invalid_arguments");
    assert_eq!(parsed["is_recoverable"], true);
    assert_eq!(parsed["fallback_tool"], tool_names::EXEC_COMMAND);
    assert_eq!(parsed["fallback_tool_args"]["cmd"], "rg --line-number --column --color=never 'foo' '.'");
    assert_eq!(parsed.get("next_action"), Some(&json!("Retry with fallback_tool_args.")));
    assert!(parsed.get("loop_detected").is_none());
}

#[test]
fn validation_error_payload_marks_loop_detection_without_prose_hint() {
    let payload = build_validation_error_content_with_fallback(
        "Tool 'read_file' is blocked due to excessive repetition (Loop Detected).".to_string(),
        "loop_detection",
        Some(tool_names::EXEC_COMMAND.to_string()),
        Some(json!({"cmd":"find '.' -maxdepth 1 -mindepth 1 -print"})),
    );
    let parsed: serde_json::Value = serde_json::from_str(&payload).expect("validation payload should be json");
    // `loop_detected` is internal control logic and is stripped from model output.
    assert!(parsed.get("loop_detected").is_none());
    assert_eq!(parsed["fallback_tool"], tool_names::EXEC_COMMAND);
    assert_eq!(parsed["fallback_tool_args"]["cmd"], "find '.' -maxdepth 1 -mindepth 1 -print");
    assert_eq!(parsed.get("next_action"), Some(&json!("Retry with fallback_tool_args.")));
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
        payload.as_object_mut().expect("payload should be an object for reuse metadata"),
    );

    assert_eq!(payload.get("reused_recent_result"), Some(&json!(true)));
    assert_eq!(payload.get("result_ref_only"), Some(&json!(true)));
    assert_eq!(payload.get("loop_detected"), Some(&json!(true)));
    assert_eq!(
        payload.get("loop_detected_note"),
        Some(&json!("Loop detected: same result returned. The content is in the result above — use it directly."))
    );
    assert_eq!(
        payload.get("next_action"),
        Some(&json!(
            "The tool result content is already in this response. Synthesize your answer from the available data."
        ))
    );
    assert_eq!(payload.get("output"), Some(&json!("preview")));
    assert_eq!(payload.get("content"), Some(&json!("preview")));
    assert_eq!(payload.get("stdout"), Some(&json!("preview")));
    assert_eq!(payload.get("stderr"), Some(&json!("preview")));
    assert_eq!(payload.get("stderr_preview"), Some(&json!("preview")));
}
