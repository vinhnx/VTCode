use super::*;

#[tokio::test]
async fn unavailable_spool_capture_remains_visible_and_does_not_collapse_pty() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "printf preview"}),
        &serde_json::json!({"spool_path": null, "output": "preview output"}),
        true,
        None,
        None,
    )
    .await
    .expect("unavailable spool result should render");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    assert!(commands.iter().any(|command| {
        matches!(command, InlineCommand::Diagnostic(diagnostic)
            if diagnostic.summary.contains("capture unavailable"))
    }));
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, InlineCommand::CollapsePtyBlock(_)))
    );
}
#[tokio::test]
async fn compact_pty_completion_emits_grouped_activity_without_live_preview() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "printf first"}),
        &serde_json::json!({"stdout": "first\nsecond"}),
        true,
        None,
        None,
    )
    .await
    .expect("compact PTY output should render");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, InlineCommand::CollapsePtyBlock(_)))
    );
    assert!(
        !commands.iter().any(|command| {
            matches!(
                command,
                InlineCommand::AppendLine { .. } | InlineCommand::Inline { .. } | InlineCommand::ReplaceLast { .. }
            )
        }),
        "compact PTY completion must not flash a live output block"
    );
}
#[tokio::test]
async fn compact_pty_attention_keeps_command_summary_and_stderr_visible() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "printf diagnostic"}),
        &serde_json::json!({"stdout": "normal output", "stderr": "diagnostic output"}),
        true,
        None,
        None,
    )
    .await
    .expect("compact PTY diagnostics should render");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    assert!(commands.iter().any(|command| {
        matches!(command, InlineCommand::AppendToolOutputLine { segments, .. }
            if segments.iter().map(|segment| segment.text.as_str()).collect::<String>().contains("• Ran printf diagnostic"))
    }));
    assert!(commands.iter().any(|command| {
        matches!(command, InlineCommand::Diagnostic(diagnostic)
            if diagnostic.summary.contains("stderr: diagnostic output"))
    }));
}
#[test]
fn compact_pty_failure_keeps_command_summary_without_live_preview() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let mut stats = SessionStats::default();
    let mut mcp = McpPanelState::default();
    let mut output_ctx = OutcomeContext {
        workspace_root: None,
        session_stats: &mut stats,
        renderer: &mut renderer,
        handle: &handle,
        mcp_panel_state: &mut mcp,
        vt_config: None::<&VTCodeConfig>,
    };
    let status = ToolExecutionStatus::Failure {
        error: ToolExecutionError::from_anyhow(
            tools::RUN_PTY_CMD,
            &anyhow::anyhow!("command failed"),
            0,
            false,
            false,
            Some("test"),
        ),
    };

    handle_non_success_common(&mut output_ctx, tools::RUN_PTY_CMD, &serde_json::json!({"command": "false"}), &status)
        .expect("compact PTY failure should render");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    assert!(commands.iter().any(|command| {
        matches!(command, InlineCommand::AppendToolOutputLine { segments, .. }
            if segments.iter().map(|segment| segment.text.as_str()).collect::<String>().contains("• Ran false"))
    }));
}
#[tokio::test]
async fn compact_command_output_emits_group_metadata_and_complete_capture() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let output = serde_json::json!({"stdout": "first\nsecond"});

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf first"}),
        &output,
        true,
        None,
        None,
    )
    .await
    .expect("compact command output should render");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    let capture_id = commands.iter().find_map(|command| match command {
        InlineCommand::RecordToolOutput { id, .. } => Some(*id),
        _ => None,
    });
    let activity = commands.iter().find_map(|command| match command {
        InlineCommand::AppendCompactActivity(activity) => Some(activity),
        _ => None,
    });

    let capture_id = capture_id.expect("complete command capture should be retained");
    let activity = activity.expect("compact command activity should be emitted");
    assert_eq!(activity.review_anchor, Some(capture_id));
    assert_eq!(activity.hidden_line_count, 2);
    assert_eq!(activity.display_text(), "• Ran printf first · … +2 lines");
    assert!(
        commands
            .iter()
            .all(|command| { !matches!(command, InlineCommand::AppendLine { .. } | InlineCommand::Inline { .. }) })
    );
}
