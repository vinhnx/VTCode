use super::*;

#[test]
fn suppresses_redundant_diff_recap_after_git_diff_view_request() {
    let history = vec![
        uni::Message::user("show diff src/main.rs".to_string()),
        uni::Message::tool_response(
            "call_1".to_string(),
            r#"{"content_type":"git_diff","command":"git diff -- src/main.rs","output":"diff --git a/src/main.rs b/src/main.rs"}"#.to_string(),
        ),
    ];

    assert!(should_suppress_redundant_diff_recap(
        &history,
        "Diff for src/main.rs:\n```diff\n@@ -1 +1 @@\n```"
    ));
}

#[test]
fn does_not_suppress_diff_recap_when_user_asked_for_analysis() {
    let history = vec![
        uni::Message::user("analyze this diff and explain".to_string()),
        uni::Message::tool_response(
            "call_1".to_string(),
            r#"{"content_type":"git_diff","command":"git diff -- src/main.rs"}"#.to_string(),
        ),
    ];

    assert!(!should_suppress_redundant_diff_recap(
        &history,
        "The diff shows one behavior change."
    ));
}

#[test]
fn suppresses_heading_style_diff_recap_after_view_request() {
    let history = vec![
        uni::Message::user("show diff on vtcode-tui/src/ui/markdown.rs".to_string()),
        uni::Message::tool_response(
            "call_1".to_string(),
            r#"{"content_type":"git_diff","command":"git diff -- vtcode-tui/src/ui/markdown.rs","output":"diff --git a/vtcode-tui/src/ui/markdown.rs b/vtcode-tui/src/ui/markdown.rs\n@@ -1 +1 @@\n- old\n+ new"}"#.to_string(),
        ),
    ];

    assert!(should_suppress_redundant_diff_recap(
        &history,
        "Implemented updated syntax highlighting for diff previews.\n\n**Diff preview changes**\n\n```\n@@\n- old\n+ new\n```\n"
    ));
}

#[test]
fn parse_reasoning_detail_value_decodes_stringified_json_object() {
    let parsed =
        parse_reasoning_detail_value(r#"{"type":"reasoning.text","id":"r1","text":"hello"}"#);
    assert!(parsed.is_object());
    assert_eq!(parsed["type"], "reasoning.text");
}

#[test]
fn build_combined_reasoning_falls_back_to_detail_text() {
    let combined = build_combined_reasoning(&[], Some("detail trace"));
    assert_eq!(combined.as_deref(), Some("detail trace"));
}

#[test]
fn build_combined_reasoning_preserves_whitespace_only_segments_without_detail() {
    let combined = build_combined_reasoning(&[ReasoningSegment::new("  ", None)], None);
    assert_eq!(combined.as_deref(), Some("  "));
}

#[test]
fn push_assistant_message_preserves_reasoning_details_when_merging() {
    let mut history = vec![uni::Message::assistant("old".to_string())];
    let new_msg = uni::Message::assistant("new".to_string()).with_reasoning_details(Some(vec![
        serde_json::json!({"type":"reasoning.text","text":"trace"}),
    ]));

    push_assistant_message(&mut history, new_msg);

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content.as_text(), "new");
    assert_eq!(
        history[0].reasoning_details,
        Some(vec![
            serde_json::json!({"type":"reasoning.text","text":"trace"})
        ])
    );
}

#[test]
fn push_assistant_message_keeps_different_phases_separate() {
    let mut history = vec![
        uni::Message::assistant("working".to_string())
            .with_phase(Some(uni::AssistantPhase::Commentary)),
    ];
    let new_msg = uni::Message::assistant("done".to_string())
        .with_phase(Some(uni::AssistantPhase::FinalAnswer));

    push_assistant_message(&mut history, new_msg);

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].phase, Some(uni::AssistantPhase::Commentary));
    assert_eq!(history[1].phase, Some(uni::AssistantPhase::FinalAnswer));
}