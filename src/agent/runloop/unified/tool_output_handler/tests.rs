use super::*;
use std::io::{IsTerminal, stdin};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::{RwLock, mpsc::unbounded_channel};
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::tools::ApprovalRecorder;
use vtcode_core::tools::registry::{ToolExecutionError, ToolRegistry};
use vtcode_core::tools::result_cache::{ToolCacheKey, ToolResultCache};
use vtcode_core::ui::inline_theme_from_core_styles;
use vtcode_core::ui::theme;
use vtcode_ui::tui::app::{InlineCommand, InlineHandle, SessionOptions, spawn_session_with_options};

fn build_harness_state() -> crate::agent::runloop::unified::run_loop_context::HarnessTurnState {
    crate::agent::runloop::unified::run_loop_context::HarnessTurnState::new(
        crate::agent::runloop::unified::run_loop_context::TurnRunId("test-run".to_string()),
        crate::agent::runloop::unified::run_loop_context::TurnId("test-turn".to_string()),
        4,
        60,
        0,
    )
}

fn dummy_handle() -> InlineHandle {
    InlineHandle::new_for_tests(unbounded_channel().0)
}

#[test]
#[serial_test::serial(transcript_state)]
fn successful_task_tracker_replacement_contains_header_plus_current_task() {
    // Compact transcript contract: header plus the single current-task row
    // with the distinct `»` visual. The full compact tree stays panel-only.
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let first = serde_json::json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "level": 0, "description": "Release", "status": "in_progress" },
                { "index_path": "1.1", "level": 1, "description": "Update version", "status": "completed" },
                { "index_path": "1.2", "level": 1, "description": "Run checks", "status": "in_progress" }
            ]
        }
    });
    let second = serde_json::json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "level": 0, "description": "Release", "status": "completed" },
                { "index_path": "1.1", "level": 1, "description": "Update version", "status": "completed" },
                { "index_path": "1.2", "level": 1, "description": "Run checks", "status": "completed" }
            ]
        }
    });

    let first_progress = task_tracker_block_lines(&first, false);
    let second_progress = task_tracker_block_lines(&second, false);
    let first_panel = crate::agent::runloop::tool_output::tracker_tree_body_lines(&first);
    let second_panel = crate::agent::runloop::tool_output::tracker_tree_body_lines(&second);

    assert_eq!(tracker_texts(&first_progress), vec!["• Release 1/3", "  » Run checks"]);
    assert_eq!(
        first_progress[1].status,
        Some(TaskItemStatus::InProgress),
        "current row carries its status for accent styling"
    );
    assert_eq!(tracker_texts(&second_progress), vec!["• Release 3/3"]);
    assert!(
        first_panel.iter().any(|line| line.contains("Update version")),
        "panel body keeps the compact tree: {first_panel:?}"
    );
    assert!(
        second_panel.iter().all(|line| !line.starts_with("• ")),
        "panel body must not re-include the transcript progress header"
    );

    apply_task_tracker_block(&handle, first_progress);
    apply_task_tracker_block(&handle, second_progress);

    let replacement = std::iter::from_fn(|| receiver.try_recv().ok()).find_map(|command| match command {
        InlineCommand::ReplaceLast { count, lines, .. } => Some((count, lines)),
        _ => None,
    });
    let (count, rows) = replacement.expect("second tracker update should replace the previous progress line");
    let rows = rows
        .into_iter()
        .map(|row| row.into_iter().map(|segment| segment.text).collect::<String>())
        .collect::<Vec<_>>();

    assert_eq!(count, 2);
    assert_eq!(rows, vec!["• Release 3/3"]);
    assert!(rows.iter().all(|row| !row.contains("next:") && !row.contains("├")));
}

