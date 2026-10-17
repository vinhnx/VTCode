use super::*;
use crate::tui::core_tui::app::session::AppSession;
use crate::tui::core_tui::app::types::InlineCommand;
use crate::tui::core_tui::session::config::AppearanceConfig;
use crate::tui::core_tui::types::{InlineSegment, InlineTextStyle, InlineTheme};
use std::sync::Arc;

fn test_session() -> AppSession {
    AppSession::new(InlineTheme::default(), None, 24)
}

#[test]
fn theme_change_rebuilds_unchanged_capture_styles() {
    let mut session = test_session();
    add_block(&mut session, &["captured output"]);
    let mut viewer = ToolOutputViewerState::open(&session, 40, 10);
    let before = viewer.messages[0].rich_lines.clone();
    session.handle_command(InlineCommand::SetTheme {
        theme: InlineTheme {
            foreground: Some(anstyle::Color::Rgb(anstyle::RgbColor(12, 34, 56))),
            ..Default::default()
        },
    });
    viewer.refresh(&session, 40, 10);
    let fresh = ToolOutputViewerState::open(&session, 40, 10);
    assert_ne!(before, viewer.messages[0].rich_lines);
    assert_eq!(viewer.messages[0].rich_lines, fresh.messages[0].rich_lines);
}

#[test]
fn shrinking_viewer_keeps_bottom_follow() {
    let mut session = test_session();
    for _ in 0..20 {
        add_block(&mut session, &["output"]);
    }
    let mut viewer = ToolOutputViewerState::open(&session, 40, 10);
    assert!(viewer.is_at_bottom(10));
    viewer.refresh(&session, 40, 5);
    assert!(viewer.is_at_bottom(5));
}

fn text_segment(text: impl Into<String>) -> InlineSegment {
    InlineSegment {
        text: text.into(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

fn add_block(session: &mut AppSession, lines: &[&str]) {
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: lines.iter().map(|line| (*line).to_string()).collect(),
        ..Default::default()
    });
    session.tool_output_revision += 1;
}

#[test]
fn unanchored_capture_stays_at_its_recorded_transcript_position() {
    let mut session = test_session();
    session.core.push_line(InlineMessageKind::Agent, vec![text_segment("before")]);
    session.handle_command(InlineCommand::RecordToolOutput { id: 91, lines: vec!["captured output".to_string()] });
    session.core.push_line(InlineMessageKind::Agent, vec![text_segment("after")]);

    let mut viewer = ToolOutputViewerState::open(&session, 60, 8);
    let export = viewer.export_text();
    assert!(export.find("before").unwrap() < export.find("captured output").unwrap());
    assert!(export.find("captured output").unwrap() < export.find("after").unwrap());
}

#[test]
fn refresh_appends_without_rebuilding_unchanged_blocks() {
    let mut session = test_session();
    add_block(&mut session, &["• Ran first", "  └ alpha"]);
    add_block(&mut session, &["• Ran second", "  └ beta"]);

    let mut viewer = ToolOutputViewerState::open(&session, 40, 10);
    let original_first = viewer.messages[0].revision;

    add_block(&mut session, &["• Ran third", "  └ gamma"]);
    viewer.refresh(&session, 40, 10);

    assert_eq!(viewer.messages[0].revision, original_first);
    assert_eq!(viewer.messages.len(), 3);
    assert!(viewer.export_text().contains("gamma"));
}

#[test]
fn refresh_rebuilds_grouped_info_head_after_detail_append() {
    let mut session = test_session();
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Info,
        segments: vec![text_segment("first diagnostic detail")],
    });
    let mut viewer = ToolOutputViewerState::open(&session, 60, 10);
    assert!(
        viewer.messages[0]
            .lines
            .iter()
            .any(|line| line.contains("first diagnostic detail"))
    );

    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Info,
        segments: vec![text_segment("second diagnostic detail")],
    });
    viewer.refresh(&session, 60, 10);

    assert!(
        viewer.messages[0]
            .lines
            .iter()
            .any(|line| line.contains("second diagnostic detail"))
    );
    assert!(
        viewer.messages[0]
            .rich_lines
            .iter()
            .map(line_text)
            .any(|line| line.contains("second diagnostic detail"))
    );
}

