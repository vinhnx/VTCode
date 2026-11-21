//! Diagnostics are one transcript source each; expansion only changes presentation.
use std::sync::Arc;

use ratatui::text::{Line, Span};
use vtcode_commons::diagnostics::{Diagnostic, DiagnosticSeverity, diagnostic_is_user_visible, redact_diagnostic};
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
        diagnostic.expanded.toggle();
        let expanded_now = diagnostic.expanded;
        line.revision = revision;
        let was_following = self.scroll_manager.offset() == 0;
        self.mark_line_dirty(index);
        self.invalidate_scroll_metrics();
        self.adjust_scroll_after_change(previous_max_offset);
        if expanded_now && was_following {
            self.pin_viewport_after_expansion(previous_max_offset);
        }
        self.request_transcript_clear();
        true
    }

    /// F8 toggles the latest diagnostic. It never emits a runtime input event.
    ///
    /// Whether a stored incident paints no main-transcript rows under the
    /// active communication style. Silent incidents stay stored (Transcript
    /// Review renders them with complete evidence) but are invisible to the
    /// main transcript, its hit-testing, and F8.
    fn diagnostic_is_hidden(&self, payload: &Diagnostic) -> bool {
        !diagnostic_is_user_visible(self.appearance.communication_style, &payload.code)
    }

    /// The latest incident with details owns the key: an incident whose payload
    /// the transcript does not disclose is skipped rather than retargeting an
    /// older one.
    pub(crate) fn toggle_latest_diagnostic(&mut self) -> bool {
        let style = self.appearance.communication_style;
        let index = self.lines.iter().rposition(|line| {
            line.diagnostic.as_ref().is_some_and(|entry| {
                !entry.payload.agent_details.trim().is_empty() && diagnostic_is_user_visible(style, &entry.payload.code)
            })
        });
        index.is_some_and(|index| self.toggle_diagnostic(index))
    }

    pub(crate) fn toggle_diagnostic_at_row(&mut self, width: u16, row: usize) -> bool {
        let target = {
            let cache = self.ensure_reflow_cache(width);
            // Hidden and grouped messages can share an offset. The last
            // matching start owns the row if it has any rendered content.
            cache
                .row_offsets
                .partition_point(|&start| start <= row)
                .checked_sub(1)
                .and_then(|index| {
                    let start = *cache.row_offsets.get(index)?;
                    let line = cache.messages.get(index)?.lines.get(row - start)?;
                    // Only the disclosure affordance toggles. Summary and evidence
                    // rows retain their normal selection and link behavior.
                    line.line
                        .spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                        .trim_start()
                        .starts_with("Details (")
                        .then_some(index)
                })
        };
        target.is_some_and(|index| self.toggle_diagnostic(index))
    }

    pub(super) fn reflow_diagnostic(&self, index: usize, width: u16, review: bool) -> Vec<TranscriptLine> {
        let Some(line) = self.lines.get(index) else {
            return Vec::new();
        };
        let Some(entry) = line.diagnostic.as_ref() else {
            return Vec::new();
        };
        let payload = &entry.payload;
        if !review && self.diagnostic_is_hidden(payload) {
            return Vec::new();
        }
        // Transcript Review always shows the complete evidence body, so it
        // renders the expanded presentation even when the transcript collapses it.
        let expanded = entry.expanded || review;

        let styles = crate::theme::active_styles();
        let base_style = anstyle::Style::new().fg_color(self.text_fallback(line.kind));
        let render_options = RenderMarkdownOptions {
            table_max_width: (width > 0).then_some(usize::from(width)),
            ..Default::default()
        };

        // Screen-reader mode forces the inline surface, where severity is
        // otherwise carried by the row kind and color. Spell it out in the
        // rendered text, matching the headless diagnostic path.
        let severity_label = if self.appearance.screen_reader_mode {
            match payload.severity {
                DiagnosticSeverity::Info => "[Info] ",
                DiagnosticSeverity::Warning => "[Warning] ",
                DiagnosticSeverity::Error => "[Error] ",
            }
        } else {
            ""
        };
        let mut summary = format!("{severity_label}{}", payload.summary);
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
mod tests;