#[test]
#[serial_test::serial(transcript_state)]
fn identical_task_tracker_block_is_not_appended_twice() {
    // Approval handoff + pipeline replay (or repeated `list` calls) emit
    // the same payload. The transcript must keep one block, not two.
    transcript::clear();
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let payload = serde_json::json!({
        "status": "updated",
        "checklist": {
            "completed": 1,
            "total": 3,
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Release", "status": "in_progress" },
                { "index_path": "1.1", "description": "Update version", "status": "completed" },
                { "index_path": "1.2", "description": "Run checks", "status": "pending" }
            ]
        }
    });
    let lines = task_tracker_block_lines(&payload, false);

    apply_task_tracker_block(&handle, lines.clone());
    // Drain the initial append so only post-repeat commands remain.
    while receiver.try_recv().is_ok() {}
    apply_task_tracker_block(&handle, lines);

    assert!(receiver.try_recv().is_err(), "identical tracker repeat must not emit another transcript command");
    transcript::clear();
}

fn tracker_texts(lines: &[TrackerLine]) -> Vec<&str> {
    lines.iter().map(|line| line.text.as_str()).collect()
}

fn plain_tracker_lines(texts: &[&str]) -> Vec<TrackerLine> {
    texts.iter().map(|text| TrackerLine::plain((*text).to_string())).collect()
}

#[test]
fn looks_like_tracker_progress_line_rejects_questions_summaries() {
    assert!(looks_like_tracker_progress_line("• Tasks 0/2"));
    assert!(looks_like_tracker_progress_line("• Release 1/4"));
    assert!(!looks_like_tracker_progress_line("• Questions 2/5 answered"));
    assert!(!looks_like_tracker_progress_line("• Plan — research/synthesis"));
    assert!(!looks_like_tracker_progress_line("• Ran cargo check"));
}

#[test]
fn looks_like_tracker_content_accepts_tree_rows_and_truncation() {
    assert!(looks_like_tracker_content("• Release 1/3"));
    assert!(looks_like_tracker_content("  ├ Investigate cache miss"));
    assert!(looks_like_tracker_content("  └ Verify with cargo check"));
    assert!(looks_like_tracker_content("  │ Defer eager setup"));
    assert!(looks_like_tracker_content("  » Defer eager setup"));
    // Legacy glyph rows remain recognizable so in-flight blocks still
    // replace cleanly across the upgrade.
    assert!(looks_like_tracker_content("  ├ □ Investigate cache miss"));
    assert!(looks_like_tracker_content("  » [-] Defer eager setup"));
    assert!(looks_like_tracker_content("  … 5 more"));
    assert!(!looks_like_tracker_content("• Questions 2/5 answered"));
    assert!(!looks_like_tracker_content("• Ran cargo check"));
    assert!(!looks_like_tracker_content("Now applying the edit"));
}

#[test]
fn task_tracker_block_lines_expanded_shows_items_with_asymmetric_statuses() {
    let payload = serde_json::json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate cache miss", "status": "completed" },
                { "index_path": "2", "description": "Defer eager setup", "status": "in_progress" },
                { "index_path": "3", "description": "Verify with cargo check", "status": "pending" },
            ]
        }
    });

    let compact = task_tracker_block_lines(&payload, false);
    let expanded = task_tracker_block_lines(&payload, true);

    assert_eq!(tracker_texts(&compact), vec!["• Release 1/3", "  » Defer eager setup"]);
    assert_eq!(compact[1].status, Some(TaskItemStatus::InProgress));
    assert_eq!(
        expanded,
        vec![
            TrackerLine::plain("• Release 1/3".to_string()),
            TrackerLine::row("  ├ Investigate cache miss".to_string(), TaskItemStatus::Completed),
            TrackerLine::row("  ├ Defer eager setup".to_string(), TaskItemStatus::InProgress),
            TrackerLine::row("  └ Verify with cargo check".to_string(), TaskItemStatus::Pending),
        ]
    );
}

