use super::*;
use crate::tui::core_tui::style::ratatui_color_from_ansi;
use crate::tui::core_tui::types::{InlineCommand, InlineTheme};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn diagnostic() -> Diagnostic {
    Diagnostic { severity: DiagnosticSeverity::Error, code: "planning.awaiting_input".into(), summary: "Planning needs your input. Research and draft are saved.".into(), agent_details: "Provider `ollama` rejected **synthesis** for `README.md`.\n\n```text\nattempts=3\n```\n<proposed_plan>\nNo raw wrappers\n</proposed_plan>".into(), correlation_id: "plan-17".into(), actions: vec![] }
}
fn text(lines: Vec<TranscriptLine>) -> String {
    lines
        .into_iter()
        .map(|line| {
            line.line
                .spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn row_text(line: &TranscriptLine) -> String {
    line.line.spans.iter().map(|span| span.content.as_ref()).collect::<String>()
}

fn affordance_row(lines: &[TranscriptLine]) -> Option<usize> {
    lines
        .iter()
        .position(|line| row_text(line).trim_start().starts_with("Details ("))
}

#[test]
fn diagnostic_toggle_preserves_payload_across_expansion() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let payload = diagnostic();
    session.upsert_diagnostic(payload.clone());

    // Expand through the mouse path, collapse through F8: both are
    // presentation-only and must leave the persisted payload untouched.
    let affordance =
        affordance_row(&session.reflow_message_lines(0, 100, true)).expect("disclosure affordance is rendered");
    assert!(session.toggle_diagnostic_at_row(100, affordance));
    assert!(session.toggle_latest_diagnostic());
    let stored = session.lines[0].diagnostic.as_ref().expect("diagnostic entry").payload.clone();
    assert_eq!(stored, payload, "toggling must not mutate the incident payload");
    assert!(stored.agent_details.contains("attempts=3"), "recovery evidence stays in the payload");
}

#[test]
fn silent_routine_incident_stays_stored_but_paints_no_main_transcript_rows() {
    use vtcode_commons::ui_protocol::CommunicationStyle;
    let mut session = Session::new(InlineTheme::default(), None, 30);
    assert_eq!(session.appearance.communication_style, CommunicationStyle::Balanced);
    let mut payload = diagnostic();
    payload.code = "tool.execution_failed".into();
    payload.severity = DiagnosticSeverity::Warning;
    payload.summary = "Tool 'task_tracker' failed.".into();
    session.upsert_diagnostic(payload.clone());

    assert!(session.lines[0].diagnostic.is_some(), "silent incidents stay stored");
    assert!(
        session.reflow_message_lines(0, 100, false).is_empty(),
        "silent incidents paint no main-transcript rows"
    );
    let review = text(session.reflow_message_lines_for_review(0, 100));
    assert!(review.contains("Tool 'task_tracker' failed."), "Review keeps the summary: {review}");
    assert!(review.contains("attempts=3"), "Review keeps the complete evidence: {review}");
    assert!(!session.toggle_latest_diagnostic(), "F8 must skip silent incidents");
    assert!(!session.toggle_diagnostic_at_row(100, 0), "hit-testing must skip silent incidents");
}

#[test]
fn command_and_policy_incidents_paint_while_routine_stays_silent() {
    use vtcode_commons::ui_protocol::CommunicationStyle;
    let mut session = Session::new(InlineTheme::default(), None, 30);
    for (code, summary, paints) in [
        ("tool.command_failed", "Tool 'exec_command' failed.", true),
        ("tool.policy_denied", "Tool 'web_fetch' blocked by policy.", true),
        ("tool.something_new", "Some future incident.", true),
        ("tool.execution_failed", "Tool 'task_tracker' failed.", false),
        ("provider.retrying", "Retrying provider request.", false),
        ("provider.retry_stopped", "Provider retries stopped.", true),
    ] {
        let mut payload = diagnostic();
        payload.code = code.into();
        payload.summary = summary.into();
        payload.correlation_id = format!("incident-{code}");
        session.upsert_diagnostic(payload);
        let index = session.lines.len() - 1;
        assert_eq!(
            !session.reflow_message_lines(index, 100, false).is_empty(),
            paints,
            "{code} paints={paints} under Balanced"
        );
        assert!(
            !session.reflow_message_lines_for_review(index, 100).is_empty(),
            "{code} always stays in Transcript Review"
        );
    }
    // Quiet additionally collapses user-direct command failures.
    session.appearance.communication_style = CommunicationStyle::Quiet;
    let command = session
        .lines
        .iter()
        .position(|line| {
            line.diagnostic
                .as_ref()
                .is_some_and(|entry| entry.payload.code == "tool.command_failed")
        })
        .expect("command incident stored");
    assert!(session.reflow_message_lines(command, 100, false).is_empty());
    // Verbose restores every row.
    session.appearance.communication_style = CommunicationStyle::Verbose;
    for index in 0..session.lines.len() {
        assert!(!session.reflow_message_lines(index, 100, false).is_empty(), "verbose paints every stored incident");
    }
}

#[test]
fn diagnostic_details_disclosure_uses_a_muted_affordance_and_muted_highlighted_code() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let mut payload = diagnostic();
    payload.agent_details = "```rust\nfn main() { let answer = 42; }\n```\n".into();
    session.upsert_diagnostic(payload);

    let collapsed = session.reflow_message_lines(0, 100, true);
    let affordance = affordance_row(&collapsed).expect("disclosure affordance is rendered");
    assert!(session.toggle_diagnostic_at_row(100, affordance), "the affordance toggles the details");

    let expanded = session.reflow_message_lines(0, 100, true);
    let theme_styles = crate::theme::active_styles();
    let error_color = session.text_fallback(InlineMessageKind::Error).expect("error color");

    let affordance_line = expanded
        .iter()
        .find(|line| row_text(line).trim_start().starts_with("Details ("))
        .expect("disclosure affordance is rendered");
    assert_eq!(affordance_line.line.spans.len(), 1, "the affordance is one hint row");
    let expected_affordance = crate::theme::recede_toward_background(
        theme_styles.foreground,
        theme_styles.background,
        MUTED_AFFORDANCE_BLEND_RATIO,
        crate::theme::get_minimum_contrast(),
    );
    assert_eq!(
        Some(ratatui_color_from_ansi(expected_affordance)),
        affordance_line.line.spans[0].style.fg,
        "the affordance must recede toward the background"
    );
    assert_ne!(
        Some(ratatui_color_from_ansi(error_color)),
        affordance_line.line.spans[0].style.fg,
        "the affordance must not keep the full-intensity incident color"
    );
    assert_ne!(
        Some(ratatui_color_from_ansi(theme_styles.foreground)),
        affordance_line.line.spans[0].style.fg,
        "the affordance must be dimmer than the summary foreground"
    );

    let code_line = expanded
        .iter()
        .find(|line| row_text(line).contains("fn main()"))
        .expect("details code is rendered");
    let code_colors = code_line
        .line
        .spans
        .iter()
        .skip(1)
        .filter_map(|span| span.style.fg)
        .collect::<std::collections::HashSet<_>>();
    assert!(code_colors.len() >= 2, "the details code must stay syntax highlighted, got {code_colors:?}");
    assert!(
        !code_colors.contains(&ratatui_color_from_ansi(error_color)),
        "the details code must not inherit the severity color, got {code_colors:?}"
    );
}

#[test]
fn diagnostic_structured_details_stay_out_of_an_informational_transcript() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let mut payload = diagnostic();
    payload.severity = DiagnosticSeverity::Info;
    payload.agent_details = "{\"phase\":\"ready:8_registered\",\"error\":null}".into();
    session.upsert_diagnostic(payload);

    let transcript = text(session.reflow_message_lines(0, 100, true));
    assert!(transcript.contains("Planning"));
    assert!(
        !transcript.contains("ready:8_registered"),
        "an informational payload is noise in the transcript: {transcript}"
    );
    assert!(!transcript.contains("Details ("), "a hidden payload has no disclosure affordance: {transcript}");
    assert!(!session.toggle_diagnostic_at_row(100, 0), "a hidden payload cannot be expanded in the transcript");
    assert!(!session.toggle_latest_diagnostic(), "F8 must not expand a hidden payload");

    let review = session.reflow_message_lines_for_review(0, 100);
    let code_line = review
        .iter()
        .find(|line| row_text(line).contains("ready:8_registered"))
        .expect("the structured payload is retained in review");
    // A structured payload belongs to the code surface: it carries the
    // gutter that marks a code block.
    assert!(row_text(code_line).starts_with("  1  "), "expected a code gutter, got {:?}", row_text(code_line));
    let code_colors = code_line
        .line
        .spans
        .iter()
        .skip(1)
        .filter_map(|span| span.style.fg)
        .collect::<std::collections::HashSet<_>>();
    assert!(code_colors.len() >= 2, "the structured payload must be syntax highlighted, got {code_colors:?}");
}