#[test]
fn grouped_review_revision_aggregates_only_the_group_head() {
    let mut session = test_session();
    for detail in ["first detail", "second detail", "third detail"] {
        session.handle_command(InlineCommand::AppendLine {
            kind: InlineMessageKind::Info,
            segments: vec![text_segment(detail)],
        });
    }

    let sources = collect_review_sources(&session);
    let revisions = sources
        .into_iter()
        .filter_map(|source| match source.kind {
            ReviewSourceKind::Core(index) if index < 3 => Some(source.revision),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(revisions, vec![3, 2, 3]);
}

#[test]
fn refresh_reflows_blocks_when_width_changes() {
    let mut session = test_session();
    add_block(&mut session, &["• Ran a command with a long output line"]);

    let mut viewer = ToolOutputViewerState::open(&session, 80, 10);
    let wide_lines = viewer.messages[0].lines.len();
    viewer.refresh(&session, 12, 10);

    assert!(viewer.messages[0].lines.len() > wide_lines);
}

#[test]
fn search_uses_cached_lowercase_lines() {
    let mut session = test_session();
    add_block(&mut session, &["• Ran Alpha"]);
    add_block(&mut session, &["  └ beta alpha"]);

    let mut viewer = ToolOutputViewerState::open(&session, 40, 10);
    viewer.search.query = "alpha".to_string();
    viewer.recompute_matches();
    let lowered = viewer.messages[0].lowered_lines.as_ref().expect("lowered lines cached")[0].clone();

    viewer.jump_next_match(10);
    viewer.recompute_matches();

    assert!(lowered.contains("alpha"));
    assert_eq!(viewer.search.matches, vec![0, 1]);
}

#[test]
fn export_text_is_cached_until_a_new_block_arrives() {
    let mut session = test_session();
    add_block(&mut session, &["• Ran alpha"]);

    let mut viewer = ToolOutputViewerState::open(&session, 40, 10);
    let exported = viewer.export_text();
    assert!(exported.contains("alpha"));
    assert_eq!(viewer.cached_export_text.as_deref(), Some(exported.as_str()));

    add_block(&mut session, &["• Ran beta"]);
    viewer.refresh(&session, 40, 10);

    assert_eq!(viewer.cached_export_text, None);
    let refreshed = viewer.export_text();
    assert!(refreshed.contains("alpha"));
    assert!(refreshed.contains("beta"));
}

#[test]
fn viewer_keeps_complete_output_for_each_tool_call() {
    let mut session = test_session();
    add_block(
        &mut session,
        &[
            "• Ran cargo check",
            "  └ first complete line",
            "    final complete line",
        ],
    );
    add_block(&mut session, &["• Ran cargo fmt", "  └ fmt complete"]);

    let viewer = ToolOutputViewerState::open(&session, 80, 10);
    let export = viewer.clone().export_text();

    assert!(export.contains("first complete line"));
    assert!(export.contains("final complete line"));
    assert!(export.contains("• Ran cargo fmt"));
    assert!(!export.contains("Ran 2 commands"));
}

#[test]
fn raw_export_strips_ansi_from_complete_captures() {
    let mut session = test_session();
    add_block(&mut session, &["\u{1b}[31m• Ran colored\u{1b}[0m", "\u{1b}[32mcomplete\u{1b}[0m"]);

    let mut viewer = ToolOutputViewerState::open(&session, 80, 10);
    let export = viewer.export_text();

    assert_eq!(export, "• Ran colored\ncomplete");
    assert!(!export.contains('\u{1b}'));
}

#[test]
fn whole_conversation_export_preserves_order_and_complete_tool_output() {
    let mut session = test_session();
    session.handle_command(InlineCommand::AppendPastedMessage {
        kind: InlineMessageKind::User,
        text: "user request".to_string(),
        line_count: 1,
    });
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![text_segment("assistant before tool")],
    });
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Policy,
        segments: vec![text_segment("reasoning")],
    });
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 0,
        lines: vec![
            "• Ran cargo check".to_string(),
            "  └ complete stdout".to_string(),
            "    complete stderr".to_string(),
        ],
    });
    session.handle_command(InlineCommand::AppendToolOutputLine {
        id: 0,
        kind: InlineMessageKind::Info,
        segments: vec![text_segment("• Ran cargo check")],
    });
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Warning,
        segments: vec![text_segment("warning after tool")],
    });
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Error,
        segments: vec![text_segment("error after tool")],
    });
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![text_segment("assistant after tool")],
    });

    let mut viewer = ToolOutputViewerState::open(&session, 100, 20);
    let export = viewer.export_text();
    let ordered = [
        "user request",
        "assistant before tool",
        "reasoning",
        "• Ran cargo check",
        "complete stdout",
        "complete stderr",
        "warning after tool",
        "error after tool",
        "assistant after tool",
    ];
    let positions = ordered
        .iter()
        .map(|needle| export.find(needle).expect("conversation entry in export"))
        .collect::<Vec<_>>();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(
        viewer
            .messages
            .iter()
            .filter(|message| message.key == ReviewBlockKey::Tool(0))
            .count(),
        1
    );

    let rich_mode = viewer.render_mode();
    viewer.toggle_render_mode();
    assert_eq!(rich_mode, TranscriptRenderMode::Rich);
    assert_eq!(viewer.render_mode(), TranscriptRenderMode::Raw);
    assert_eq!(viewer.export_text(), export);
}

