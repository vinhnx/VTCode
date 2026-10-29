use super::*;

#[tokio::test]
async fn test_renderer_records_mcp_event_for_mcp_tool() {
    let mut renderer = AnsiRenderer::stdout();

    // Note: tests involving `apply_turn_outcome` live in `turn/turn_loop.rs` and can be added there
    let mut stats = SessionStats::default();
    let mut mcp = McpPanelState::new(32, true); // enabled

    let output_json = serde_json::json!({"exit_code":0});
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: output_json.clone(),
        stdout: Some("ok".to_string()),
        modified_files: vec![],
        command_success: true,
    });

    let handle = dummy_handle();
    let mut output_ctx = OutcomeContext {
        workspace_root: None,
        session_stats: &mut stats,
        renderer: &mut renderer,
        handle: &handle,
        mcp_panel_state: &mut mcp,
        vt_config: None::<&VTCodeConfig>,
    };
    let (_mod_files, _touched, _last_stdout) =
        process_outcome_common(&mut output_ctx, "mcp_example", &serde_json::json!({}), &outcome)
            .await
            .expect("render should succeed")
            .into_full_tuple();

    // Ensure mcp panel recorded an event
    assert!(mcp.event_count() > 0);
}
/// Recording the MCP panel event must not divert canonical `mcp::` names away
/// from the shared inline render path. The pre-fix parser routed them into the
/// panel-only branch, silently dropping their visible transcript output.
#[tokio::test]
async fn canonical_mcp_name_still_renders_inline_output() {
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let mut stats = SessionStats::default();
    let mut mcp = McpPanelState::new(32, true);

    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0, "output": "visible mcp output"}),
        stdout: Some("visible mcp output".to_string()),
        modified_files: vec![],
        command_success: true,
    });

    let mut output_ctx = OutcomeContext {
        workspace_root: None,
        session_stats: &mut stats,
        renderer: &mut renderer,
        handle: &handle,
        mcp_panel_state: &mut mcp,
        vt_config: None::<&VTCodeConfig>,
    };
    process_outcome_common(&mut output_ctx, "mcp::time::get_current_time", &serde_json::json!({}), &outcome)
        .await
        .expect("render should succeed");

    // Panel event recorded AND inline output emitted: neither surface is lost.
    assert!(mcp.event_count() > 0, "canonical mcp:: names must reach the MCP panel");
    let commands = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
    assert!(!commands.is_empty(), "canonical mcp:: names must still emit inline transcript output");
}
#[tokio::test]
async fn spooled_exec_output_keeps_transcript_at_reference_only() {
    let mut renderer = AnsiRenderer::stdout();
    let mut stats = SessionStats::default();
    let mut mcp = McpPanelState::default();
    let handle = dummy_handle();

    transcript::clear();

    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "output": "preview text that should stay out of transcript persistence",
            "spool_path": ".vtcode/context/tool_outputs/exec_command_1.txt",
            "exit_code": 0,
            "is_exited": true
        }),
        stdout: Some("preview text that should stay out of transcript persistence".to_string()),
        modified_files: vec![],
        command_success: true,
    });

    let mut output_ctx = OutcomeContext {
        workspace_root: None,
        session_stats: &mut stats,
        renderer: &mut renderer,
        handle: &handle,
        mcp_panel_state: &mut mcp,
        vt_config: None::<&VTCodeConfig>,
    };

    process_outcome_common(
        &mut output_ctx,
        tools::UNIFIED_EXEC,
        &serde_json::json!({
            "action": "run",
            "command": "cargo check -p vtcode-core"
        }),
        &outcome,
    )
    .await
    .expect("render should succeed");

    let transcript_lines = transcript::snapshot();
    let transcript_text = transcript_lines.join("\n");
    let stripped_text = vtcode_core::utils::ansi_parser::strip_ansi(&transcript_text);
    assert!(stripped_text.contains("Large output was spooled to"), "Transcript: {stripped_text:?}");
    assert!(!stripped_text.contains("preview text that should stay out of transcript persistence"));

    transcript::clear();
}
#[tokio::test]
async fn inline_tool_output_viewer_retains_complete_spooled_capture() {
    let workspace = TempDir::new().expect("workspace temp dir");
    let spool_path = workspace.path().join(".vtcode/context/tool_outputs/exec_command_1.txt");
    tokio::fs::create_dir_all(spool_path.parent().expect("spool parent"))
        .await
        .expect("create spool parent");
    let complete_output = "first complete line\nsecond complete line\n";
    tokio::fs::write(&spool_path, complete_output).await.expect("write spool");

    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let mut stats = SessionStats::default();
    let mut mcp = McpPanelState::default();
    transcript::clear();
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "output": "preview line",
            "spool_path": ".vtcode/context/tool_outputs/exec_command_1.txt",
            "spooled_bytes": complete_output.len(),
            "spool_sha256": vtcode_commons::utils::calculate_sha256(complete_output.as_bytes()),
            "spool_state": "completed",
            "exit_code": 0,
            "is_exited": true
        }),
        stdout: Some("preview line".to_string()),
        modified_files: vec![],
        command_success: true,
    });
    let mut output_ctx = OutcomeContext {
        workspace_root: Some(workspace.path()),
        session_stats: &mut stats,
        renderer: &mut renderer,
        handle: &handle,
        mcp_panel_state: &mut mcp,
        vt_config: None::<&VTCodeConfig>,
    };

    process_outcome_common(
        &mut output_ctx,
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "cargo check"}),
        &outcome,
    )
    .await
    .expect("render should succeed");

    let mut recorded = None;
    let mut commands = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        if let InlineCommand::RecordToolOutput { lines, .. } = &command {
            recorded = Some(lines.clone());
        }
        commands.push(command);
    }
    let lines = recorded.expect("the complete output should be recorded for the viewer");
    assert_eq!(lines[0], "• Ran cargo check");
    assert!(lines.iter().any(|line| line == "  └ first complete line"));
    assert!(lines.iter().any(|line| line == "    second complete line"));
    assert!(!lines.iter().any(|line| line.contains("preview line")));
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, InlineCommand::CollapsePtyBlock(_))),
        "received {} inline commands",
        commands.len()
    );

    let transcript_text = transcript::snapshot().join("\n");
    assert!(!transcript_text.contains("first complete line"));
    assert!(!transcript_text.contains("second complete line"));
    transcript::clear();
}