#[test]
fn diagnostic_actionable_structured_details_stay_disclosed_in_the_transcript() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let mut payload = diagnostic();
    payload.severity = DiagnosticSeverity::Warning;
    // A payload a producer appended to after the JSON still counts as
    // machine-readable evidence.
    payload.agent_details =
        "{\"phase\":\"initial_catalog\",\"error\":\"connection refused\"}\nDiagnostic persistence failed.".into();
    session.upsert_diagnostic(payload);

    let collapsed = text(session.reflow_message_lines(0, 100, true));
    assert!(collapsed.contains("Details ("), "an actionable incident keeps its disclosure control: {collapsed}");

    let affordance = affordance_row(&session.reflow_message_lines(0, 100, true)).expect("affordance is rendered");
    assert!(session.toggle_diagnostic_at_row(100, affordance), "the affordance toggles the details");
    let expanded = text(session.reflow_message_lines(0, 100, true));
    assert!(
        expanded.contains("connection refused"),
        "an actionable payload keeps its evidence in the transcript: {expanded}"
    );
}

#[test]
fn diagnostic_informational_prose_with_bracket_prefix_stays_disclosed() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let mut payload = diagnostic();
    payload.severity = DiagnosticSeverity::Info;
    // Prose that begins with a bracket is not a payload: it must stay
    // disclosed even though the shape test alone would hide it.
    payload.agent_details = "[mcp] Catalog refresh used 2 servers and 8 tools.".into();
    session.upsert_diagnostic(payload);

    let collapsed = text(session.reflow_message_lines(0, 100, true));
    assert!(collapsed.contains("Details ("), "prose evidence offers a disclosure control: {collapsed}");
    let affordance = affordance_row(&session.reflow_message_lines(0, 100, true)).expect("affordance is rendered");
    assert!(session.toggle_diagnostic_at_row(100, affordance));
    let expanded = text(session.reflow_message_lines(0, 100, true));
    assert!(
        expanded.contains("Catalog refresh used 2 servers"),
        "prose evidence is disclosed for an informational incident: {expanded}"
    );
}