#[test]
#[serial_test::serial(transcript_state)]
fn write_tracker_progress_transcript_replaces_compact_with_expanded_block() {
    transcript::clear();
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    write_tracker_progress_transcript(&handle, plain_tracker_lines(&["• Release 0/2"]));
    while receiver.try_recv().is_ok() {}
    let expanded = vec![
        TrackerLine::plain("• Release 1/2".to_string()),
        TrackerLine::row("  ├ Investigate cache miss".to_string(), TaskItemStatus::Completed),
        TrackerLine::row("  └ Defer eager setup".to_string(), TaskItemStatus::Pending),
    ];

    write_tracker_progress_transcript(&handle, expanded.clone());

    let replacement = std::iter::from_fn(|| receiver.try_recv().ok()).find_map(|command| match command {
        InlineCommand::ReplaceLast { count, lines, .. } => Some((count, lines)),
        _ => None,
    });
    let (count, rows) = replacement.expect("compact block should be replaced by expanded block");
    let rows = rows
        .into_iter()
        .map(|row| row.into_iter().map(|segment| segment.text).collect::<String>())
        .collect::<Vec<_>>();
    assert_eq!(count, 1);
    assert_eq!(rows, tracker_texts(&expanded));
    assert_eq!(transcript::snapshot(), tracker_texts(&expanded).into_iter().map(str::to_string).collect::<Vec<_>>());
    transcript::clear();
}