#[test]
fn whole_review_stops_before_following_anchored_pty_call() {
    let mut session = test_session();
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 1,
        lines: vec!["• Ran first".to_string(), "  └ first output".to_string()],
    });
    session.handle_command(InlineCommand::AppendCompactActivity(
        vtcode_commons::ui_protocol::CompactActivityMetadata {
            group_id: 1,
            command_count: 1,
            command: Some("first".to_string().into()),
            hidden_line_count: 1,
            suffix: None,
            review_anchor: Some(1),
            review_anchors: vec![1],
        },
    ));
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Pty,
        segments: vec![text_segment("• Ran second")],
    });
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 2,
        lines: vec!["• Ran second".to_string(), "  └ second output".to_string()],
    });

    let mut viewer = ToolOutputViewerState::open(&session, 100, 20);
    let export = viewer.export_text();
    assert!(export.find("first output").unwrap() < export.find("second output").unwrap());
    assert_eq!(export.matches("• Ran second").count(), 1);
}

#[test]
fn whole_conversation_export_keeps_follow_up_guidance() {
    let mut session = test_session();
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 7,
        lines: vec![
            "• Ran cargo check".to_string(),
            "  └ complete output".to_string(),
            "    Review the result before continuing.".to_string(),
        ],
    });
    session.handle_command(InlineCommand::AppendToolOutputLine {
        id: 7,
        kind: InlineMessageKind::Info,
        segments: vec![text_segment("• Ran cargo check")],
    });

    let mut viewer = ToolOutputViewerState::open(&session, 100, 20);
    assert!(viewer.export_text().contains("Review the result before continuing."));
}

#[test]
fn active_transcript_updates_refresh_without_reopening() {
    let mut session = test_session();
    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![text_segment("initial response")],
    });
    let mut viewer = ToolOutputViewerState::open(&session, 80, 10);
    assert!(viewer.export_text().contains("initial response"));

    session.handle_command(InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![text_segment("streamed continuation")],
    });
    viewer.refresh(&session, 80, 10);
    let export = viewer.export_text();
    assert!(export.contains("streamed continuation"));
}

#[test]
fn focused_review_jumps_to_the_requested_capture_in_a_group() {
    let mut session = test_session();
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 11,
        lines: vec!["• Ran first".to_string(), "  └ first output".to_string()],
    });
    session.handle_command(InlineCommand::AppendCompactActivity(
        vtcode_commons::ui_protocol::CompactActivityMetadata {
            group_id: 1,
            command_count: 1,
            command: Some("first".to_string().into()),
            hidden_line_count: 1,
            suffix: None,
            review_anchor: Some(11),
            review_anchors: vec![11],
        },
    ));
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 12,
        lines: vec!["• Ran second".to_string(), "  └ second output".to_string()],
    });
    session.handle_command(InlineCommand::ReplaceCompactActivity(
        vtcode_commons::ui_protocol::CompactActivityMetadata {
            group_id: 1,
            command_count: 2,
            command: None,
            hidden_line_count: 2,
            suffix: None,
            review_anchor: Some(11),
            review_anchors: vec![11, 12],
        },
    ));

    let viewer = ToolOutputViewerState::open_focused(&session, 80, 2, Some(12));

    assert_eq!(
        viewer.messages.iter().map(|message| message.key).collect::<Vec<_>>(),
        vec![ReviewBlockKey::Tool(11), ReviewBlockKey::Tool(12)]
    );
    assert_eq!(viewer.scroll_top, 2);
    assert_eq!(viewer.focus_target, None);
}

