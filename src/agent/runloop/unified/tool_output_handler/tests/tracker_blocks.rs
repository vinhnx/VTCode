use super::*;

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