#[test]
#[serial_test::serial(transcript_state)]
fn write_tracker_progress_transcript_uses_ui_write_len_not_transcript_only_tail() {
    transcript::clear();
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    write_tracker_progress_transcript(&handle, plain_tracker_lines(&["• Release 0/2"]));
    while receiver.try_recv().is_ok() {}
    // UI-only append after tracker (Questions-style) must not be clobbered
    // via TRANSCRIPT-derived replace count once Questions dual-writes; when
    // TRANSCRIPT still has tracker as tail but UI has an extra line, the
    // helper uses recorded UI write length.
    handle.append_line(
        InlineMessageKind::Info,
        vec![InlineSegment {
            text: "• Questions 1/2 answered".to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );

    write_tracker_progress_transcript(&handle, plain_tracker_lines(&["• Release 1/2"]));

    let commands: Vec<InlineCommand> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
    let replaced_ui_len = commands.iter().find_map(|command| match command {
        InlineCommand::ReplaceLast { count, lines, .. } => {
            let rows: Vec<String> = lines
                .iter()
                .map(|row| row.iter().map(|segment| segment.text.clone()).collect::<String>())
                .collect();
            Some((*count, rows))
        }
        _ => None,
    });
    // TRANSCRIPT still sees tracker at tail (Questions not mirrored here),
    // so replace happens — but only the helper's UI write length (1), and
    // the replacement content is the new progress line.
    let (count, rows) = replaced_ui_len.expect("tracker update should replace remembered UI write");
    assert_eq!(count, 1);
    assert_eq!(rows, vec!["• Release 1/2".to_string()]);
    transcript::clear();
}

#[test]
#[serial_test::serial(transcript_state)]
fn write_tracker_progress_transcript_replaces_previous_block_on_change() {
    transcript::clear();
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let first = plain_tracker_lines(&["• Release 0/2"]);
    let second = plain_tracker_lines(&["• Release 1/2"]);

    write_tracker_progress_transcript(&handle, first);
    while receiver.try_recv().is_ok() {}
    write_tracker_progress_transcript(&handle, second.clone());

    let replacement = std::iter::from_fn(|| receiver.try_recv().ok()).find_map(|command| match command {
        InlineCommand::ReplaceLast { count, lines, .. } => Some((count, lines)),
        _ => None,
    });
    let (count, rows) = replacement.expect("changed progress must replace the previous tracker block");
    let rows = rows
        .into_iter()
        .map(|row| row.into_iter().map(|segment| segment.text).collect::<String>())
        .collect::<Vec<_>>();
    assert_eq!(count, 1);
    assert_eq!(rows, tracker_texts(&second));
    transcript::clear();
}

#[test]
#[serial_test::serial(transcript_state)]
fn write_tracker_progress_transcript_does_not_clobber_intervening_lines() {
    transcript::clear();
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    write_tracker_progress_transcript(&handle, plain_tracker_lines(&["• Release 0/2"]));
    while receiver.try_recv().is_ok() {}
    // Intervening transcript content after the tracker block.
    transcript::append("Now applying the edit");

    write_tracker_progress_transcript(&handle, plain_tracker_lines(&["• Release 1/2"]));

    let commands: Vec<InlineCommand> = std::iter::from_fn(|| receiver.try_recv().ok()).collect();
    let appended = commands
        .iter()
        .any(|command| matches!(command, InlineCommand::AppendLine { .. }));
    let replaced = commands
        .iter()
        .any(|command| matches!(command, InlineCommand::ReplaceLast { .. }));
    assert!(appended && !replaced, "must append after intervening lines instead of replace_last clobber");
    let snap = transcript::snapshot();
    assert!(snap.contains(&"Now applying the edit".to_string()));
    assert_eq!(
        snap.iter().filter(|line| line.starts_with("• Release")).count(),
        2,
        "old block remains under intervening line; new block appended (no silent clobber)"
    );
    transcript::clear();
}

#[test]
#[serial_test::serial(transcript_state)]
fn write_tracker_progress_transcript_replaces_tail_tracker_progress_line() {
    transcript::clear();
    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    transcript::append("• Tasks 0/1");
    while receiver.try_recv().is_ok() {}

    write_tracker_progress_transcript(&handle, plain_tracker_lines(&["• Release 1/1"]));

    let replacement = std::iter::from_fn(|| receiver.try_recv().ok()).find_map(|command| match command {
        InlineCommand::ReplaceLast { count, lines, .. } => Some((count, lines)),
        _ => None,
    });
    let (count, rows) = replacement.expect("tail tracker progress line should be replaced");
    let rows = rows
        .into_iter()
        .map(|row| row.into_iter().map(|segment| segment.text).collect::<String>())
        .collect::<Vec<_>>();
    assert_eq!(count, 1);
    assert_eq!(rows, vec!["• Release 1/1".to_string()]);
    assert_eq!(transcript::snapshot(), vec!["• Release 1/1".to_string()]);
    transcript::clear();
}

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

#[test]
fn ordered_stream_texts_deduplicates_merged_output_aliases() {
    let output = serde_json::json!({
        "output": "stdout line\nstderr line",
        "stdout": "stdout line",
        "stderr": "stderr line"
    });

    assert_eq!(ordered_stream_texts(&output), vec!["stdout line\nstderr line"]);
}

#[test]
fn ordered_stream_texts_preserves_distinct_pipe_streams() {
    let output = serde_json::json!({
        "output": "merged line",
        "stdout": "stdout line",
        "stderr": "stderr line"
    });

    assert_eq!(ordered_stream_texts(&output), vec!["merged line", "stdout line", "stderr line"]);
}

#[test]
fn canonical_pipe_streams_preserve_unrepresented_content() {
    let output = serde_json::json!({
        "stdout": "command output",
        "content": "additional structured content"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![
            (Some("stdout"), "command output"),
            (None, "additional structured content")
        ]
    );
}

#[test]
fn canonical_pipe_streams_keep_merged_output_once() {
    let output = serde_json::json!({
        "output": "stdout line\nstderr line",
        "stdout": "stdout line",
        "stderr": "stderr line"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].label, None);
    assert_eq!(streams[0].text, "stdout line\nstderr line");
}

#[test]
fn canonical_pipe_streams_label_separate_streams() {
    let output = serde_json::json!({
        "stdout": "stdout line",
        "stderr": "stderr line"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![(Some("stdout"), "stdout line"), (Some("stderr"), "stderr line")]
    );
}

#[test]
fn canonical_pipe_streams_preserve_identical_named_streams() {
    let output = serde_json::json!({
        "stdout": "same output",
        "stderr": "same output"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![(Some("stdout"), "same output"), (Some("stderr"), "same output")]
    );
}

#[test]
fn canonical_pipe_streams_require_distinct_merged_occurrences() {
    let single_occurrence = serde_json::json!({
        "output": "same output",
        "stdout": "same output",
        "stderr": "same output"
    });
    let streams = canonical_pipe_streams(&single_occurrence);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![(Some("stdout"), "same output"), (Some("stderr"), "same output")]
    );
    assert_eq!(stderr_for_inline_display(&single_occurrence), Some("same output"));

    let distinct_occurrences = serde_json::json!({
        "output": "same output\nsame output",
        "stdout": "same output",
        "stderr": "same output"
    });
    let streams = canonical_pipe_streams(&distinct_occurrences);
    assert_eq!(streams.len(), 1);
    assert_eq!(streams[0].label, None);
    assert_eq!(streams[0].text, "same output\nsame output");
    assert_eq!(stderr_for_inline_display(&distinct_occurrences), None);
}

#[test]
fn canonical_pipe_streams_preserve_merged_lines_when_named_streams_overlap() {
    let output = serde_json::json!({
        "output": "same output\nmerged-only output",
        "stdout": "same output",
        "stderr": "same output"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![
            (Some("stdout"), "same output"),
            (Some("stderr"), "same output"),
            (None, "same output\nmerged-only output")
        ]
    );
}

#[test]
fn canonical_pipe_streams_preserve_full_named_alias() {
    let output = serde_json::json!({
        "output": "stdout line",
        "stdout": "stdout line\nsecond stdout line",
        "stderr": "stderr line"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![
            (Some("stdout"), "stdout line\nsecond stdout line"),
            (Some("stderr"), "stderr line")
        ]
    );
}

#[test]
fn canonical_pipe_streams_preserve_distinct_streams_when_output_is_preview() {
    let output = serde_json::json!({
        "output": "preview",
        "stdout": "preview\nstdout line",
        "stderr": "preview\nstderr line"
    });

    let streams = canonical_pipe_streams(&output);
    assert_eq!(
        streams.iter().map(|stream| (stream.label, stream.text)).collect::<Vec<_>>(),
        vec![
            (Some("stdout"), "preview\nstdout line"),
            (Some("stderr"), "preview\nstderr line")
        ]
    );
}

#[test]
fn normalize_terminal_output_lines_handles_ansi_rewrites_and_blanks() {
    let capture = "stale\n\x1b[2J\x1b[H\x1b[31mred\x1b[0m\rfinal\n\nlast\n";

    assert_eq!(normalize_terminal_output_lines(capture), vec!["final", "", "last"]);
    assert_eq!(normalize_terminal_output_lines("abc\x08d\n"), vec!["abd"]);
}

#[test]
fn build_pipe_command_output_lines_labels_stderr_once() {
    let output = serde_json::json!({
        "stdout": "normal output",
        "stderr": "diagnostic output",
        "exit_code": 1
    });

    assert_eq!(
        build_pipe_command_output_lines(
            tools::EXECUTE_CODE,
            &serde_json::json!({"command": "printf test"}),
            &output,
            None,
            ToolDisplayStatus::Failure,
        ),
        vec![
            "• Ran printf test",
            "  stdout:",
            "    normal output",
            "  stderr:",
            "    diagnostic output",
            "    ✗ run error, exit code: 1",
        ]
    );
}

#[test]
fn build_merged_command_output_lines_keeps_complete_capture_and_status_once() {
    let output = serde_json::json!({
        "exit_code": 2,
        "critical_note": "output was retained in the current session"
    });
    let capture = "stdout line\nstderr line\n";

    let lines = build_merged_command_output_lines(
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "long command"}),
        capture,
        None,
        &output,
        ToolDisplayStatus::Failure,
    );

    assert_eq!(lines[0], "• Ran long command");
    assert!(lines.contains(&"  └ stdout line".to_string()));
    assert!(lines.contains(&"    stderr line".to_string()));
    assert!(lines.contains(&"    output was retained in the current session".to_string()));
    assert_eq!(lines.iter().filter(|line| line.contains("stderr line")).count(), 1);
    assert_eq!(lines.iter().filter(|line| line.contains("exit code: 2")).count(), 1);
}

#[test]
fn build_merged_command_output_lines_keeps_distinct_stderr_without_capture() {
    let output = serde_json::json!({"stderr": "diagnostic output"});

    let lines = build_merged_command_output_lines(
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "long command"}),
        "",
        None,
        &output,
        ToolDisplayStatus::Success,
    );

    assert_eq!(lines, vec!["• Ran long command", "  stderr:", "    diagnostic output",]);
}

#[test]
fn build_merged_command_output_lines_labels_distinct_stderr_with_merged_output() {
    let output = serde_json::json!({
        "output": "normal output",
        "stderr": "diagnostic output"
    });

    let lines = build_merged_command_output_lines(
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "long command"}),
        "normal output\ndiagnostic output\n",
        None,
        &output,
        ToolDisplayStatus::Success,
    );

    assert_eq!(
        lines,
        vec![
            "• Ran long command",
            "  └ normal output",
            "  stderr:",
            "    diagnostic output",
        ]
    );
}