#[test]
fn compact_activity_hint_uses_the_primary_review_binding() {
    let session = test_session();
    let hint = compact_activity_hint_text(&session).expect("default review binding should have a hint");
    assert_eq!(hint, "Ctrl+T transcript · click to expand");
}

#[test]
fn compact_activity_hint_underlines_click_affordance() {
    let session = test_session();
    let metadata = vtcode_commons::ui_protocol::CompactActivityMetadata {
        group_id: 7,
        command_count: 2,
        command: None,
        hidden_line_count: 0,
        suffix: None,
        review_anchor: Some(7),
        review_anchors: vec![7],
    };
    let segments = compact_activity_segments(&session, &metadata);
    let text: String = segments.iter().map(|segment| segment.text.as_str()).collect();
    assert!(text.contains("2 commands"));
    assert!(text.contains("click to expand"));
    let action = segments
        .iter()
        .find(|segment| segment.text == "click to expand")
        .expect("click affordance");
    assert!(action.style.effects.contains(anstyle::Effects::UNDERLINE));
}

#[test]
fn compact_activity_hint_refreshes_when_review_binding_changes() {
    let mut session = test_session();
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 12,
        lines: vec!["• Ran printf hint".to_string(), "  └ output".to_string()],
    });
    session.handle_command(InlineCommand::AppendCompactActivity(
        vtcode_commons::ui_protocol::CompactActivityMetadata {
            group_id: 12,
            command_count: 1,
            command: Some("printf hint".to_string().into()),
            hidden_line_count: 1,
            suffix: None,
            review_anchor: Some(12),
            review_anchors: vec![12],
        },
    ));

    let initial = session.core.lines[0]
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    assert!(initial.contains("Ctrl+T"));

    let mut bindings = hashbrown::HashMap::new();
    bindings.insert("open_transcript_review".to_string(), vec!["ctrl+x".to_string()]);
    session.handle_command(InlineCommand::SetKeyBindings { bindings });

    let refreshed = session.core.lines[0]
        .segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    assert!(refreshed.contains("Ctrl+X"));
    assert!(!refreshed.contains("Ctrl+T"));
}

#[test]
fn transcript_review_controls_follow_appearance_configuration() {
    let appearance = AppearanceConfig {
        show_transcript_review_hints: false,
        show_transcript_review_shortcut_guide: false,
        show_transcript_review_close_button: false,
        ..AppearanceConfig::default()
    };
    let session = AppSession::new_with_logs(
        InlineTheme::default(),
        None,
        24,
        true,
        Some(appearance),
        Vec::new(),
        "Agent TUI".to_string(),
    );

    assert!(compact_activity_hint_text(&session).is_none());
    assert!(transcript_review_shortcut_hint(&session, false).is_none());
    assert!(transcript_review_shortcut_hint(&session, true).is_none());
    assert!(!session.core.transcript_review_close_button_visible());
    let metadata = vtcode_commons::ui_protocol::CompactActivityMetadata {
        group_id: 1,
        command_count: 1,
        command: Some("printf configured".to_string().into()),
        hidden_line_count: 1,
        suffix: None,
        review_anchor: Some(1),
        review_anchors: vec![1],
    };
    let segs = compact_activity_segments(&session, &metadata);
    // Single-command now tokenized: •, Ran, command tokens + hidden count.
    assert!(segs.len() > 1);
    let text: String = segs.iter().map(|s| s.text.as_str()).collect();
    assert!(text.contains("printf configured"));
    assert!(!text.contains("Ctrl+T"));
}

