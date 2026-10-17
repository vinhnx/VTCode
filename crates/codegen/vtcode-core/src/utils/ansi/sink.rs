//! Inline-sink markdown writer and payload helpers.

use super::*;

use crate::utils::transcript;

use crate::config::loader::SyntaxHighlightingConfig;
use crate::ui::markdown::{
    MarkdownLine, MarkdownSegment, RenderMarkdownOptions, render_markdown_to_lines_with_options,
};
use crate::ui::theme;
use crate::ui::tui::{
    InlineHandle, InlineListItem, InlineListSearchConfig, InlineListSelection, InlineMessageKind, InlineSegment,
    InlineTextStyle, SecurePromptConfig, convert_style as convert_to_inline_style,
};
#[cfg(feature = "tui")]
use ansi_to_tui::IntoText;
use anstyle::{Ansi256Color, AnsiColor, Color as AnsiColorEnum, Effects, RgbColor, Style};
use anyhow::Result;
#[cfg(feature = "tui")]
use ratatui::style::{Color as RatColor, Modifier as RatModifier, Style as RatatuiStyle};
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;
use vtcode_commons::diff_paths::looks_like_diff_content;
use vtcode_commons::ui_protocol::ToolOutputId;

pub(super) fn contains_markdown_fence(text: &str) -> bool {
    text.contains("```") || text.contains("~~~")
}

pub(super) fn looks_like_diff(text: &str) -> bool {
    looks_like_diff_content(text)
}

pub(super) const INLINE_JSON_COLLAPSE_BYTES: usize = 50_000;
pub(super) const INLINE_JSON_COLLAPSE_LINES: usize = 200;

pub(super) struct LargeJsonPayload<'a> {
    text: &'a str,
    line_count: usize,
}

pub(super) struct InlineSink {
    pub(super) handle: InlineHandle,
    pub(super) highlight_config: SyntaxHighlightingConfig,
    pub(super) table_max_width: Option<usize>,
    pub(super) table_max_width_override: Option<usize>,
}

impl InlineSink {
    pub(super) fn table_content_width(&self, kind: InlineMessageKind, indent: &str) -> Option<usize> {
        self.table_max_width.map(|terminal_width| {
            terminal_width
                .saturating_sub(UnicodeWidthStr::width(indent))
                .saturating_sub(transcript_table_frame_width(kind, self.handle.agent_label_frame_width()))
        })
    }

    pub(super) fn should_record_transcript(kind: InlineMessageKind) -> bool {
        kind != InlineMessageKind::Pty
    }

    pub(super) fn count_lines(text: &str) -> usize {
        if text.is_empty() {
            0
        } else {
            text.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1
        }
    }

    pub(super) fn unwrap_single_fenced_block(text: &str) -> Option<&str> {
        let trimmed = text.trim_end();
        if !trimmed.starts_with("```") || !trimmed.ends_with("```") {
            return None;
        }

        let first_newline = trimmed.find('\n')?;
        let last_fence = trimmed.rfind("\n```")?;
        if last_fence <= first_newline {
            return None;
        }

        Some(&trimmed[first_newline + 1..last_fence])
    }