#[test]
fn build_merged_command_output_lines_labels_identical_named_streams_without_merged_output() {
    let output = serde_json::json!({
        "stdout": "same output",
        "stderr": "same output"
    });

    let lines = build_merged_command_output_lines(
        tools::RUN_PTY_CMD,
        &serde_json::json!({"command": "long command"}),
        "same output\nsame output",
        None,
        &output,
        ToolDisplayStatus::Success,
    );

    assert_eq!(
        lines,
        vec![
            "• Ran long command",
            "  stdout:",
            "    same output",
            "  stderr:",
            "    same output",
        ]
    );
}

#[tokio::test]
async fn pty_capture_reads_complete_workspace_spool() {
    let workspace = TempDir::new().expect("workspace temp dir");
    let spool_path = workspace.path().join(".vtcode/context/tool_outputs/pty.txt");
    tokio::fs::create_dir_all(spool_path.parent().expect("spool parent"))
        .await
        .expect("create spool parent");
    let complete_output = "first complete line\nsecond complete line\n";
    tokio::fs::write(&spool_path, complete_output).await.expect("write spool");

    let output = serde_json::json!({
        "spool_path": ".vtcode/context/tool_outputs/pty.txt",
        "spooled_bytes": complete_output.len(),
        "spool_sha256": vtcode_commons::utils::calculate_sha256(complete_output.as_bytes()),
        "spool_state": "completed",
        "output": "first preview line"
    });

    assert_eq!(
        load_complete_output(&output, Some(workspace.path())).await.as_deref(),
        Some("first complete line\nsecond complete line\n")
    );
}