#[test]
fn diagnostic_f8_targets_the_latest_incident_without_retargeting_an_older_one() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let mut prose = diagnostic();
    prose.correlation_id = "plan-1".into();
    session.upsert_diagnostic(prose);
    let mut hidden = diagnostic();
    hidden.correlation_id = "plan-2".into();
    hidden.severity = DiagnosticSeverity::Info;
    hidden.agent_details = "{\"phase\":\"ready\"}".into();
    session.upsert_diagnostic(hidden);

    assert!(!session.toggle_latest_diagnostic(), "the latest incident owns the key");
    assert!(
        !session.lines[0].diagnostic.as_ref().expect("prose diagnostic").expanded,
        "an older incident must not be expanded instead"
    );
}

#[test]
fn diagnostic_code_highlighting_honors_the_user_opt_out() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    let mut payload = diagnostic();
    payload.agent_details = "```rust\nfn main() {}\n```\n".into();
    session.upsert_diagnostic(payload);
    let affordance =
        affordance_row(&session.reflow_message_lines(0, 100, true)).expect("disclosure affordance is rendered");
    assert!(session.toggle_diagnostic_at_row(100, affordance));

    let highlighted = session.reflow_message_lines(0, 100, true);
    let code_line = highlighted
        .iter()
        .find(|line| row_text(line).contains("fn main()"))
        .expect("details code is rendered");
    let highlighted_colors = code_line
        .line
        .spans
        .iter()
        .skip(1)
        .filter_map(|span| span.style.fg)
        .collect::<std::collections::HashSet<_>>();

    session.appearance.syntax_highlighting.enabled = false;
    let plain = session.reflow_message_lines(0, 100, true);
    let code_line = plain
        .iter()
        .find(|line| row_text(line).contains("fn main()"))
        .expect("details code is rendered");
    let plain_colors = code_line
        .line
        .spans
        .iter()
        .skip(1)
        .filter_map(|span| span.style.fg)
        .collect::<std::collections::HashSet<_>>();
    assert!(
        plain_colors.len() < highlighted_colors.len(),
        "disabling syntax highlighting must collapse the token colors: {plain_colors:?} vs {highlighted_colors:?}"
    );
}
#[test]
fn diagnostic_click_maps_past_wrapped_text_and_zero_row_incidents() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    session.push_line(
        InlineMessageKind::Agent,
        vec![InlineSegment {
            text: "An ordinary transcript line that wraps into several rows at the narrow width.".into(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    );
    let mut silent = diagnostic();
    silent.code = "tool.execution_failed".into();
    silent.correlation_id = "silent-before-disclosure".into();
    session.upsert_diagnostic(silent);
    session.upsert_diagnostic(diagnostic());

    for width in [24, 100] {
        let preceding_rows = session.reflow_message_lines(0, width, true).len();
        assert!(session.reflow_message_lines(1, width, true).is_empty());
        let visible = session.reflow_message_lines(2, width, true);
        let disclosure = preceding_rows + affordance_row(&visible).expect("visible incident has a disclosure");

        assert!(!session.toggle_diagnostic_at_row(width, preceding_rows - 1));
        assert!(!session.toggle_diagnostic_at_row(width, preceding_rows));
        assert!(!session.toggle_diagnostic_at_row(width, preceding_rows + visible.len()));
        assert!(session.toggle_diagnostic_at_row(width, disclosure));
        assert!(!session.lines[1].diagnostic.as_ref().unwrap().expanded);
        assert!(session.lines[2].diagnostic.as_ref().unwrap().expanded);
        assert!(session.toggle_diagnostic_at_row(width, disclosure));
    }
}

#[test]
fn diagnostic_summary_details_resize_review_and_dedup() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    session.handle_command(InlineCommand::Diagnostic(Box::new(diagnostic())));
    for width in [24, 100] {
        let collapsed = text(session.reflow_message_lines(0, width, true));
        assert!(collapsed.contains("Planning"));
        assert!(collapsed.contains("Details"));
        assert!(!collapsed.contains("attempts=3"));
        let review = text(session.reflow_message_lines_for_review(0, width));
        assert!(review.contains("attempts=3"));
        assert!(!review.contains("<proposed_plan>"));
    }
    assert!(session.process_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE)).is_none());
    assert!(text(session.reflow_message_lines(0, 100, true)).contains("attempts=3"));
    assert!(!session.toggle_diagnostic_at_row(100, 0), "summary selection must not collapse details");
    let affordance = session
        .reflow_message_lines(0, 100, true)
        .iter()
        .position(|line| {
            line.line
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("Details (")
        })
        .unwrap();
    assert!(session.toggle_diagnostic_at_row(100, affordance));
    assert!(!text(session.reflow_message_lines(0, 100, true)).contains("attempts=3"));
    let mut updated = diagnostic();
    updated.agent_details = "attempts=4".into();
    session.upsert_diagnostic(updated);
    assert_eq!(session.lines.len(), 1);
    assert!(text(session.reflow_message_lines_for_review(0, 100)).contains("attempts=4"));
}
#[test]
fn screen_reader_mode_labels_inline_diagnostic_severity() {
    for (severity, label) in [
        (DiagnosticSeverity::Info, "[Info]"),
        (DiagnosticSeverity::Warning, "[Warning]"),
        (DiagnosticSeverity::Error, "[Error]"),
    ] {
        let mut session = Session::new(InlineTheme::default(), None, 100);
        session.appearance.screen_reader_mode = true;
        let mut item = diagnostic();
        item.severity = severity;
        session.upsert_diagnostic(item);
        let rendered = text(session.reflow_message_lines(0, 100, true));
        assert!(rendered.contains(label), "screen reader mode must spell out severity: {rendered}");
    }

    let mut plain = Session::new(InlineTheme::default(), None, 100);
    plain.upsert_diagnostic(diagnostic());
    let rendered = text(plain.reflow_message_lines(0, 100, true));
    assert!(!rendered.contains("[Error]"), "severity label is screen-reader-only");
}

