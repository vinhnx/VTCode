use super::*;

#[tokio::test]
async fn compact_file_diff_is_a_glance_boundary_between_command_groups() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf first"}),
        &serde_json::json!({"stdout": "first output"}),
        true,
        None,
        None,
    )
    .await
    .expect("first command should render");

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EDIT_FILE,
        &serde_json::json!({"path": "src/lib.rs"}),
        &serde_json::json!({
            "success": true,
            "diff": [{
                "path": "src/lib.rs",
                "operation": "updated",
                "content": "@@ -1 +1 @@\n-before\n+after\n",
                "additions": 1,
                "deletions": 1,
                "truncated": false,
                "skipped": false
            }]
        }),
        true,
        None,
        None,
    )
    .await
    .expect("file diff should render");

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
    .expect("second command should render");

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
    assert!(
        !commands
            .iter()
            .any(|command| matches!(command, InlineCommand::ReplaceCompactActivity(_)))
    );
    assert!(!visible_text.contains("Edit file"));
    assert!(visible_text.contains("• Edited src/lib.rs (+1 -1)"));
    assert!(visible_text.contains("-    1 │ before"));
    assert!(visible_text.contains("+    1 │ after"));
}
#[tokio::test]
async fn compact_command_artifacts_start_a_fresh_group() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    renderer
        .render_compact_command_activity("printf first", 1, None, None)
        .expect("seed compact command should render");
    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf second"}),
        &serde_json::json!({"stdout": "normal output", "stderr": "diagnostic output"}),
        true,
        None,
        None,
    )
    .await
    .expect("artifact-bearing command should render");

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
#[tokio::test]
async fn compact_pty_artifacts_flush_a_preceding_command_group() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    renderer
        .render_compact_command_activity("printf first", 0, None, None)
        .expect("seed compact command should render");
    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "printf diagnostic"}),
        &serde_json::json!({"output": "normal output", "stderr": "diagnostic output"}),
        true,
        None,
        None,
    )
    .await
    .expect("artifact-bearing PTY command should render");
    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf second"}),
        &serde_json::json!({"stdout": "normal output"}),
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
#[tokio::test]
async fn compact_command_output_keeps_distinct_stderr_visible() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let output = serde_json::json!({
        "stdout": "normal output",
        "stderr": "diagnostic output"
    });

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf test"}),
        &output,
        true,
        None,
        None,
    )
    .await
    .expect("stderr-bearing command output should render");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
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

    assert!(
        commands
            .iter()
            .any(|command| matches!(command, InlineCommand::AppendCompactActivity(_)))
    );
    assert!(visible_text.contains("diagnostic output"));
    assert!(!visible_text.contains("normal output"));
}
#[tokio::test]
async fn compact_command_output_preserves_identical_stdout_and_stderr() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let output = serde_json::json!({
        "stdout": "same output",
        "stderr": "same output"
    });

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::EXECUTE_CODE,
        &serde_json::json!({"command": "printf test"}),
        &output,
        true,
        None,
        None,
    )
    .await
    .expect("identical named streams should remain visible");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
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
    assert!(visible_text.contains("same output"));
    let capture_text = commands
        .iter()
        .find_map(|command| match command {
            InlineCommand::RecordToolOutput { lines, .. } => Some(lines.join("\n")),
            _ => None,
        })
        .expect("complete command capture should be retained");
    assert!(capture_text.contains("stdout"));
    assert!(capture_text.contains("stderr"));
    assert_eq!(capture_text.matches("same output").count(), 2);
}
#[tokio::test]
async fn pty_input_forwarding_does_not_collapse_as_a_command() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::SEND_PTY_INPUT,
        &serde_json::json!({"session_id": "pty-1", "chars": ""}),
        &serde_json::json!({"output": "polled output"}),
        true,
        None,
        None,
    )
    .await
    .expect("PTY input forwarding should render");

    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    assert!(!commands.iter().any(|command| {
        matches!(command, InlineCommand::AppendCompactActivity(_) | InlineCommand::CollapsePtyBlock(_))
    }));
    assert!(!commands.iter().any(|command| {
        matches!(command, InlineCommand::AppendLine { segments, .. }
            if segments.iter().any(|segment| segment.text.contains("• Ran send_pty_input")))
    }));
}
#[tokio::test]
async fn compact_pty_output_keeps_distinct_stderr_visible() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let output = serde_json::json!({
        "output": "terminal output",
        "stderr": "pty diagnostic",
        "is_exited": true,
        "exit_code": 0
    });

    render_tool_output_common(
        &mut renderer,
        &handle,
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "printf test"}),
        &output,
        true,
        None,
        None,
    )
    .await
    .expect("PTY stderr should render");

    let visible_text = std::iter::from_fn(|| receiver.try_recv().ok())
        .filter_map(|command| match command {
            InlineCommand::AppendLine { segments, .. } => {
                Some(segments.into_iter().map(|segment| segment.text).collect::<String>())
            }
            InlineCommand::Inline { segment, .. } => Some(segment.text),
            InlineCommand::Diagnostic(diagnostic) => Some(diagnostic.summary),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(visible_text.contains("stderr: pty diagnostic"), "visible text: {visible_text:?}");
    assert!(!visible_text.contains("• Ran printf test ·"));
}
