//! Diagnostics are one transcript source each; expansion only changes presentation.
use std::sync::Arc;

use ratatui::text::{Line, Span};
use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity, redact_diagnostic};
use vtcode_commons::ui_protocol::{InlineMessageKind, InlineSegment, InlineTextStyle, convert_style};

use super::{Session, message::TranscriptLine};
use crate::tui::core_tui::style::ratatui_style_from_inline;
use crate::tui::ui::markdown::{
    CodeBlockWeight, MarkdownLine, MarkdownSegment, RenderMarkdownOptions, render_markdown_to_lines_with_options,
    theme_matched_highlight_config,
};
use crate::tui::ui::theme::ThemeStyles;

#[derive(Clone)]
pub struct DiagnosticPresentation {
    pub payload: Diagnostic,
    pub expanded: bool,
}

impl Session {
    pub(crate) fn upsert_diagnostic(&mut self, diagnostic: Diagnostic) {
        let diagnostic = redact_diagnostic(&diagnostic);
        let previous_max_offset = self.current_max_scroll_offset();
        let existing = self.lines.iter().position(|line| {
            line.diagnostic
                .as_ref()
                .is_some_and(|entry| entry.payload.correlation_id == diagnostic.correlation_id)
        });
        let kind = match diagnostic.severity {
            DiagnosticSeverity::Info => InlineMessageKind::Info,
            DiagnosticSeverity::Warning => InlineMessageKind::Warning,
            DiagnosticSeverity::Error => InlineMessageKind::Error,
        };
        self.clear_thinking_spinner_if_active(kind);
        let segments = vec![InlineSegment {
            text: diagnostic.summary.clone(),
            style: Arc::new(InlineTextStyle::default()),
        }];
        let index = if let Some(index) = existing {
            if self.lines[index]
                .diagnostic
                .as_ref()
                .is_some_and(|entry| entry.payload == diagnostic)
            {
                return;
            }
            let revision = self.next_revision();
            let line = &mut self.lines[index];
            line.kind = kind;
            line.segments = segments;
            line.revision = revision;
            index
        } else {
            self.push_line(kind, segments);
            self.lines.len().saturating_sub(1)
        };
        let expanded = self.lines[index].diagnostic.as_ref().is_some_and(|entry| entry.expanded);
        self.lines[index].diagnostic = Some(Box::new(DiagnosticPresentation { payload: diagnostic, expanded }));
        self.mark_line_dirty(index);
        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);
        self.request_transcript_clear();
    }

    pub(crate) fn toggle_diagnostic(&mut self, index: usize) -> bool {
        let previous_max_offset = self.current_max_scroll_offset();
        let revision = self.next_revision();
        let Some(line) = self.lines.get_mut(index) else {
            return false;
        };
        let Some(diagnostic) = line.diagnostic.as_mut() else {
            return false;
        };
        let kind = line.kind;
        if !details_disclosed_in_transcript(&diagnostic.payload.agent_details, kind) {
            return false;
        }
        diagnostic.expanded = !diagnostic.expanded;
        line.revision = revision;
        self.mark_line_dirty(index);
        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);
        self.request_transcript_clear();
        true
    }

    /// F8 toggles the latest diagnostic. It never emits a runtime input event.
    ///
    /// The latest incident with details owns the key: an incident whose payload
    /// the transcript does not disclose is skipped rather than retargeting an
    /// older one.
    pub(crate) fn toggle_latest_diagnostic(&mut self) -> bool {
        let index = self.lines.iter().rposition(|line| {
            line.diagnostic
                .as_ref()
                .is_some_and(|entry| !entry.payload.agent_details.trim().is_empty())
        });
        index.is_some_and(|index| self.toggle_diagnostic(index))
    }

    pub(crate) fn toggle_diagnostic_at_row(&mut self, width: u16, row: usize) -> bool {
        let mut start = 0;
        for index in 0..self.lines.len() {
            let rows = self.reflow_message_lines(index, width, true);
            let count = rows.len();
            if row >= start && row < start + count {
                // Only the disclosure affordance toggles. Clicking summary or
                // evidence text must preserve selection and link interaction.
                let is_affordance = rows.get(row - start).is_some_and(|line| {
                    line.line
                        .spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                        .trim_start()
                        .starts_with("Details (")
                });
                return is_affordance && self.toggle_diagnostic(index);
            }
            start += count;
        }
        false
    }

    pub(super) fn reflow_diagnostic(&self, index: usize, width: u16, review: bool) -> Vec<TranscriptLine> {
        let Some(line) = self.lines.get(index) else {
            return Vec::new();
        };
        let Some(entry) = line.diagnostic.as_ref() else {
            return Vec::new();
        };
        let payload = &entry.payload;
        // Transcript Review always shows the complete evidence body, so it
        // renders the expanded presentation even when the transcript collapses it.
        let expanded = entry.expanded || review;

        let styles = crate::theme::active_styles();
        let base_style = anstyle::Style::new().fg_color(self.text_fallback(line.kind));
        let render_options = RenderMarkdownOptions {
            table_max_width: (width > 0).then_some(usize::from(width)),
            ..Default::default()
        };

        let mut summary = payload.summary.clone();
        if !payload.actions.is_empty() {
            summary.push_str("\n\nAvailable actions: ");
            summary.push_str(
                &payload
                    .actions
                    .iter()
                    .map(|action| action.label.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        }
        let mut rendered = render_markdown_to_lines_with_options(&summary, base_style, &styles, None, render_options);

        if !payload.agent_details.is_empty() {
            // Transcript Review always retains the complete evidence; the
            // transcript discloses prose evidence and actionable evidence in
            // place, and leaves an informational payload to the review surface.
            if review || details_disclosed_in_transcript(&payload.agent_details, line.kind) {
                push_markdown_separator(&mut rendered);
                let affordance = if expanded {
                    "Details (click or F8 to collapse)"
                } else {
                    "Details (click or F8 to expand)"
                };
                rendered.push(MarkdownLine {
                    segments: vec![MarkdownSegment {
                        style: disclosure_affordance_style(&styles),
                        text: affordance.to_string(),
                        link_target: None,
                    }],
                    line_background: None,
                });
                if expanded {
                    push_markdown_separator(&mut rendered);
                    // The evidence body is secondary content: highlight the code
                    // so it stays scannable, and recede it so the summary keeps
                    // the eye.
                    let highlight_config = theme_matched_highlight_config(&self.appearance.syntax_highlighting);
                    let body_options = RenderMarkdownOptions {
                        code_block_weight: CodeBlockWeight::Muted,
                        ..render_options
                    };
                    rendered.extend(render_markdown_to_lines_with_options(
                        &details_source(&payload.agent_details),
                        base_style,
                        &styles,
                        Some(&highlight_config),
                        body_options,
                    ));
                }
            }
        }
        let mut output = Vec::new();
        for markdown_line in rendered {
            let spans = markdown_line
                .segments
                .into_iter()
                .map(|segment| {
                    Span::styled(
                        segment.text,
                        ratatui_style_from_inline(&convert_style(segment.style), self.text_fallback(line.kind)),
                    )
                })
                .collect::<Vec<_>>();
            let rendered_line = Line::from(spans);
            let rows = if width == 0 {
                vec![rendered_line]
            } else {
                self.wrap_line(rendered_line, usize::from(width))
            };
            output.extend(
                rows.into_iter().map(|line| {
                    super::reflow::transcript_line_with_detected_links(line, self.workspace_root.as_deref())
                }),
            );
        }
        output.push(TranscriptLine::default());
        output
    }
}

/// Keep one blank row between rendered markdown blocks.
fn push_markdown_separator(lines: &mut Vec<MarkdownLine>) {
    if lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(MarkdownLine::default());
    }
}

