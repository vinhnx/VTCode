use super::*;
use anstyle::{Color as AnsiColorEnum, Effects};
use ratatui::style::{Color, Modifier};
use tui_shimmer::shimmer_spans_with_style_at_phase;
use unicode_width::UnicodeWidthStr;

use super::input::{CompactInputPreview, InputRender, InputStatusLine, InputToken, InputTokenKind, tokenize_input};
use super::utils::line_truncation::truncate_line_with_ellipsis_if_overflow;

impl Session {
    /// Build (or reuse) the input paragraph model for the current input state.
    pub(super) fn build_input_render(&mut self, width: u16, height: u16) -> InputRender {
        if width == 0 || height == 0 {
            return InputRender { text: Text::default(), cursor_x: 0, cursor_y: 0 };
        }

        // Hash content (not just length) so same-length edits cannot serve a
        // stale cached paragraph. Cursor and flags cover layout/placeholder.
        let mut hasher = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(self.input_manager.content(), &mut hasher);
        std::hash::Hash::hash(&self.prompt_prefix, &mut hasher);
        let content_hash = std::hash::Hasher::finish(&hasher);
        let cursor = self.input_manager.cursor();
        let compact = self.input_compact_mode;
        let suggested = self.suggested_prompt_state.active;
        let key = (width, height, content_hash, cursor, compact, suggested);

        if let Some((w, h, hash, cur, cmp, sug, cached)) = self.input_render_cache.take() {
            if (w, h, hash, cur, cmp, sug) == key {
                let render = InputRender {
                    text: cached.text.clone(),
                    cursor_x: cached.cursor_x,
                    cursor_y: cached.cursor_y,
                };
                self.input_render_cache = Some((w, h, hash, cur, cmp, sug, cached));
                return render;
            }
            // Stale — drop before rebuild.
        }

        let render = self.build_input_render_uncached(width, height);
        self.input_render_cache = Some((
            width,
            height,
            content_hash,
            cursor,
            compact,
            suggested,
            InputRender {
                text: render.text.clone(),
                cursor_x: render.cursor_x,
                cursor_y: render.cursor_y,
            },
        ));
        render
    }