#[test]
fn compact_activity_hint_updates_when_appearance_is_reloaded() {
    let mut session = test_session();
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 48,
        lines: vec!["• Ran printf reload".to_string(), "  └ complete output".to_string()],
    });
    session.handle_command(InlineCommand::AppendCompactActivity(
        vtcode_commons::ui_protocol::CompactActivityMetadata {
            group_id: 48,
            command_count: 1,
            command: Some("printf reload".to_string().into()),
            hidden_line_count: 1,
            suffix: None,
            review_anchor: Some(48),
            review_anchors: vec![48],
        },
    ));
    // With shell syntax: •, Ran, command tokens, hidden count, plus 3 hint segments (separator, binding, rest).
    assert!(session.core.lines[0].segments.len() > 3);
    let hint_text: String = session.core.lines[0].segments.iter().map(|s| s.text.as_str()).collect();
    assert!(hint_text.contains("Ctrl+T"));

    let mut appearance = session.core.appearance.clone();
    appearance.show_transcript_review_hints = false;
    session.handle_command(InlineCommand::SetAppearance { appearance });

    // Without hint, still tokenized command segments remain.
    assert!(session.core.lines[0].segments.len() > 1);
    let plain_text: String = session.core.lines[0].segments.iter().map(|s| s.text.as_str()).collect();
    assert!(!plain_text.contains("click to expand"));
}

#[test]
fn repeated_command_captures_keep_their_identity_and_order() {
    let mut session = test_session();
    for (id, output) in [(1, "first capture"), (2, "second capture")] {
        session.handle_command(InlineCommand::RecordToolOutput {
            id,
            lines: vec![format!("capture block {id}"), format!("  └ {output}")],
        });
        session.handle_command(InlineCommand::AppendToolOutputLine {
            id,
            kind: InlineMessageKind::Info,
            segments: vec![text_segment("• Ran cargo check")],
        });
    }

    let viewer = ToolOutputViewerState::open(&session, 100, 20);
    let export = viewer.clone().export_text();
    assert!(
        export.find("first capture").expect("first capture") < export.find("second capture").expect("second capture")
    );
}

#[test]
fn unanchored_failed_capture_stays_before_following_compact_activity() {
    let mut session = test_session();
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 21,
        lines: vec![
            "• Ran failed command".to_string(),
            "    failed: command exited with status 1".to_string(),
        ],
    });
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 22,
        lines: vec!["• Ran successful command".to_string(), "  └ success output".to_string()],
    });
    session.handle_command(InlineCommand::AppendCompactActivity(
        vtcode_commons::ui_protocol::CompactActivityMetadata {
            group_id: 22,
            command_count: 1,
            command: Some("successful command".to_string().into()),
            hidden_line_count: 1,
            suffix: None,
            review_anchor: Some(22),
            review_anchors: vec![22],
        },
    ));

    let mut viewer = ToolOutputViewerState::open(&session, 100, 20);
    let export = viewer.export_text();
    assert!(
        export.find("failed: command exited").expect("failed capture")
            < export.find("success output").expect("successful capture")
    );
}

#[test]
fn wrapping_preserves_blank_output_lines() {
    assert_eq!(wrap_output_line("", 20), vec![String::new()]);
    assert_eq!(wrap_output_line("abcdef", 3), vec!["abc", "def"]);
}

#[test]
fn next_match_wraps_asymmetrically() {
    assert_eq!(next_match_index(None, 0, true), None);
    assert_eq!(next_match_index(Some(0), 0, false), None);
    assert_eq!(next_match_index(None, 1, true), Some(0));
    assert_eq!(next_match_index(Some(0), 1, true), Some(0));
    assert_eq!(next_match_index(Some(0), 1, false), Some(0));
    // Asymmetric pair: forward from last wraps to first, backward from first wraps to last.
    assert_eq!(next_match_index(Some(2), 3, true), Some(0));
    assert_eq!(next_match_index(Some(0), 3, false), Some(2));
    assert_eq!(next_match_index(Some(1), 3, true), Some(2));
    assert_eq!(next_match_index(Some(1), 3, false), Some(0));
}

#[test]
fn scroll_by_clamps_at_bounds() {
    let mut session = test_session();
    for _ in 0..20 {
        add_block(&mut session, &["output"]);
    }
    let mut viewer = ToolOutputViewerState::open(&session, 40, 10);
    viewer.scroll_to_top();
    viewer.scroll_by(-100, 10);
    assert_eq!(viewer.scroll_top, 0);
    let max = viewer.max_scroll(10);
    assert!(max > 0);
    viewer.scroll_by(10_000, 10);
    assert_eq!(viewer.scroll_top, max);
    viewer.scroll_by(-10_000, 10);
    assert_eq!(viewer.scroll_top, 0);
}