#[tokio::test]
async fn pty_capture_rejects_spool_outside_workspace() {
    let workspace = TempDir::new().expect("workspace temp dir");
    let outside = TempDir::new().expect("outside temp dir");
    let spool_path = outside.path().join("pty.txt");
    tokio::fs::write(&spool_path, "secret outside workspace")
        .await
        .expect("write outside spool");

    let output = serde_json::json!({ "spool_path": spool_path });

    assert!(load_complete_output(&output, Some(workspace.path())).await.is_none());
}

#[tokio::test]
async fn pty_capture_rejects_malformed_spool_metadata_without_inline_fallback() {
    let workspace = TempDir::new().expect("workspace temp dir");
    let output = serde_json::json!({
        "spool_path": null,
        "output": "untrusted inline fallback"
    });

    assert!(load_complete_output(&output, Some(workspace.path())).await.is_none());
}

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
        matches!(command, InlineCommand::AppendLine { segments, .. }
            if segments.iter().any(|segment| segment.text.contains("capture unavailable")))
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
        matches!(command, InlineCommand::AppendLine { segments, .. }
            if segments.iter().map(|segment| segment.text.as_str()).collect::<String>().contains("stderr: diagnostic output"))
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
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(visible_text.contains("stderr: pty diagnostic"), "visible text: {visible_text:?}");
    assert!(!visible_text.contains("• Ran printf test ·"));
}

