use super::*;

#[test]
#[serial_test::serial(transcript_state)]
fn pipeline_replay_after_approval_handoff_is_not_appended_twice() {
    // `render_created_task_tracker` appends the approval block without
    // updating `HarnessTurnState`. The pipeline replay of the identical
    // payload must still collapse to one block, and the replayed block
    // must be remembered so the next update replaces it.
    transcript::clear();
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let payload = serde_json::json!({
        "status": "updated",
        "checklist": {
            "completed": 1,
            "total": 3,
            "items": [
                { "index_path": "1", "description": "Release", "status": "in_progress" },
                { "index_path": "1.1", "description": "Update version", "status": "completed" },
                { "index_path": "1.2", "description": "Run checks", "status": "pending" }
            ]
        }
    });
    let lines = task_tracker_block_lines(&payload, false);
    // Simulate the approval handoff's transcript write.
    for line in &lines {
        transcript::append(&line.text);
    }

    apply_task_tracker_block(&handle, lines.clone());

    assert!(
        receiver.try_recv().is_err(),
        "pipeline replay after approval handoff must not emit another transcript command"
    );
    assert!(transcript::tracker_block_matches(&lines.iter().map(|line| line.text.clone()).collect::<Vec<_>>()));
    transcript::clear();
}
#[test]
fn task_tracker_row_segments_render_inline_code_without_backticks() {
    let rows = task_tracker_block_segments(&[
        TrackerLine::plain("• Task tracker".to_string()),
        TrackerLine::row("  └ Use `src/main.rs` parser".to_string(), TaskItemStatus::Pending),
    ]);

    assert_eq!(rows.len(), 2);
    // Title keeps its plain single segment.
    assert_eq!(rows[0].len(), 1);
    assert_eq!(rows[0][0].text, "• Task tracker");
    // Tree prefix stays intact while the code span renders styled without
    // literal backticks.
    let text = rows[1].iter().map(|segment| segment.text.as_str()).collect::<String>();
    assert_eq!(text, "  └ Use src/main.rs parser");
    assert!(rows[1].len() > 1, "code span should produce distinct styled segments");
    assert!(!text.contains('`'));
}
#[test]
fn task_tracker_row_segments_keep_diagnostics_plain() {
    let rows = task_tracker_block_segments(&[
        TrackerLine::plain("• Task tracker".to_string()),
        TrackerLine::plain("  Tracker status: error".to_string()),
        TrackerLine::row("  └ Plain action".to_string(), TaskItemStatus::Pending),
    ]);

    assert_eq!(rows[1].len(), 1);
    assert_eq!(rows[1][0].text, "  Tracker status: error");
    let text = rows[2].iter().map(|segment| segment.text.as_str()).collect::<String>();
    assert_eq!(text, "  └ Plain action");
}
#[test]
fn task_tracker_done_rows_render_struck_through_dimmed_italic() {
    use vtcode_core::ui::tui::convert_style;

    let rows = task_tracker_block_segments(&[
        TrackerLine::plain("• Release 1/3".to_string()),
        TrackerLine::row("  ├ Investigate cache miss".to_string(), TaskItemStatus::Completed),
        TrackerLine::row("  ├ Defer eager setup".to_string(), TaskItemStatus::InProgress),
        TrackerLine::row("  └ Verify with cargo check".to_string(), TaskItemStatus::Pending),
    ]);

    let done_style = &rows[1][1].style;
    assert!(done_style.effects.contains(Effects::STRIKETHROUGH), "done rows must strike through: {rows:?}");
    assert!(done_style.effects.contains(Effects::ITALIC), "done rows must italicize: {rows:?}");
    assert!(done_style.effects.contains(Effects::DIMMED), "done rows must dim: {rows:?}");
    // No glyphs leak into any row.
    for row in &rows {
        let text = row.iter().map(|segment| segment.text.as_str()).collect::<String>();
        assert!(!text.contains("□") && !text.contains("[x]") && !text.contains("[-]"), "{text:?}");
    }
    // Pending keeps the default style (no accent, no strike).
    assert_eq!(rows[3][1].style.color, None);
    assert!(!rows[3][1].style.effects.contains(Effects::STRIKETHROUGH));
    // Non-current in-progress rows use the primary accent (the markdown
    // base style contributes weight pipeline-wide; the focused current row
    // adds the `»` marker on top).
    let expected = convert_style(theme::active_styles().primary);
    assert_eq!(rows[2][1].style.color, expected.color);
}
#[test]
fn task_tracker_current_row_segments_carry_primary_accent() {
    use vtcode_core::ui::tui::convert_style;

    let rows = task_tracker_block_segments(&[
        TrackerLine::plain("• Release 1/3".to_string()),
        TrackerLine::row("  » Defer eager setup".to_string(), TaskItemStatus::InProgress),
    ]);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0].text, "• Release 1/3");
    let text = rows[1].iter().map(|segment| segment.text.as_str()).collect::<String>();
    assert_eq!(text, "  » Defer eager setup");
    let expected = convert_style(theme::active_styles().primary);
    assert!(expected.color.is_some(), "primary accent must resolve to a concrete color for the TODO visual");
    assert_eq!(
        rows[1][0].style.color, expected.color,
        "current-task marker must use the primary accent, not the default style"
    );
    assert!(rows[1][0].style.effects.contains(Effects::BOLD), "current-task marker must be bold: {rows:?}");
    assert!(
        rows[1].iter().skip(1).any(|segment| segment.style.color == expected.color),
        "current-task body must keep the accent tint: {rows:?}"
    );
}
#[test]
fn split_tracker_row_prefix_handles_nested_and_parent_rows() {
    assert_eq!(split_tracker_row_prefix("  ├ Investigate"), Some(("  ├ ", "Investigate")));
    assert_eq!(split_tracker_row_prefix("  │ Update version"), Some(("  │ ", "Update version")));
    assert_eq!(split_tracker_row_prefix("  » Defer eager setup"), Some(("  » ", "Defer eager setup")));
    // Legacy glyph rows still split so in-flight blocks degrade gracefully.
    assert_eq!(split_tracker_row_prefix("  ├ □ Investigate"), Some(("  ├ □ ", "Investigate")));
    assert_eq!(split_tracker_row_prefix("  ├ Prepare release"), Some(("  ├ ", "Prepare release")));
    assert_eq!(split_tracker_row_prefix("• Task tracker"), None);
    assert_eq!(split_tracker_row_prefix("  Tracker status: error"), None);
}
// Use Tokio runtime for async test blocks
#[tokio::test]
async fn test_renderer_records_tool_and_collects_modified_files() {
    // Setup a stdout renderer
    let mut renderer = AnsiRenderer::stdout();

    // Prepare session stats and mcp state
    let mut stats = SessionStats::default();
    let mut mcp = McpPanelState::default();

    // Create an outcome that indicates write to /tmp/foo.txt
    let output_json = serde_json::json!({"result":"ok"});
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: output_json.clone(),
        stdout: None,
        modified_files: vec!["/tmp/foo.txt".to_string()],
        command_success: true,
    });

    // Invoke the shared outcome processor via a minimal output context.
    let handle = dummy_handle();
    let mut output_ctx = OutcomeContext {
        workspace_root: None,
        session_stats: &mut stats,
        renderer: &mut renderer,
        handle: &handle,
        mcp_panel_state: &mut mcp,
        vt_config: None::<&VTCodeConfig>,
    };
    let (mod_files, _touched, _last_stdout) =
        process_outcome_common(&mut output_ctx, "write_file", &serde_json::json!({}), &outcome)
            .await
            .expect("render should succeed")
            .into_full_tuple();

    // Confirm the function recorded the tool call
    let recorded = stats.sorted_tools();
    assert!(recorded.contains(&"write_file".to_string()));

    // Confirm the modified files list contains our path
    assert_eq!(mod_files, vec![PathBuf::from("/tmp/foo.txt")]);
}
#[test]
fn tool_call_visual_status_colors_success_failure_and_warning() {
    let palette = ColorPalette::default();
    assert_eq!(ToolDisplayStatus::Success.color(palette), palette.success);
    assert_eq!(ToolDisplayStatus::Failure.color(palette), palette.error);
    assert_eq!(ToolDisplayStatus::Warning.color(palette), palette.warning);

    assert!(matches!(
        ToolDisplayStatus::from_command_output(&serde_json::json!({}), true),
        ToolDisplayStatus::Success
    ));
    assert!(matches!(
        ToolDisplayStatus::from_command_output(&serde_json::json!({}), false),
        ToolDisplayStatus::Failure
    ));
    assert!(matches!(
        ToolDisplayStatus::from_command_output(&serde_json::json!({"warning": "no results"}), true),
        ToolDisplayStatus::Warning
    ));
    assert!(matches!(
        ToolDisplayStatus::from_command_output(&serde_json::json!({"warning": null}), true),
        ToolDisplayStatus::Success
    ));

    assert!(compact_run_completion_line(&serde_json::json!({"exit_code": 0}), ToolDisplayStatus::Success).is_some());
    assert!(
        compact_run_completion_line(&serde_json::json!({"warning": "no results"}), ToolDisplayStatus::Warning)
            .is_some()
    );
    assert!(compact_run_completion_line(&serde_json::json!({}), ToolDisplayStatus::Success).is_none());
}
#[test]
fn compact_hidden_line_count_excludes_distinct_stderr() {
    let output = serde_json::json!({
        "output": "stdout line\nstderr line",
        "stdout": "stdout line",
        "stderr": "stderr line"
    });

    assert_eq!(compact_hidden_line_count(&output, None), 1);
}
#[test]
fn command_extraction_uses_canonical_command_text_shapes() {
    assert_eq!(
        extract_command_line(&serde_json::json!({"command": ["git", "status", "--short"]})),
        Some("git status --short".to_string())
    );
    assert_eq!(
        extract_command_line(&serde_json::json!({"command.0": "git", "command.1": "status"})),
        Some("git status".to_string())
    );
    assert_eq!(
        command_output_header(tools::EXECUTE_CODE, &serde_json::json!({"command": ["git", "status", "--short"]}), None),
        "• Ran git status --short"
    );
}
#[test]
fn command_extraction_leaves_shell_operators_bare() {
    // Screenshot 2026-09-02: `• Ran cat … '2>' '/dev/null' '|' …` quoted
    // every operator. Display joining must keep `|`, `>`, `;` bare and
    // quote only words containing whitespace.
    let args = serde_json::json!({
        "command": [
            "cat", "docs/guides/agent-loop-contract.md",
            "2>/dev/null", "|", "head", "-120",
            ";", "echo", "---"
        ]
    });
    assert_eq!(
        extract_command_line(&args),
        Some("cat docs/guides/agent-loop-contract.md 2>/dev/null | head -120 ; echo ---".to_string())
    );
    assert_eq!(
        command_output_header(tools::EXEC_COMMAND, &args, None),
        "• Ran cat docs/guides/agent-loop-contract.md 2>/dev/null | head -120 ; echo ---"
    );
    // String commands preserve raw shell text (no re-quoting).
    let string_args = serde_json::json!({
        "command": "cat docs/guides/agent-loop-contract.md 2>/dev/null | head -120; echo ---"
    });
    let header = command_output_header(tools::EXEC_COMMAND, &string_args, None);
    assert_eq!(header, "• Ran cat docs/guides/agent-loop-contract.md 2>/dev/null | head -120; echo ---");
    assert!(!header.contains("'|'"), "pipe must not be quoted: {header}");
    assert!(!header.contains("'2>'"), "redirection must not be quoted: {header}");
}
#[test]
fn command_header_for_multiline_python_stays_readable() {
    // Screenshot 2026-09-02: `• Ran python3 -c "` with `tur…ool_calls`
    // continuations and `\'\'` quoting noise. The viewer header must stay
    // single-line with real script content and no nested-quote artifacts.
    let args = serde_json::json!({
        "command": "python3 -c \"\nimport json\nwith open('.vtcode/checkpoints/turn_1032.json') as f: d = json.load(f)\""
    });
    let header = command_output_header(tools::EXEC_COMMAND, &args, None);
    assert!(header.starts_with("• Ran python3 -c "), "got: {header}");
    assert!(header.contains("import json"), "got: {header}");
    assert!(!header.contains('\n'), "newlines leaked: {header:?}");
    assert!(!header.contains("tur…ool"), "mid-string ellipsis leaked: {header}");
    assert!(!header.contains("\\'"), "escaped quotes leaked: {header}");
    assert_ne!(header, "• Ran python3 -c \"");
}
#[test]
fn command_header_shows_full_command_without_truncation() {
    // Transcript Review must show the complete command: a long chained
    // `git add && git commit && git log` must survive in full instead of
    // head-truncating at the 120-char compact preview budget.
    let command = "git add README.md && git commit -m 'docs(readme): expand contributors and sponsors by default' && git log --oneline -1 && git status --short && git diff --stat";
    assert!(command.chars().count() > COMPACT_PREVIEW_LEN, "fixture must overflow compact cap");
    let args = serde_json::json!({ "command": command });
    let header = command_output_header(tools::EXEC_COMMAND, &args, None);
    assert_eq!(header, format!("• Ran {command}"), "got: {header:?}");
    assert!(!header.contains('…'), "truncation ellipsis leaked: {header:?}");
}
