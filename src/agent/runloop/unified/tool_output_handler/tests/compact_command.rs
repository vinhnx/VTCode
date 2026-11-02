use super::*;

#[tokio::test]
async fn compact_command_capture_keeps_follow_up_guidance() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let output = serde_json::json!({
        "stdout": "command output",
        "next_action": "Review the result before continuing."
    });

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf guidance"}),
        &output,
        true,
        None,
        None,
    )
    .await
    .expect("guidance-bearing command output should render");

    let capture = std::iter::from_fn(|| receiver.try_recv().ok()).find_map(|command| match command {
        InlineCommand::RecordToolOutput { lines, .. } => Some(lines),
        _ => None,
    });
    let capture = capture.expect("complete command capture should be retained");
    assert!(capture.iter().any(|line| line.contains("Review the result before continuing.")));
}
#[tokio::test]
async fn compact_command_capture_keeps_structured_result_metadata() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let output = serde_json::json!({
        "stdout": "command output",
        "generated_files": {
            "count": 1,
            "files": ["src/generated.rs"],
            "summary": "Generated one file"
        },
        "metadata_flag": false,
        "metadata_count": 0,
        "fallback_tool": tools::CODE_SEARCH,
        "fallback_tool_args": {"query": "generated"},
        "stderr_preview": "no stderr was emitted"
    });

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "generate"}),
        &output,
        true,
        None,
        None,
    )
    .await
    .expect("structured command output should render");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    let capture = commands.iter().find_map(|command| match command {
        InlineCommand::RecordToolOutput { lines, .. } => Some(lines.join("\n")),
        _ => None,
    });
    let capture = capture.expect("complete command capture should be retained");
    assert!(capture.contains("structured output"));
    assert!(capture.contains("generated_files"));
    assert!(capture.contains("src/generated.rs"));
    assert!(capture.contains("metadata_flag"));
    assert!(capture.contains("metadata_count"));
    assert!(capture.contains("fallback_tool"));
    assert!(capture.contains("fallback_tool_args"));
    assert!(capture.contains("stderr_preview"));

    let visible_text = commands
        .iter()
        .filter_map(|command| match command {
            InlineCommand::AppendLine { segments, .. } => {
                Some(segments.iter().map(|segment| segment.text.as_str()).collect::<String>())
            }
            InlineCommand::Inline { segment, .. } => Some(segment.text.clone()),
            InlineCommand::Diagnostic(diagnostic) => Some(diagnostic.summary.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(visible_text.contains("generated_files"));
    assert!(visible_text.contains("src/generated.rs"));
}
#[tokio::test]
async fn expanded_command_summary_carries_capture_identity() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    renderer.set_tool_display_mode(vtcode_core::config::ToolDisplayMode::Expanded);

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf identity"}),
        &serde_json::json!({"stdout": "captured output"}),
        true,
        None,
        None,
    )
    .await
    .expect("expanded command output should render");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    let capture_id = commands.iter().find_map(|command| match command {
        InlineCommand::RecordToolOutput { id, .. } => Some(*id),
        _ => None,
    });
    let summary_id = commands.iter().find_map(|command| match command {
        InlineCommand::AppendToolOutputLine { id, .. } => Some(*id),
        _ => None,
    });

    assert_eq!(summary_id, capture_id);
}
#[tokio::test]
async fn compact_warning_remains_visible_and_flushes_command_group() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    renderer
        .render_compact_command_activity("printf first", 0, None, None)
        .expect("seed compact command should render");
    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf warning"}),
        &serde_json::json!({"warning": "no results"}),
        true,
        None,
        None,
    )
    .await
    .expect("warning command should render");
    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf second"}),
        &serde_json::json!({"stdout": "second output"}),
        true,
        None,
        None,
    )
    .await
    .expect("following command should render");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    let activities = commands
        .iter()
        .filter_map(|command| match command {
            InlineCommand::AppendCompactActivity(activity) | InlineCommand::ReplaceCompactActivity(activity) => {
                Some(activity)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let visible_text = commands
        .iter()
        .filter_map(|command| match command {
            InlineCommand::AppendLine { segments, .. } => {
                Some(segments.iter().map(|segment| segment.text.as_str()).collect::<String>())
            }
            InlineCommand::Inline { segment, .. } => Some(segment.text.clone()),
            InlineCommand::Diagnostic(diagnostic) => Some(diagnostic.summary.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(activities.len(), 2);
    assert!(activities.iter().all(|activity| activity.command_count == 1));
    assert!(visible_text.contains("no results"));
}
#[tokio::test]
async fn compact_warning_flushes_non_pty_command_alias_group() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    renderer
        .render_compact_command_activity("printf first", 0, None, None)
        .expect("seed compact command should render");
    render_tool_output_common(
        &mut renderer,
        &handle,
        "bash",
        &serde_json::json!({"command": "printf warning"}),
        &serde_json::json!({"warning": "no results"}),
        true,
        None,
        None,
    )
    .await
    .expect("non-PTY warning command should render");
    render_tool_output_common(
        &mut renderer,
        &handle,
        "bash",
        &serde_json::json!({"command": "printf second"}),
        &serde_json::json!({"stdout": "second output"}),
        true,
        None,
        None,
    )
    .await
    .expect("following command should render");

    let activities = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendCompactActivity(activity) | InlineCommand::ReplaceCompactActivity(activity) => {
                Some(activity)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(activities.len(), 2);
    assert!(activities.iter().all(|activity| activity.command_count == 1));
}