#[test]
fn streaming_append_extends_matches_incrementally() {
    let mut session = test_session();
    add_block(&mut session, &["alpha one"]);
    add_block(&mut session, &["beta"]);
    let mut viewer = ToolOutputViewerState::open(&session, 40, 10);
    viewer.search.query = "alpha".to_string();
    viewer.recompute_matches();
    assert_eq!(viewer.search.matches, vec![0]);
    assert_eq!(viewer.search.current_match, Some(0));

    // Asymmetric tail: matching line appended after a non-match.
    add_block(&mut session, &["alpha two"]);
    viewer.refresh(&session, 40, 10);
    assert_eq!(viewer.search.matches, vec![0, 2]);
    // Prefix current selection is preserved, not reset.
    assert_eq!(viewer.search.current_match, Some(0));
}

#[test]
fn refresh_without_changes_keeps_matches_untouched() {
    let mut session = test_session();
    add_block(&mut session, &["alpha one"]);
    let mut viewer = ToolOutputViewerState::open(&session, 40, 10);
    viewer.search.query = "alpha".to_string();
    viewer.recompute_matches();
    assert_eq!(viewer.search.matches, vec![0]);

    viewer.refresh(&session, 40, 10);
    assert_eq!(viewer.search.matches, vec![0]);
    assert_eq!(viewer.search.current_match, Some(0));
}

#[test]
fn shortcut_hint_switches_while_searching() {
    let session = test_session();
    let idle = transcript_review_shortcut_hint(&session, false).expect("idle hint");
    assert!(idle.contains("q/Esc close"));
    let searching = transcript_review_shortcut_hint(&session, true).expect("search hint");
    assert!(!searching.contains("q/Esc close"));
    assert!(searching.contains("Esc cancel"));
}

#[test]
fn incremental_matches_full_after_mid_edit_row_shift() {
    let mut session = test_session();
    add_block(&mut session, &["alpha one"]);
    add_block(&mut session, &["beta"]);
    add_block(&mut session, &["alpha tail"]);
    let mut viewer = ToolOutputViewerState::open(&session, 40, 10);
    viewer.search.query = "alpha".to_string();
    viewer.recompute_matches();
    assert_eq!(viewer.search.matches, vec![0, 2]);

    // Mid-list edit that shifts rows: expand the middle block to two lines,
    // as a rebuilt block after ReplaceLast would.
    viewer.messages[1].lowered_lines = None;
    viewer.messages[1].lines = vec!["beta".to_string(), "alpha inserted".to_string()];
    viewer.messages[1].rich_lines = vec![Line::raw("beta"), Line::raw("alpha inserted")];
    viewer.update_row_offsets();
    viewer.recompute_matches_after_refresh();

    // Independent oracle: full rescan over the same state takes a
    // different path (truncate nothing, scan from row 0).
    let mut oracle = viewer.clone();
    oracle.search.matches.clear();
    oracle.search.current_match = None;
    oracle.recompute_matches();
    assert_eq!(viewer.search.matches, oracle.search.matches);
    assert_eq!(viewer.search.matches, vec![0, 2, 3]);
}

#[test]
fn cancel_search_keeps_committed_matches() {
    let mut session = test_session();
    add_block(&mut session, &["alpha one"]);
    let mut viewer = ToolOutputViewerState::open(&session, 40, 10);
    viewer.search.query = "alpha".to_string();
    viewer.recompute_matches();
    assert_eq!(viewer.search.matches, vec![0]);

    viewer.start_search();
    viewer.insert_search_text("zzz");
    viewer.cancel_search();
    assert_eq!(viewer.search.query, "alpha");
    assert_eq!(viewer.search.matches, vec![0]);
    assert_eq!(viewer.search.current_match, Some(0));
}