#[tokio::test]
async fn test_handle_pipeline_output_collects_modified_files_and_records_stats() {
    if !stdin().is_terminal() {
        eprintln!("Skipping TUI-dependent test in non-interactive environment");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path().to_path_buf();

    let mut registry = ToolRegistry::new(workspace.clone()).await;
    let permission_cache_arc = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(vtcode_core::config::PermissionsConfig::default()));

    let mut session = spawn_session_with_options(
        inline_theme_from_core_styles(&theme::active_styles()),
        SessionOptions {
            inline_rows: 10,
            workspace_root: Some(workspace.clone()),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    let cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
    let key = ToolCacheKey::new("read_file", "{}", "/tmp/foo.txt");
    {
        let mut c = cache.write().await;
        c.insert_arc(key.clone(), Arc::new("{}".to_string()));
        assert!(c.get(&key).is_some());
    }

    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let mut session_stats = SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel = McpPanelState::new(10, true);
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let traj = TrajectoryLogger::new(&workspace);
    let tools = Arc::new(RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state();
    let mut ctx = RunLoopContext::new(
        &mut renderer,
        &handle,
        &mut registry,
        &tools,
        &cache,
        &permission_cache_arc,
        &permissions_state,
        &decision_ledger,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel,
        &approval_recorder,
        &mut session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"ok": true}),
        stdout: None,
        modified_files: vec!["/tmp/foo.txt".to_string()],
        command_success: true,
    });

    let (mod_files, _last_stdout) =
        handle_pipeline_output(&mut ctx, "read_file", &serde_json::json!({}), &outcome, None::<&VTCodeConfig>)
            .await
            .expect("handle should succeed");

    assert_eq!(mod_files, vec![PathBuf::from("/tmp/foo.txt")]);

    // Cache invalidation is handled in execution side-effects, not output rendering.
    {
        let c = cache.write().await;
        assert!(c.get(&key).is_some());
    }

    // Ensure session stats were updated
    let rec = session_stats.sorted_tools();
    assert!(rec.contains(&"read_file".to_string()));
}