    pub(super) fn detect_large_json_payload<'a>(
        kind: InlineMessageKind,
        text: &'a str,
    ) -> Option<LargeJsonPayload<'a>> {
        if !matches!(kind, InlineMessageKind::Tool | InlineMessageKind::Pty) {
            return None;
        }

        let candidate = Self::unwrap_single_fenced_block(text).unwrap_or(text);
        let trimmed = candidate.trim();
        if trimmed.is_empty() {
            return None;
        }

        if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
            return None;
        }
        if !(trimmed.ends_with('}') || trimmed.ends_with(']')) {
            return None;
        }

        let line_count = Self::count_lines(candidate);
        if candidate.len() < INLINE_JSON_COLLAPSE_BYTES && line_count < INLINE_JSON_COLLAPSE_LINES {
            return None;
        }

        Some(LargeJsonPayload { text: candidate, line_count })
    }

    pub(super) fn indent_multiline(text: &str, indent: &str) -> String {
        if indent.is_empty() {
            return text.to_string();
        }

        let mut out = String::with_capacity(text.len() + indent.len() * 4);
        for (idx, line) in text.split('\n').enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            out.push_str(indent);
            out.push_str(line);
        }
        out
    }

    pub(super) fn emit_large_json_payload(
        &mut self,
        payload: LargeJsonPayload<'_>,
        indent: &str,
        kind: InlineMessageKind,
        record_transcript: bool,
    ) -> Result<()> {
        let full_text = if !indent.is_empty() {
            Self::indent_multiline(payload.text, indent)
        } else {
            payload.text.to_string()
        };
        if record_transcript {
            transcript::append(&full_text);
        }
        self.handle.append_pasted_message(kind, full_text, payload.line_count);
        Ok(())
    }
    #[cfg(feature = "tui")]
    pub(super) fn ansi_from_ratatui_color(color: RatColor) -> Option<AnsiColorEnum> {
        match color {
            RatColor::Reset => None,
            RatColor::Black => Some(AnsiColorEnum::Ansi(AnsiColor::Black)),
            RatColor::Red => Some(AnsiColorEnum::Ansi(AnsiColor::Red)),
            RatColor::Green => Some(AnsiColorEnum::Ansi(AnsiColor::Green)),
            RatColor::Yellow => Some(AnsiColorEnum::Ansi(AnsiColor::Yellow)),
            RatColor::Blue => Some(AnsiColorEnum::Ansi(AnsiColor::Blue)),
            RatColor::Magenta => Some(AnsiColorEnum::Ansi(AnsiColor::Magenta)),
            RatColor::Cyan => Some(AnsiColorEnum::Ansi(AnsiColor::Cyan)),
            RatColor::Gray => Some(AnsiColorEnum::Rgb(RgbColor(0x88, 0x88, 0x88))),
            RatColor::DarkGray => Some(AnsiColorEnum::Rgb(RgbColor(0x66, 0x66, 0x66))),
            RatColor::LightRed => Some(AnsiColorEnum::Ansi(AnsiColor::Red)),
            RatColor::LightGreen => Some(AnsiColorEnum::Ansi(AnsiColor::Green)),
            RatColor::LightYellow => Some(AnsiColorEnum::Ansi(AnsiColor::Yellow)),
            RatColor::LightBlue => Some(AnsiColorEnum::Ansi(AnsiColor::Blue)),
            RatColor::LightMagenta => Some(AnsiColorEnum::Ansi(AnsiColor::Magenta)),
            RatColor::LightCyan => Some(AnsiColorEnum::Ansi(AnsiColor::Cyan)),
            RatColor::White => Some(AnsiColorEnum::Ansi(AnsiColor::White)),
            RatColor::Rgb(r, g, b) => Some(AnsiColorEnum::Rgb(RgbColor(r, g, b))),
            RatColor::Indexed(value) => Some(AnsiColorEnum::Ansi256(Ansi256Color(value))),
        }
    }

    #[cfg(feature = "tui")]
    pub(super) fn inline_style_from_ratatui(&self, style: RatatuiStyle, fallback: &InlineTextStyle) -> InlineTextStyle {
        let mut resolved = fallback.clone();
        // Keep transcript segments theme-dynamic by default. Only persist a
        // foreground color when ANSI parsing produced a color different from the
        // logical fallback for this message kind.
        resolved.color = None;
        if let Some(color) = style.fg.and_then(Self::ansi_from_ratatui_color)
            && Some(color) != fallback.color
        {
            resolved.color = Some(color);
        }
        // Explicit backgrounds (e.g. diff add/del/hunk tints, already chosen
        // for the detected theme at render time) must survive: without them
        // ANSI-rendered diff rows lose their tint and render half-painted.
        if let Some(bg) = style.bg.and_then(Self::ansi_from_ratatui_color) {
            resolved.bg_color = Some(bg);
        }

        let added = style.add_modifier;

        if added.contains(RatModifier::BOLD) {
            resolved.effects |= Effects::BOLD;
        }

        if added.contains(RatModifier::ITALIC) {
            resolved.effects |= Effects::ITALIC;
        }

        if added.contains(RatModifier::UNDERLINED) {
            resolved.effects |= Effects::UNDERLINE;
        }

        if added.contains(RatModifier::DIM) {
            resolved.effects |= Effects::DIMMED;
        }

        resolved
    }

    #[cfg(test)]
    pub(super) fn prepare_markdown_lines(
        &self,
        text: &str,
        indent: &str,
        base_style: Style,
        preserve_blank_lines: bool,
        preserve_code_indentation: bool,
    ) -> (Vec<Vec<InlineSegment>>, Vec<String>, bool) {
        self.prepare_markdown_lines_with_table_width(
            text,
            indent,
            base_style,
            preserve_blank_lines,
            preserve_code_indentation,
            self.table_max_width,
        )
    }

    pub(super) fn prepare_markdown_lines_with_table_width(
        &self,
        text: &str,
        indent: &str,
        base_style: Style,
        preserve_blank_lines: bool,
        preserve_code_indentation: bool,
        table_max_width: Option<usize>,
    ) -> (Vec<Vec<InlineSegment>>, Vec<String>, bool) {
        let fallback = self.resolve_fallback_style(base_style);
        let fallback_arc = Arc::new(fallback.clone());
        let theme_styles = theme::active_styles();
        let highlight_cfg = self.highlight_config.enabled.then_some(&self.highlight_config);
        let mut rendered = render_markdown_to_lines_with_options(
            text,
            base_style,
            &theme_styles,
            highlight_cfg,
            RenderMarkdownOptions {
                preserve_code_indentation,
                disable_code_block_table_reparse: false,
                table_max_width,
            },
        );
        if preserve_blank_lines {
            let mut cleaned = Vec::with_capacity(rendered.len());
            let mut last_blank = false;
            for line in rendered {
                let is_blank = line.is_empty();
                if is_blank {
                    if last_blank {
                        continue;
                    }
                    last_blank = true;
                } else {
                    last_blank = false;
                }
                cleaned.push(line);
            }
            rendered = cleaned;
        } else {
            // TUI space is constrained; drop blank lines to keep transcripts compact.
            rendered.retain(|line| !line.is_empty());
        }
        if rendered.is_empty() {
            rendered.push(MarkdownLine::default());
        }

        let mut prepared = Vec::with_capacity(rendered.len());
        let mut plain = Vec::with_capacity(rendered.len());
        let available_width = table_max_width.map(|width| width.saturating_sub(UnicodeWidthStr::width(indent)));

        for line in rendered {
            // Pre-allocate segments and plain text with estimated capacity
            let mut segments = Vec::with_capacity(line.segments.len());
            let mut plain_line = String::with_capacity(120);

            let has_content = line.segments.iter().any(|segment| !segment.text.trim().is_empty());

            if !indent.is_empty() && has_content {
                segments.push(InlineSegment {
                    text: indent.to_string(),
                    style: Arc::clone(&fallback_arc),
                });
                plain_line.push_str(indent);
            }

            for segment in &line.segments {
                if segment.text.is_empty() {
                    continue;
                }
                let mut converted = convert_to_inline_style(segment.style);
                // Plain file-like markdown tokens are styled as underlined during markdown parsing.
                // In inline UI, actual clickability is decided later from resolved transcript links.
                // Strip local-link underlines here to avoid showing non-clickable path text as links.
                if segment
                    .link_target
                    .as_deref()
                    .is_some_and(should_strip_inline_local_link_underline)
                {
                    converted.effects = converted.effects.remove(Effects::UNDERLINE);
                }
                let mut inline_style = fallback.clone();
                inline_style.color = None;
                if let Some(color) = converted.color
                    && Some(color) != fallback.color
                {
                    inline_style.color = Some(color);
                }
                if let Some(bg) = converted.bg_color {
                    inline_style.bg_color = Some(bg);
                }
                inline_style.effects = converted.effects | fallback.effects;
                plain_line.push_str(&segment.text);
                segments.push(InlineSegment {
                    text: segment.text.clone(),
                    style: Arc::new(inline_style),
                });
            }

            prepared.push(segments);
            plain.push(plain_line);
            // Extend the tinted band on this same row (not a follow-up line)
            // so empty/short add/del rows stay full-width.
            if let (Some(available_width), Some(background)) = (available_width, line.line_background) {
                let padding_style = Style::new().bg_color(Some(background));
                let rendered_width: usize = line
                    .segments
                    .iter()
                    .map(|segment| UnicodeWidthStr::width(segment.text.as_str()))
                    .sum();
                let padding_width = available_width.saturating_sub(rendered_width);
                let Some(last_line) = prepared.last_mut() else {
                    continue;
                };
                let Some(last_plain) = plain.last_mut() else {
                    continue;
                };
                if padding_width > 0 {
                    last_line.push(InlineSegment {
                        text: " ".repeat(padding_width),
                        style: Arc::new(convert_to_inline_style(padding_style)),
                    });
                    last_plain.push_str(&" ".repeat(padding_width));
                } else if last_line.is_empty() {
                    // Empty add/del body: still paint a one-cell tint band.
                    last_line.push(InlineSegment {
                        text: " ".to_owned(),
                        style: Arc::new(convert_to_inline_style(padding_style)),
                    });
                    last_plain.push(' ');
                }
            }
        }

        if prepared.is_empty() {
            prepared.push(Vec::new());
            plain.push(String::new());
        }

        let last_empty = plain.last().map(|line| line.trim().is_empty()).unwrap_or(true);

        (prepared, plain, last_empty)
    }

    pub(super) fn write_markdown(
        &mut self,
        text: &str,
        indent: &str,
        base_style: Style,
        kind: InlineMessageKind,
        preserve_code_indentation: bool,
    ) -> Result<bool> {
        let record_transcript = Self::should_record_transcript(kind);
        if let Some(payload) = Self::detect_large_json_payload(kind, text) {
            self.emit_large_json_payload(payload, indent, kind, record_transcript)?;
            return Ok(false);
        }
        let table_max_width = self.table_content_width(kind, indent);
        let (prepared, plain, last_empty) = self.prepare_markdown_lines_with_table_width(
            text,
            indent,
            base_style,
            true,
            preserve_code_indentation,
            table_max_width,
        );
        for (segments, line) in prepared.into_iter().zip(plain.iter()) {
            if segments.is_empty() {
                self.handle.append_line(kind, Vec::new());
            } else {
                self.handle.append_line(kind, segments);
            }
            if record_transcript {
                transcript::append(line);
            }
        }
        Ok(last_empty)
    }

    pub(super) fn replace_inline_lines(
        &mut self,
        count: usize,
        lines: Vec<Vec<InlineSegment>>,
        plain: &[String],
        kind: InlineMessageKind,
    ) {
        self.handle.replace_last(count, kind, lines);
        if Self::should_record_transcript(kind) {
            transcript::replace_last(count, plain);
        }
    }

    pub(super) fn new(handle: InlineHandle, highlight_config: SyntaxHighlightingConfig) -> Self {
        Self {
            handle,
            highlight_config,
            table_max_width: None,
            table_max_width_override: None,
        }
    }

    pub(super) fn set_highlight_config(&mut self, highlight_config: SyntaxHighlightingConfig) {
        self.highlight_config = highlight_config;
    }

    pub(super) fn show_list_modal(
        &self,
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
    ) {
        self.handle.show_list_modal(title, lines, items, selected, search);
    }

    pub(super) fn show_list_modal_with_status(
        &self,
        title: String,
        lines: Vec<String>,
        items: Vec<InlineListItem>,
        selected: Option<InlineListSelection>,
        search: Option<InlineListSearchConfig>,
        footer_hint: Option<String>,
        status: Option<vtcode_commons::ui_protocol::InlineStatus>,
    ) {
        self.handle
            .show_list_modal_with_status(title, lines, items, selected, search, footer_hint, status);
    }

    pub(super) fn show_secure_prompt_modal(&self, title: String, lines: Vec<String>, prompt_label: String) {
        self.handle.show_modal(
            title,
            lines,
            Some(SecurePromptConfig {
                label: prompt_label,
                placeholder: None,
                mask_input: true,
            }),
        );
    }

    pub(super) fn close_modal(&self) {
        self.handle.close_modal();
    }

    #[expect(
        dead_code,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    pub(super) fn clear_screen(&self) {
        self.handle.clear_screen();
    }

    pub(super) fn resolve_fallback_style(&self, style: Style) -> InlineTextStyle {
        let mut text_style = convert_to_inline_style(style);
        if text_style.color.is_none() {
            let active = theme::active_styles();
            text_style = text_style.merge_color(Some(active.foreground));
        }
        text_style
    }

    pub(super) fn style_to_segment(&self, style: Style, text: &str) -> InlineSegment {
        let text_style = self.resolve_fallback_style(style);
        InlineSegment {
            text: text.to_string(),
            style: Arc::new(text_style),
        }
    }

    pub(super) fn convert_plain_lines(
        &self,
        text: &str,
        fallback: &InlineTextStyle,
    ) -> (Vec<Vec<InlineSegment>>, Vec<String>) {
        let fallback_arc = Arc::new(fallback.clone());
        if text.is_empty() {
            return (vec![Vec::new()], vec![String::new()]);
        }

        let had_trailing_newline = text.ends_with('\n');
        let line_count_estimate = Self::count_lines(text).max(1);

        #[cfg(feature = "tui")]
        if let Ok(parsed) = text.as_bytes().into_text() {
            let mut converted_lines = Vec::with_capacity(parsed.lines.len().max(line_count_estimate));
            let mut plain_lines = Vec::with_capacity(parsed.lines.len().max(line_count_estimate));
            let base_style = RatatuiStyle::default().patch(parsed.style);

            for line in &parsed.lines {
                // Pre-allocate segments based on typical span count (3-5 spans per line)
                let mut segments = Vec::with_capacity(line.spans.len());
                let mut plain_line = String::with_capacity(80);
                let line_style = base_style.patch(line.style);

                for span in &line.spans {
                    // Use as_ref() to avoid unnecessary clone - Cow is already optimized
                    let content: &str = &span.content;
                    if content.is_empty() {
                        continue;
                    }

                    let span_style = line_style.patch(span.style);
                    let inline_style = self.inline_style_from_ratatui(span_style, fallback);
                    plain_line.push_str(content);
                    segments.push(InlineSegment {
                        text: content.to_string(),
                        style: Arc::new(inline_style),
                    });
                }

                converted_lines.push(segments);
                plain_lines.push(plain_line);
            }

            let needs_placeholder_line = if converted_lines.is_empty() {
                true
            } else {
                had_trailing_newline && plain_lines.last().is_none_or(|line| !line.is_empty())
            };
            if needs_placeholder_line {
                converted_lines.push(Vec::new());
                plain_lines.push(String::new());
            }

            return (converted_lines, plain_lines);
        }

        // Fallback: Process as plain text without ANSI parsing
        let line_count_estimate = Self::count_lines(text).max(1);
        let mut converted_lines = Vec::with_capacity(line_count_estimate);
        let mut plain_lines = Vec::with_capacity(line_count_estimate);

        for line in text.split('\n') {
            let mut segments = Vec::with_capacity(1);
            if !line.is_empty() {
                let owned = line.to_string();
                segments.push(InlineSegment {
                    text: owned.clone(),
                    style: Arc::clone(&fallback_arc),
                });
                converted_lines.push(segments);
                plain_lines.push(owned);
            } else {
                converted_lines.push(segments);
                plain_lines.push(String::new());
            }
        }

        if had_trailing_newline {
            converted_lines.push(Vec::new());
            plain_lines.push(String::new());
        }

        if converted_lines.is_empty() {
            converted_lines.push(Vec::new());
            plain_lines.push(String::new());
        }

        (converted_lines, plain_lines)
    }

    pub(super) fn write_multiline(
        &mut self,
        style: Style,
        indent: &str,
        text: &str,
        kind: InlineMessageKind,
    ) -> Result<()> {
        self.write_multiline_with_transcript(style, indent, text, kind, Self::should_record_transcript(kind), None)
    }

    pub(super) fn write_multiline_with_transcript(
        &mut self,
        style: Style,
        indent: &str,
        text: &str,
        kind: InlineMessageKind,
        record_transcript: bool,
        tool_output_id: Option<ToolOutputId>,
    ) -> Result<()> {
        let text_storage;
        let text = if kind == InlineMessageKind::Agent {
            text_storage = crate::utils::ansi_parser::strip_ansi(text);
            &text_storage
        } else {
            text
        };
        let record_transcript = record_transcript && Self::should_record_transcript(kind);

        if text.is_empty() {
            if let Some(id) = tool_output_id {
                self.handle.append_tool_output_line(id, kind, Vec::new());
            } else {
                self.handle.append_line(kind, Vec::new());
            }
            return Ok(());
        }

        if let Some(payload) = Self::detect_large_json_payload(kind, text) {
            // Summary lines are ordinary short text, so an anchor cannot
            // normally reach this path. Keep the large-payload placeholder
            // protocol unchanged if a future caller does pass one through.
            self.emit_large_json_payload(payload, indent, kind, record_transcript)?;
            return Ok(());
        }

        let fallback = self.resolve_fallback_style(style);
        let fallback_arc = Arc::new(fallback.clone());
        let (converted_lines, plain_lines) = self.convert_plain_lines(text, &fallback);

        // Combine multiple lines into a single append for User and Tool to avoid
        // creating a separate inline entry for each line. This prevents the
        // UI from showing a separate line per original line of tool output.
        if kind == InlineMessageKind::User || kind == InlineMessageKind::Tool {
            let total_plain_len: usize = plain_lines.iter().map(|p| p.len()).sum();
            let mut combined_segments = Vec::with_capacity(converted_lines.len());
            let mut combined_plain = String::with_capacity(total_plain_len);

            for (mut segments, plain) in converted_lines.into_iter().zip(plain_lines) {
                if !combined_segments.is_empty() {
                    combined_segments.push(InlineSegment {
                        text: "\n".to_owned(),
                        style: Arc::clone(&fallback_arc),
                    });
                    combined_plain.push('\n');
                }

                if !indent.is_empty() && !plain.is_empty() {
                    segments.insert(
                        0,
                        InlineSegment {
                            text: indent.to_string(),
                            style: Arc::clone(&fallback_arc),
                        },
                    );
                    combined_plain.insert_str(0, indent);
                } else if !indent.is_empty() && plain.is_empty() {
                    segments.insert(
                        0,
                        InlineSegment {
                            text: indent.to_string(),
                            style: Arc::clone(&fallback_arc),
                        },
                    );
                }

                combined_segments.extend(segments);
                combined_plain.push_str(&plain);
            }

            // A captured tool-output id must survive even on Tool/User lines
            // (expand notices are ToolDetail): the combined append keeps the
            // one-entry-per-call layout while `append_tool_output_line` binds
            // the row to its viewer capture.
            if let Some(id) = tool_output_id {
                self.handle.append_tool_output_line(id, kind, combined_segments);
            } else {
                self.handle.append_line(kind, combined_segments);
            }
            if record_transcript {
                transcript::append(&combined_plain);
            }
        } else {
            let fallback_arc_opt = if !indent.is_empty() {
                Some(Arc::new(fallback.clone()))
            } else {
                None
            };
            let mut tool_output_id = tool_output_id;
            for (mut segments, mut plain) in converted_lines.into_iter().zip(plain_lines) {
                if let Some(ref style_arc) = fallback_arc_opt
                    && !plain.is_empty()
                {
                    segments.insert(
                        0,
                        InlineSegment {
                            text: indent.to_string(),
                            style: Arc::clone(style_arc),
                        },
                    );
                    plain.insert_str(0, indent);
                }

                if let Some(id) = tool_output_id.take() {
                    self.handle.append_tool_output_line(id, kind, segments);
                } else if segments.is_empty() {
                    self.handle.append_line(kind, Vec::new());
                } else {
                    self.handle.append_line(kind, segments);
                }
                if record_transcript {
                    transcript::append(&plain);
                }
            }
        }

        Ok(())
    }

    pub(super) fn write_line(&mut self, style: Style, indent: &str, text: &str, kind: InlineMessageKind) -> Result<()> {
        self.write_multiline(style, indent, text, kind)
    }

    pub(super) fn write_inline(&mut self, style: Style, text: &str, kind: InlineMessageKind) {
        if text.is_empty() {
            return;
        }
        let fallback = self.resolve_fallback_style(style);
        let fallback_arc = Arc::new(fallback.clone());
        let (converted_lines, _) = self.convert_plain_lines(text, &fallback);
        let line_count = converted_lines.len();

        for (index, segments) in converted_lines.into_iter().enumerate() {
            let has_next = index + 1 < line_count;
            if segments.is_empty() {
                if has_next {
                    self.handle.inline(
                        kind,
                        InlineSegment {
                            text: "\n".to_owned(),
                            style: Arc::clone(&fallback_arc),
                        },
                    );
                }
                continue;
            }

            for mut segment in segments {
                if has_next {
                    segment.text.push('\n');
                }
                self.handle.inline(kind, segment);
            }
        }
    }

    pub(super) fn write_segments(&mut self, segments: &[MarkdownSegment], kind: InlineMessageKind) -> Result<()> {
        let converted = self.convert_segments(segments);
        let plain = segments.iter().map(|segment| segment.text.clone()).collect::<String>();
        self.handle.append_line(kind, converted);
        if Self::should_record_transcript(kind) {
            transcript::append(&plain);
        }
        Ok(())
    }

    pub(super) fn convert_segments(&self, segments: &[MarkdownSegment]) -> Vec<InlineSegment> {
        if segments.is_empty() {
            return Vec::new();
        }

        let mut converted = Vec::with_capacity(segments.len());
        for segment in segments {
            if segment.text.is_empty() {
                continue;
            }
            converted.push(self.style_to_segment(segment.style, &segment.text));
        }
        converted
    }
}
