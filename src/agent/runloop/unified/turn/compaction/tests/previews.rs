use super::*;

#[test]
fn recovery_context_previews_include_latest_user_request_and_recent_distinct_tool_outputs() {
    let history = vec![
        Message::user("first request".to_string()),
        Message::tool_response("call-1".to_string(), "duplicate output".to_string()),
        Message::tool_response("call-2".to_string(), "distinct output".to_string()),
        Message::tool_response("call-3".to_string(), "duplicate output".to_string()),
        Message::user("latest request".to_string()),
    ];

    let previews = build_recovery_context_previews_with_workspace(&history, None);

    assert_eq!(previews[0], "Latest user request: latest request");
    assert_eq!(previews[1], "Tool output 1: duplicate output");
    assert_eq!(previews[2], "Tool output 2: distinct output");
    assert_eq!(previews.len(), 3);
}

#[test]
fn recovery_context_previews_fall_back_to_latest_assistant_text_when_needed() {
    let history = vec![Message::assistant("assistant summary".to_string())];

    let previews = build_recovery_context_previews_with_workspace(&history, None);

    assert_eq!(previews, vec!["Latest assistant text: assistant summary"]);
}

#[test]
fn recovery_context_previews_extract_structured_tool_guidance() {
    let history = vec![
        Message::user("search for Widget".to_string()),
        Message::tool_response(
            "call-1".to_string(),
            json!({
                "results": [],
                "path": "src/agent",
                "is_recoverable": true,
                "hint": "Try narrowing the path.",
                "next_action": "Retry with narrower filters.",
                "fallback_tool": "code_search",
                "fallback_tool_args": {"query": "Widget", "path": "src/agent"}
            })
            .to_string(),
        ),
    ];

    let previews = build_recovery_context_previews_with_workspace(&history, None);

    assert_eq!(previews[0], "Latest user request: search for Widget");
    assert!(previews[1].contains("No matches found in src/agent"));
    assert!(previews[1].contains("Try narrowing the path."));
    assert!(previews[1].contains("Next action: Retry with narrower filters."));
    assert!(previews[1].contains("Fallback tool: code_search"));
}

#[test]
fn recovery_context_previews_extract_nested_error_guidance_and_spool_excerpt() {
    let temp = tempdir().expect("tempdir");

    let history = vec![
        Message::user("review the read failure".to_string()),
        Message::tool_response(
            "call-1".to_string(),
            json!({
                "path": "src/main.rs",
                "spool_path": ".vtcode/context/tool_outputs/read_1.txt",
                "preview": "spooled-line-1\nspooled-line-2\nSpool excerpt: nested diagnostic",
                "error": {
                    "message": "Read failed",
                    "hint": "Inspect the spooled content.",
                    "next_action": "Retry with a smaller slice."
                }
            })
            .to_string(),
        ),
    ];

    let previews = build_recovery_context_previews_with_workspace(&history, Some(temp.path()));

    assert_eq!(previews[0], "Latest user request: review the read failure");
    assert!(previews[1].contains("Read failed"));
    assert!(previews[1].contains("Inspect the spooled content."));
    assert!(previews[1].contains("Next action: Retry with a smaller slice."));
    assert!(previews[1].contains("source_path: src/main.rs"));
    assert!(previews[1].contains("Spool excerpt:"));
    assert!(previews[1].contains("spooled-line-1"));
    assert_eq!(previews[1].matches("Spool excerpt:").count(), 1);
    assert!(previews[1].contains("Output excerpt: nested diagnostic"));
}

#[test]
fn recovery_context_previews_prefer_substantive_reads_over_recent_low_signal_outputs() {
    let history = vec![
        Message::user("tell me more".to_string()),
        Message::tool_response(
            "call-1".to_string(),
            json!({
                "path": "README.md",
                "content": "VT Code is an open-source coding agent with LLM-native code understanding."
            })
            .to_string(),
        ),
        Message::tool_response(
            "call-2".to_string(),
            json!({
                "path": "docs/ARCHITECTURE.md",
                "content": "VT Code follows a modular architecture designed for maintainability and extensibility."
            })
            .to_string(),
        ),
        Message::tool_response(
            "call-3".to_string(),
            json!({
                "count": 20,
                "items": [{"path": "docs/ide"}]
            })
            .to_string(),
        ),
        Message::tool_response(
            "call-4".to_string(),
            json!({
                "error": "Repeated reads of 'docs/ARCHITECTURE.md' with limited progress detected.",
                "next_action": "Try an alternative tool or narrower scope."
            })
            .to_string(),
        ),
    ];

    let previews = build_recovery_context_previews_with_workspace(&history, None);

    assert_eq!(previews[0], "Latest user request: tell me more");
    assert!(previews[1].contains("VT Code follows a modular architecture"));
    assert!(previews[2].contains("VT Code is an open-source coding agent"));
    assert!(previews[3].contains("Repeated reads of 'docs/ARCHITECTURE.md'"));
    assert!(
        previews.iter().all(|preview| !preview.contains("Listed 20 items")),
        "low-signal listing should be dropped when richer previews exist: {previews:?}"
    );
}