#[test]
fn diagnostic_mouse_render_resize_and_screen_reader_keep_summary_visible() {
    use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use ratatui::{Terminal, backend::TestBackend};
    let mut session = Session::new(InlineTheme::default(), None, 30);
    session.appearance.screen_reader_mode = true;
    session.upsert_diagnostic(diagnostic());
    for width in [24, 100] {
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|frame| session.render(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let row = (0..buffer.area.height)
            .find(|row| {
                (0..buffer.area.width)
                    .map(|col| buffer[(col, *row)].symbol())
                    .collect::<String>()
                    .contains("Details")
            })
            .expect("diagnostic affordance is visible");
        let area = session.transcript_area().unwrap();
        assert!(session.handle_transcript_click(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: area.x + 1,
            row,
            modifiers: KeyModifiers::NONE
        }));
        assert!(session.lines[0].diagnostic.as_ref().unwrap().expanded);
        terminal.draw(|frame| session.render(frame)).unwrap();
        let rendered = text(session.reflow_message_lines(0, width, true));
        assert!(rendered.contains("attempts=3"));
        assert!(!rendered.contains("**"));
        assert!(!rendered.contains("<proposed_plan>"));
        assert!(session.toggle_latest_diagnostic());
    }
    assert_eq!(session.lines.len(), 1, "expansion never appends transcript sources");
}

#[test]
fn diagnostic_all_severities_keep_summary_and_redact_hidden_details() {
    let mut session = Session::new(InlineTheme::default(), None, 30);
    for (index, severity) in [
        DiagnosticSeverity::Info,
        DiagnosticSeverity::Warning,
        DiagnosticSeverity::Error,
    ]
    .into_iter()
    .enumerate()
    {
        let mut payload = diagnostic();
        payload.severity = severity;
        payload.correlation_id = format!("request-{index}");
        payload.summary = format!("Visible summary {index}");
        payload.agent_details = "password=hidden-diagnostic-secret".into();
        session.upsert_diagnostic(payload);
        assert!(text(session.reflow_message_lines(index, 100, true)).contains(&format!("Visible summary {index}")));
        let review = text(session.reflow_message_lines_for_review(index, 100));
        assert!(!review.contains("hidden-diagnostic-secret"));
        assert!(review.contains("REDACTED"));
    }
    assert_eq!(session.lines.len(), 3);
}