#[test]
fn screen_reader_review_opening_and_live_enablement_preserve_search_and_export() {
    let mut session = test_session();
    session.handle_command(InlineCommand::RecordToolOutput {
        id: 73,
        lines: vec![
            "\x1b[31mfirst evidence long enough to wrap into many rows\x1b[0m".into(),
            "second evidence".into(),
            "tail".into(),
        ],
    });
    session.handle_command(InlineCommand::AppendToolOutputLine {
        id: 73,
        kind: InlineMessageKind::Tool,
        segments: vec![text_segment("captured evidence")],
    });
    session.open_tool_output_viewer(100, 2, None);
    let viewer = session.tool_output_viewer_state_mut().unwrap();
    assert_eq!(viewer.mode, TranscriptRenderMode::Rich);
    viewer.start_search();
    viewer.insert_search_text("evidence");
    viewer.commit_search(2);
    let export = viewer.export_text();
    assert!(export.contains("first evidence long enough to wrap into many rows"));
    assert!(export.contains("second evidence"));
    assert!(export.ends_with("tail"));
    assert!(!export.contains('\x1b'));

    let mut appearance = session.core.appearance.clone();
    appearance.screen_reader_mode = true;
    session.handle_command(InlineCommand::SetAppearance { appearance: appearance.clone() });
    let viewer = session.tool_output_viewer_state_mut().unwrap();
    assert_eq!(viewer.mode, TranscriptRenderMode::Raw);
    assert_eq!(viewer.search.matches, vec![0, 1]);
    assert_eq!(viewer.row_offsets, vec![0]);
    assert_eq!(viewer.total_lines, 3);
    assert!(viewer.scroll_top <= viewer.max_scroll(2));
    assert_eq!(viewer.export_text(), export);
    let scroll_top = viewer.scroll_top;
    viewer.set_render_mode(TranscriptRenderMode::Raw);
    assert_eq!(viewer.scroll_top, scroll_top);
    assert_eq!(viewer.search.matches, vec![0, 1]);
    viewer.toggle_render_mode();
    assert_eq!(viewer.mode, TranscriptRenderMode::Rich);
    session.handle_command(InlineCommand::SetAppearance { appearance: appearance.clone() });
    assert_eq!(
        session.tool_output_viewer_state().unwrap().mode,
        TranscriptRenderMode::Rich,
        "reapplying config preserves manual toggle"
    );
    appearance.screen_reader_mode = false;
    session.handle_command(InlineCommand::SetAppearance { appearance: appearance.clone() });
    assert_eq!(session.tool_output_viewer_state().unwrap().mode, TranscriptRenderMode::Rich);
    appearance.screen_reader_mode = true;
    session.handle_command(InlineCommand::SetAppearance { appearance });
    session.close_tool_output_viewer();
    for focus in [None, Some(73)] {
        if let Some(id) = focus {
            session.core.apply_transcript_width(100);
            session.core.apply_transcript_rows(2);
            session.handle_command(InlineCommand::FocusTranscriptReview { id });
        } else {
            session.open_tool_output_viewer(100, 2, None);
        }
        let viewer = session.tool_output_viewer_state_mut().unwrap();
        assert_eq!(viewer.mode, TranscriptRenderMode::Raw);
        assert_eq!(viewer.focus_target, None);
        assert_eq!(viewer.scroll_top, if focus.is_some() { 0 } else { 1 });
        assert!(viewer.scroll_top <= viewer.max_scroll(2));
        assert_eq!(viewer.export_text(), export);
        viewer.toggle_render_mode();
        viewer.toggle_render_mode();
        assert_eq!(viewer.mode, TranscriptRenderMode::Raw);
    }
}

#[test]
fn review_mode_setter_invalidates_wrapped_search_rows_and_clamps_cancel_scroll() {
    let mut viewer = ToolOutputViewerState {
        height: 1,
        messages: vec![CachedToolOutputBlock {
            lines: vec!["first target second target".into(), "tail".into()],
            rich_lines: vec![
                Line::raw("first"),
                Line::raw("target"),
                Line::raw("second"),
                Line::raw("target"),
                Line::raw("tail"),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    viewer.update_row_offsets();
    viewer.start_search();
    viewer.insert_search_text("target");
    viewer.commit_search(1);
    assert_eq!(viewer.search.matches, vec![1, 3]);
    viewer.scroll_to_bottom(1);
    viewer.start_search();
    viewer.set_render_mode(TranscriptRenderMode::Raw);
    assert_eq!(viewer.search.matches, vec![0]);
    assert_eq!(viewer.total_lines, 2);
    assert_eq!(viewer.scroll_top, 1);
    viewer.cancel_search();
    assert!(viewer.scroll_top <= 1);
    assert_eq!(viewer.current_match_line(), Some(0));
    viewer.toggle_render_mode();
    assert_eq!(viewer.search.matches, vec![1, 3]);
}