    #[cfg_attr(feature = "profiling", hotpath::measure)]
    pub(super) fn build_input_render_uncached(&self, width: u16, height: u16) -> InputRender {
        if width == 0 || height == 0 {
            return InputRender { text: Text::default(), cursor_x: 0, cursor_y: 0 };
        }

        let max_visible_lines = height.max(1).min(ui::INLINE_INPUT_MAX_LINES as u16) as usize;

        let mut prompt_style = self.prompt_style.clone();
        if prompt_style.color.is_none() {
            prompt_style.color = self.theme.primary.or(self.theme.foreground);
        }
        if self.suggested_prompt_state.active {
            prompt_style.color = self
                .theme
                .tool_accent
                .or(self.theme.secondary)
                .or(self.theme.primary)
                .or(self.theme.foreground);
            prompt_style.effects |= Effects::BOLD;
        }
        let prompt_style = ratatui_style_from_inline(&prompt_style, self.theme.foreground);
        let prompt_width = UnicodeWidthStr::width(self.prompt_prefix.as_str()) as u16;
        let prompt_display_width = prompt_width.min(width);
        let accent_style = ratatui_style_from_inline(&self.styles.accent_inline_style(), self.theme.foreground);

        let cursor_at_end = self.input_manager.cursor() == self.input_manager.content().len();
        if self.input_compact_mode
            && cursor_at_end
            && let Some(preview) = self.input_compact_preview().or_else(|| {
                self.input_compact_placeholder().map(|placeholder| CompactInputPreview {
                    before: String::new(),
                    placeholder,
                    after_lines: Vec::new(),
                })
            })
        {
            let placeholder_style = InlineTextStyle {
                color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                bg_color: None,
                effects: Effects::DIMMED,
            };
            let style = ratatui_style_from_inline(&placeholder_style, Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)));
            let mut spans = Vec::new();
            spans.push(Span::styled(self.prompt_prefix.clone(), prompt_style));
            if !preview.before.is_empty() {
                let needs_space = !preview.before.ends_with(char::is_whitespace);
                spans.push(Span::styled(preview.before, accent_style));
                if needs_space {
                    spans.push(Span::styled(" ".to_string(), accent_style));
                }
            }
            let first_after = preview.after_lines.first().map(String::as_str).unwrap_or_default();
            let needs_after_space = !first_after.is_empty() && !first_after.starts_with(char::is_whitespace);
            spans.push(Span::styled(preview.placeholder, style));
            if needs_after_space {
                spans.push(Span::styled(" ".to_string(), accent_style));
            }
            if !first_after.is_empty() {
                spans.push(Span::styled(first_after.to_string(), accent_style));
            }
            let indent_prefix = " ".repeat(prompt_display_width as usize);
            let mut lines = vec![Line::from(spans)];
            for line in preview.after_lines.iter().skip(1) {
                let mut spans = vec![Span::styled(indent_prefix.clone(), prompt_style)];
                if !line.is_empty() {
                    spans.push(Span::styled(line.clone(), accent_style));
                }
                lines.push(Line::from(spans));
            }
            let cursor_y = lines.len().saturating_sub(1) as u16;
            let cursor_x = if cursor_y == 0 {
                lines[0]
                    .spans
                    .iter()
                    .skip(1)
                    .map(|span| UnicodeWidthStr::width(span.content.as_ref()) as u16)
                    .fold(prompt_display_width, u16::saturating_add)
            } else {
                let last_line = preview.after_lines.last().map(String::as_str).unwrap_or_default();
                prompt_display_width.saturating_add(UnicodeWidthStr::width(last_line) as u16)
            };
            return InputRender { text: Text::from(lines), cursor_x, cursor_y };
        }

        if self.input_manager.content().is_empty() {
            let mut spans = Vec::new();
            spans.push(Span::styled(self.prompt_prefix.clone(), prompt_style));

            if let Some(suffix) = self.visible_inline_prompt_suggestion_suffix() {
                let ghost_style = ratatui_style_from_inline(
                    &InlineTextStyle {
                        color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                        bg_color: None,
                        effects: Effects::DIMMED | Effects::ITALIC,
                    },
                    Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                );
                spans.push(Span::styled(suffix, ghost_style));
            } else if let Some(placeholder) = &self.placeholder {
                let placeholder_style = self.placeholder_style.clone().unwrap_or(InlineTextStyle {
                    color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                    bg_color: None,
                    effects: Effects::ITALIC,
                });
                let style = ratatui_style_from_inline(&placeholder_style, Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)));
                spans.push(Span::styled(placeholder.clone(), style));
            }

            return InputRender {
                text: Text::from(vec![Line::from(spans)]),
                cursor_x: prompt_display_width,
                cursor_y: 0,
            };
        }

        let slash_style = accent_style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
        let file_ref_style = accent_style.fg(Color::Cyan).add_modifier(Modifier::UNDERLINED);
        let code_style = accent_style.fg(Color::Green).add_modifier(Modifier::BOLD);

        let (layout, start, end) = self.visible_input_window(width, max_visible_lines as u16);
        let tokens = tokenize_input(self.input_manager.content());
        let cursor_y = layout.cursor_line_idx.saturating_sub(start) as u16;

        let vis_count = end.saturating_sub(start);
        let mut lines = Vec::with_capacity(vis_count);
        for buffer in &layout.buffers[start..end] {
            let mut spans = Vec::with_capacity(4);
            spans.push(Span::styled(buffer.prefix.clone(), prompt_style));
            if !buffer.text.is_empty() {
                let buf_chars: Vec<char> = buffer.text.chars().collect();
                let buf_len = buf_chars.len();
                let buf_start = buffer.char_start;
                let buf_end = buf_start + buf_len;

                let mut pos = 0usize;
                for token in &tokens {
                    if token.end <= buf_start || token.start >= buf_end {
                        continue;
                    }
                    let seg_start = token.start.max(buf_start).saturating_sub(buf_start);
                    let seg_end = token.end.min(buf_end).saturating_sub(buf_start);
                    if seg_start > pos {
                        let text: String = buf_chars[pos..seg_start].iter().collect();
                        spans.push(Span::styled(text, accent_style));
                    }
                    let text: String = buf_chars[seg_start..seg_end].iter().collect();
                    let style = match token.kind {
                        InputTokenKind::SlashCommand => slash_style,
                        InputTokenKind::AgentReference | InputTokenKind::FileReference => file_ref_style,
                        InputTokenKind::InlineCode => code_style,
                        InputTokenKind::Normal => accent_style,
                    };
                    spans.push(Span::styled(text, style));
                    pos = seg_end;
                }
                if pos < buf_len {
                    let text: String = buf_chars[pos..].iter().collect();
                    spans.push(Span::styled(text, accent_style));
                }
            }
            lines.push(Line::from(spans));
        }

        if let Some(suffix) = self.visible_inline_prompt_suggestion_suffix() {
            let ghost_style = ratatui_style_from_inline(
                &InlineTextStyle {
                    color: Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
                    bg_color: None,
                    effects: Effects::DIMMED | Effects::ITALIC,
                },
                Some(AnsiColorEnum::Rgb(PLACEHOLDER_COLOR)),
            );
            if let Some(line) = lines.get_mut(cursor_y as usize) {
                line.spans.push(Span::styled(suffix, ghost_style));
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(vec![Span::styled(self.prompt_prefix.clone(), prompt_style)]));
        }

        InputRender {
            text: Text::from(lines),
            cursor_x: layout.cursor_column,
            cursor_y,
        }
    }

    #[cfg_attr(feature = "profiling", hotpath::measure)]
    pub(super) fn build_input_status_line(&self, width: u16) -> Option<InputStatusLine> {
        if width == 0 {
            return None;
        }

        let copy_notification = self.copy_notification_text();
        let showing_progress = copy_notification.is_none() && self.progress.is_active() && !self.progress_row_visible();
        // Queued follow-ups are the freshest user action, so they take the
        // composer line ahead of configured context and the generic
        // "Thinking" label — but never ahead of a live phase status.
        //
        // While the transcript owns foreground progress, runtime/stage status
        // text stays suppressed so the line never repeats or reflows the
        // loading row between phases; a queued follow-up is a direct user
        // action rather than background churn, so it still surfaces (and is
        // often the only acknowledgement visible once the loading row has
        // scrolled the queue overlay away).
        let left = copy_notification.or_else(|| {
            self.progress_footer_status_text().map(str::to_owned).or_else(|| {
                (!self.progress.is_active() && !self.footer_context_configured)
                    .then(|| self.status_left_text().map(str::to_owned))
                    .flatten()
            })
        });
        let thinking_spinner_line = || {
            (self.thinking_spinner.is_active && !self.progress.is_active()).then(|| {
                if self.appearance.should_animate_progress_status() {
                    format!("{} Thinking", self.thinking_spinner.current_frame())
                } else {
                    "Thinking".to_owned()
                }
            })
        };
        let left = left.or_else(|| self.queued_input_status_text()).or_else(thinking_spinner_line);
        let configured_right = if self.footer_context_configured {
            self.footer_context_right.as_deref()
        } else {
            self.status_right_text()
        };
        let right = match (configured_right, self.vim_state.status_label()) {
            (Some(existing), Some(vim_label)) => Some(format!("{vim_label} · {existing}")),
            (None, Some(vim_label)) => Some(vim_label.to_string()),
            (existing, None) => existing.map(str::to_owned),
        };
        let mode_pill = self.primary_mode_pill();
        // Bottom-line background affordances are idle-only. While anything is
        // busy — the transcript loading row, a foreground command, or an
        // in-flight turn — the composer line keeps only configured context so
        // it never reflows as progress phases or the PTY counter toggle
        // underneath (the `· Ctrl+B background` flicker after the branch
        // status). The live count moves to the transcript loading row plus a
        // static header badge, and `Ctrl+B` discovery moves to the header
        // suggestions line; the `Ctrl+B`, `Alt+S`, `/jobs`, and empty-Enter
        // entry points keep working while busy. When idle the copy is
        // width-deterministic (budget decides) rather than state-gated, so a
        // fitting count stays put across tool gaps.
        let busy = self.progress.is_active() || self.has_active_foreground_pty() || self.is_running_activity();
        let background_hint = if busy {
            None
        } else {
            self.local_agents_input_status_hint()
        };
        let background_status = if self.progress.is_active() {
            None
        } else {
            self.background_activity_status_text()
        };
        let dim_style = {
            let mut style = self.styles.default_style().add_modifier(Modifier::DIM);
            if let Some(secondary) = self.theme.secondary.or(self.theme.foreground) {
                style = style.fg(ratatui_color_from_ansi(secondary));
            }
            style
        };
        let key_style = {
            let mut style = self.styles.default_style().add_modifier(Modifier::BOLD);
            if let Some(primary) = self.theme.primary.or(self.theme.foreground) {
                style = style.fg(ratatui_color_from_ansi(primary));
            }
            style
        };
        let label_style = {
            let mut style = self.styles.default_style();
            if let Some(secondary) = self.theme.secondary.or(self.theme.foreground) {
                style = style.fg(ratatui_color_from_ansi(secondary));
            }
            style
        };
        // Allocate persistent regions first. Mode has first claim, then the
        // configured right side, then context, then optional activity/hints.
        let mut right_spans = Vec::new();
        if let Some((label, style)) = mode_pill {
            right_spans =
                truncate_line_with_ellipsis_if_overflow(Line::from(Span::styled(label, style)), usize::from(width))
                    .spans;
        }
        let mode_width = Line::from(right_spans.clone()).width() as u16;
        if let Some(value) = right {
            let separator = u16::from(mode_width > 0);
            let budget = width.saturating_sub(mode_width.saturating_add(separator));
            if budget > 0 {
                if separator > 0 {
                    right_spans.push(Span::raw(" "));
                }
                right_spans.extend(
                    truncate_line_with_ellipsis_if_overflow(
                        Line::from(Span::styled(value, dim_style)),
                        usize::from(budget),
                    )
                    .spans,
                );
            }
        }
        let right_width = Line::from(right_spans.clone()).width() as u16;
        let left_budget = width.saturating_sub(right_width.saturating_add(u16::from(right_width > 0)));
        let mut spans = left
            .as_ref()
            .map(|text| {
                if !self.footer_context_configured
                    && status_requires_shimmer(text)
                    && self.appearance.should_animate_progress_status()
                {
                    shimmer_spans_with_style_at_phase(
                        text,
                        self.styles.accent_style().add_modifier(Modifier::DIM),
                        self.shimmer_state.phase(),
                    )
                } else {
                    self.create_git_status_spans(text, dim_style)
                }
            })
            .unwrap_or_default();
        spans = if left_budget == 0 {
            Vec::new()
        } else {
            truncate_line_with_ellipsis_if_overflow(Line::from(spans), usize::from(left_budget)).spans
        };
        let mut background_hits = Vec::new();
        let mut progress_start = 0;
        let mut progress_columns = 0;

        if showing_progress && let Some(text) = self.progress.text() {
            let used = Line::from(spans.clone()).width() as u16;
            let separator = u16::from(used > 0);
            let budget = left_budget.saturating_sub(used + separator).min(24);
            if budget > 1 {
                if separator > 0 {
                    spans.push(Span::raw(" "));
                }
                progress_start = used + separator;
                let truncated = measure_text_width(&text) > budget;
                let style = self.styles.accent_style().add_modifier(Modifier::DIM);
                let progress_spans = if self.progress.is_animated() && self.appearance.should_animate_progress_status()
                {
                    shimmer_spans_with_style_at_phase(&text, style, self.shimmer_state.phase())
                } else {
                    vec![Span::styled(text, dim_style)]
                };
                let line = truncate_line_with_ellipsis_if_overflow(Line::from(progress_spans), usize::from(budget));
                let actual = line.width() as u16;
                progress_columns = actual.saturating_sub(u16::from(truncated));
                spans.extend(line.spans);
                spans.push(Span::raw(" ".repeat(usize::from(budget.saturating_sub(actual)))));
            }
        }
        if let Some(status) = background_status {
            let used = Line::from(spans.clone()).width() as u16;
            let separator = u16::from(used > 0);
            let budget = left_budget.saturating_sub(used + separator * 3);
            if measure_text_width(&status) <= budget {
                let start = used + separator * 3;
                if separator > 0 {
                    spans.push(Span::raw(" · "));
                }
                background_hits.push((start, start + measure_text_width(&status)));
                if self.appearance.should_animate_progress_status() {
                    spans.extend(shimmer_spans_with_style_at_phase(&status, dim_style, self.shimmer_state.phase()));
                } else {
                    spans.push(Span::styled(status, dim_style));
                }
            }
        }
        if let Some(hint) = background_hint {
            let used = Line::from(spans.clone()).width() as u16;
            let mut hint_spans = Vec::new();
            let hit = Self::append_background_hint_spans(
                &mut hint_spans,
                &hint,
                self.background_shortcut_label(),
                dim_style,
                key_style,
                label_style,
            );
            if Line::from(hint_spans.clone()).width() <= usize::from(left_budget.saturating_sub(used)) {
                spans.extend(hint_spans);
                if let Some((start, end)) = hit {
                    background_hits.push((used + start, used + end));
                }
            }
        }
        for hint in [
            self.shell_mode_status_hint().map(str::to_owned),
            self.build_scroll_indicator(),
        ]
        .into_iter()
        .flatten()
        {
            let used = Line::from(spans.clone()).width() as u16;
            let separator = u16::from(used > 0);
            if measure_text_width(&hint) + separator * 3 <= left_budget.saturating_sub(used) {
                if separator > 0 {
                    spans.push(Span::raw(" · "));
                }
                spans.push(Span::styled(hint, dim_style));
            }
        }
        if !right_spans.is_empty() {
            let used = Line::from(spans.clone()).width() as u16;
            spans.push(Span::raw(" ".repeat(usize::from(width.saturating_sub(used + right_width)))));
            spans.extend(right_spans);
        }
        (!spans.is_empty()).then_some(InputStatusLine {
            line: Line::from(spans),
            background_hits,
            progress_start,
            progress_columns,
        })
    }
}