#[tokio::test]
async fn task_tracker_updates_replace_previous_inline_block() {
    transcript::clear();

    let (sender, mut receiver) = unbounded_channel();
    let handle = InlineHandle::new_for_tests(sender);
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let mut stats = SessionStats::default();
    let mut mcp = McpPanelState::default();

    let first = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "status": "updated",
            "view": {
                "title": "Respond to user greeting and assess next steps",
                "lines": [
                    {"display": "├ ✔ Greet user and summarize current workspace state"},
                    {"display": "├ > Ask what task they'd like to tackle"},
                    {"display": "└ • Offer to provide workspace tour if needed"}
                ]
            },
            "checklist": {
                "title": "Respond to user greeting and assess next steps",
                "total": 3,
                "completed": 1,
                "in_progress": 1,
                "pending": 1,
                "blocked": 0,
                "progress_percent": 33,
                "items": [
                    {"index": 1, "description": "Greet user and summarize current workspace state", "status": "completed"},
                    {"index": 2, "description": "Ask what task they'd like to tackle", "status": "in_progress"},
                    {"index": 3, "description": "Offer to provide workspace tour if needed", "status": "pending"}
                ]
            },
            "message": "Item 2 status changed: pending → in_progress"
        }),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });
    let second = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({
            "status": "updated",
            "view": {
                "title": "Respond to user greeting and assess next steps",
                "lines": [
                    {"display": "├ ✔ Greet user and summarize current workspace state"},
                    {"display": "├ ✔ Ask what task they'd like to tackle"},
                    {"display": "└ • Offer to provide workspace tour if needed"}
                ]
            },
            "checklist": {
                "title": "Respond to user greeting and assess next steps",
                "total": 3,
                "completed": 2,
                "in_progress": 0,
                "pending": 1,
                "blocked": 0,
                "progress_percent": 67,
                "items": [
                    {"index": 1, "description": "Greet user and summarize current workspace state", "status": "completed"},
                    {"index": 2, "description": "Ask what task they'd like to tackle", "status": "completed"},
                    {"index": 3, "description": "Offer to provide workspace tour if needed", "status": "pending"}
                ]
            },
            "message": "Item 2 status changed: in_progress → completed"
        }),
        stdout: None,
        modified_files: vec![],
        command_success: true,
    });

    let args = serde_json::json!({"action": "update", "index": 2, "status": "in_progress"});
    let mut output_ctx = OutcomeContext {
        workspace_root: None,
        session_stats: &mut stats,
        renderer: &mut renderer,
        handle: &handle,
        mcp_panel_state: &mut mcp,
        vt_config: None::<&VTCodeConfig>,
    };

    process_outcome_common(&mut output_ctx, tools::TASK_TRACKER, &args, &first)
        .await
        .expect("first tracker render should succeed");

    let args = serde_json::json!({"action": "update", "index": 2, "status": "completed"});
    process_outcome_common(&mut output_ctx, tools::TASK_TRACKER, &args, &second)
        .await
        .expect("second tracker render should succeed");

    let mut saw_task_panel_update = false;
    while let Ok(command) = receiver.try_recv() {
        if matches!(command, InlineCommand::ShowTransient { .. }) {
            saw_task_panel_update = true;
        }
    }

    assert!(saw_task_panel_update, "expected tracker updates to refresh the dedicated task panel");
}

#[tokio::test]
async fn test_handle_pipeline_output_mcp_events() {
    if !stdin().is_terminal() {
        eprintln!("Skipping TUI-dependent test in non-interactive environment");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path().to_path_buf();

    let mut registry = ToolRegistry::new(workspace.clone()).await;
    let permission_cache_arc = Arc::new(RwLock::new(ToolPermissionCache::new()));
    let permissions_state = Arc::new(RwLock::new(vtcode_core::config::PermissionsConfig::default()));

    let mut session = spawn_session_with_options(
        inline_theme_from_core_styles(&theme::active_styles()),
        SessionOptions {
            inline_rows: 10,
            workspace_root: Some(workspace.clone()),
            ..SessionOptions::default()
        },
    )
    .unwrap();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

    let cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
    let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
    let mut session_stats = SessionStats::default();
    let mut plan_session =
        crate::agent::runloop::unified::planning_workflow_state::PlanningWorkflowSessionState::default();
    let mut mcp_panel = McpPanelState::new(10, true);
    let approval_recorder = ApprovalRecorder::new(workspace.clone());
    let traj = TrajectoryLogger::new(&workspace);
    let tools = Arc::new(RwLock::new(Vec::new()));

    let mut harness_state = build_harness_state();
    let mut ctx = RunLoopContext::new(
        &mut renderer,
        &handle,
        &mut registry,
        &tools,
        &cache,
        &permission_cache_arc,
        &permissions_state,
        &decision_ledger,
        &mut session_stats,
        &mut plan_session,
        &mut mcp_panel,
        &approval_recorder,
        &mut session,
        None,
        &traj,
        &mut harness_state,
        None,
    );

    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: serde_json::json!({"exit_code": 0}),
        stdout: Some("ok".to_string()),
        modified_files: vec![],
        command_success: true,
    });

    let (_mod_files, _last_stdout) =
        handle_pipeline_output(&mut ctx, "mcp_example", &serde_json::json!({}), &outcome, None::<&VTCodeConfig>)
            .await
            .expect("handle should succeed");

    assert!(ctx.mcp_panel_state.event_count() > 0);
}
