//! Diagnostics are one transcript source each; expansion only changes presentation.
use std::sync::Arc;

use ratatui::text::{Line, Span};
use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity, redact_diagnostic};
use vtcode_commons::ui_protocol::{InlineMessageKind, InlineSegment, InlineTextStyle, convert_style};

use super::{Session, message::TranscriptLine};
use crate::tui::core_tui::style::ratatui_style_from_inline;
use crate::tui::ui::markdown::{RenderMarkdownOptions, render_markdown_to_lines_with_options};

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
        if diagnostic.payload.agent_details.is_empty() {
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
    pub(crate) fn toggle_latest_diagnostic(&mut self) -> bool {
        let index = self.lines.iter().rposition(|line| {
            line.diagnostic
                .as_ref()
                .is_some_and(|entry| !entry.payload.agent_details.is_empty())
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
        let mut source = payload.summary.clone();
        if !payload.actions.is_empty() {
            source.push_str("\n\nAvailable actions: ");
            source.push_str(
                &payload
                    .actions
                    .iter()
                    .map(|action| action.label.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        }
        if !payload.agent_details.is_empty() {
            source.push_str(if entry.expanded || review {
                "\n\nDetails (click or F8 to collapse)\n\n"
            } else {
                "\n\nDetails (click or F8 to expand)"
            });
            if entry.expanded || review {
                source.push_str(&payload.agent_details);
            }
        }
        let styles = crate::theme::active_styles();
        let base_style = anstyle::Style::new().fg_color(self.text_fallback(line.kind));
        let rendered = render_markdown_to_lines_with_options(
            &source,
            base_style,
            &styles,
            None,
            RenderMarkdownOptions {
                table_max_width: (width > 0).then_some(usize::from(width)),
                ..Default::default()
            },
        );
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

#[cfg(test)]
mod tests {
    use super::*;
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