/// Share of the theme background mixed into the disclosure affordance.
const MUTED_AFFORDANCE_BLEND_RATIO: f32 = 0.35;

// The transcript classifies the payload on every reflow, so an oversized
// payload is classified by its shape instead of being parsed.
const MAX_STRUCTURED_DETAILS_BYTES: usize = 64 * 1024;

/// Whether the details payload is machine-readable structured data.
///
/// Diagnostic evidence is frequently a JSON payload rather than prose. Such a
/// payload is noise in the transcript: an informational incident keeps only its
/// summary, and the payload stays in Transcript Review, which always retains
/// the complete details. A warning or an error is actionable, so its evidence
/// stays in the transcript on the code surface.
/// A payload that opens an object or an array and actually parses as JSON is
/// machine-readable. Probing the payload's first line separately keeps a
/// trailing note (a persistence failure) from flipping the classification,
/// and rejects prose that merely begins with a bracket.
fn is_structured_details(details: &str) -> bool {
    let trimmed = details.trim_start();
    if !trimmed.starts_with('{') && !trimmed.starts_with('[') {
        return false;
    }
    if trimmed.len() > MAX_STRUCTURED_DETAILS_BYTES {
        // Too large to probe: the shape is the signal.
        return true;
    }
    if vtcode_commons::fs::try_parse_json_value(trimmed).is_some() {
        return true;
    }
    trimmed
        .split('\n')
        .next()
        .and_then(vtcode_commons::fs::try_parse_json_value)
        .is_some()
}

/// Whether the details payload is disclosed in the transcript.
///
/// Prose evidence is disclosed in place behind the disclosure affordance.
/// A machine-readable payload is disclosed only for an actionable incident: an
/// informational notice keeps its summary alone, because the payload is noise
/// there, and Transcript Review retains it in full.
fn details_disclosed_in_transcript(details: &str, kind: InlineMessageKind) -> bool {
    if details.trim().is_empty() {
        return false;
    }
    if !is_structured_details(details) {
        return true;
    }
    kind != InlineMessageKind::Info
}

/// Render the evidence body on the code surface when it is structured data.
///
/// A structured payload is a single machine-shaped line; on the code surface it
/// is scannable, syntax highlighted, and receded like every other code block.
/// Payloads that already carry markdown keep their own structure.
fn details_source(details: &str) -> String {
    let trimmed = details.trim();
    if !is_structured_details(details) {
        return details.to_string();
    }
    // The fence must be longer than any backtick run in the payload, or the
    // payload could close the fence early and lose content.
    let longest_run = trimmed.split(|c: char| c != '`').map(|run| run.len()).max().unwrap_or(0);
    let fence = "`".repeat(longest_run.max(2) + 1);
    format!("{fence}json\n{trimmed}\n{fence}")
}

/// Muted foreground for the disclosure affordance.
///
/// The affordance is chrome, not incident content: it uses the theme
/// foreground receded toward the background rather than the incident color,
/// because a severity color has no headroom to dim without losing legibility.
/// The recede stops at the theme's contrast floor, so the affordance stays
/// readable on light and dark backgrounds alike. The color is explicit rather
/// than a faint/dim attribute because several terminals attenuate that
/// attribute to near-invisible.
fn disclosure_affordance_style(theme_styles: &ThemeStyles) -> anstyle::Style {
    let receded = crate::theme::recede_toward_background(
        theme_styles.foreground,
        theme_styles.background,
        MUTED_AFFORDANCE_BLEND_RATIO,
        crate::theme::get_minimum_contrast(),
    );
    anstyle::Style::new().fg_color(Some(receded))
}

#[cfg(test)]
mod tests {
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
}
