use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BottomPanelKind {
    None,
    InlineModal,
    FilePalette,
    HistoryPicker,
    SlashPalette,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BottomPanelSpec {
    kind: BottomPanelKind,
    height: u16,
}

impl Session {
    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let viewport = frame.area();
        if viewport.height == 0 || viewport.width == 0 {
            return;
        }

        // Clear entire frame if modal was just closed to remove artifacts
        if self.needs_full_clear {
            frame.render_widget(Clear, viewport);
            self.needs_full_clear = false;
        }

        // Calculate layout constraints
        let header_lines = self.header_lines();
        let header_height = self.header_height_from_lines(viewport.width, &header_lines);
        if header_height != self.header_rows {
            self.header_rows = header_height;
            self.recalculate_transcript_rows();
        }

        let inner_width = viewport.width.saturating_sub(2);
        let desired_lines = self.desired_input_lines(inner_width);
        let block_height = Self::input_block_height_for_lines(desired_lines);
        let status_height = ui::INLINE_INPUT_STATUS_HEIGHT;
        let input_core_height = block_height.saturating_add(status_height);
        let panel = resolve_bottom_panel_spec(self, viewport, header_height, input_core_height);
        let input_height = input_core_height.saturating_add(panel.height);
        self.apply_input_height(input_height);

        let mut constraints = vec![Constraint::Length(header_height), Constraint::Min(1)];
        constraints.push(Constraint::Length(input_height));

        let segments = Layout::vertical(constraints).split(viewport);

        let header_area = segments[0];
        let main_area = segments[1];
        let input_index = segments.len().saturating_sub(1);
        let input_area = segments[input_index];

        let _available_width = main_area.width;
        let _horizontal_minimum = ui::INLINE_CONTENT_MIN_WIDTH + ui::INLINE_NAVIGATION_MIN_WIDTH;

        let modal_in_bottom = matches!(panel.kind, BottomPanelKind::InlineModal);
        let (transcript_area, modal_area) = if modal_in_bottom {
            (main_area, None)
        } else {
            render::split_inline_modal_area(self, main_area)
        };
        let (input_area, bottom_panel_area) =
            split_input_and_bottom_panel_area(input_area, panel.height);
        let navigation_area = Rect::new(main_area.x, main_area.y, 0, 0); // No navigation area since timeline pane is removed

        // Use SessionWidget for buffer-based rendering (header, transcript, overlays)
        SessionWidget::new(self)
            .header_lines(header_lines.clone())
            .header_area(header_area)
            .transcript_area(transcript_area)
            .navigation_area(navigation_area) // Pass empty navigation area
            .render(viewport, frame.buffer_mut());

        // Handle frame-based rendering for components that need it
        // Note: header, transcript, and overlays are handled by SessionWidget
        // Timeline pane has been removed, so no navigation rendering
        self.render_input(frame, input_area);
        let mut rendered_bottom_modal = false;
        if !modal_in_bottom {
            if let Some(modal_area) = modal_area {
                render::render_modal(self, frame, modal_area);
            } else {
                render::render_modal(self, frame, viewport);
            }
        }
        if let Some(panel_area) = bottom_panel_area {
            match panel.kind {
                BottomPanelKind::InlineModal => {
                    frame.render_widget(Clear, panel_area);
                    render::render_modal(self, frame, panel_area);
                    rendered_bottom_modal = true;
                }
                BottomPanelKind::FilePalette => {
                    render::render_file_palette(self, frame, panel_area);
                }
                BottomPanelKind::HistoryPicker => {
                    render::render_history_picker(self, frame, panel_area);
                }
                BottomPanelKind::SlashPalette => {
                    slash::render_slash_palette(self, frame, panel_area);
                }
                BottomPanelKind::None => {
                    frame.render_widget(Clear, panel_area);
                }
            }
        }
        if modal_in_bottom && !rendered_bottom_modal {
            render::render_modal(self, frame, viewport);
        }

        // Render diff preview modal if active
        if self.diff_preview.is_some() {
            diff_preview::render_diff_preview(self, frame, viewport);
        }

        // Apply mouse text selection highlight
        if self.mouse_selection.has_selection || self.mouse_selection.is_selecting {
            self.mouse_selection
                .apply_highlight(frame.buffer_mut(), viewport);

            // Copy to clipboard once when selection is finalized
            if self.mouse_selection.needs_copy() {
                let text = self
                    .mouse_selection
                    .extract_text(frame.buffer_mut(), viewport);
                if !text.is_empty() {
                    MouseSelectionState::copy_to_clipboard(&text);
                }
                self.mouse_selection.mark_copied();
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn render_message_spans(&self, index: usize) -> Vec<Span<'static>> {
        let Some(line) = self.lines.get(index) else {
            return vec![Span::raw(String::new())];
        };
        message_renderer::render_message_spans(
            line,
            &self.theme,
            &self.labels,
            |kind| self.prefix_text(kind),
            |line| self.prefix_style(line),
            |kind| self.text_fallback(kind),
        )
    }
}

fn resolve_bottom_panel_spec(
    session: &mut Session,
    viewport: Rect,
    header_height: u16,
    input_reserved_height: u16,
) -> BottomPanelSpec {
    let max_panel_height = viewport
        .height
        .saturating_sub(header_height)
        .saturating_sub(input_reserved_height)
        .saturating_sub(1);
    if max_panel_height == 0 || viewport.width == 0 {
        return BottomPanelSpec {
            kind: BottomPanelKind::None,
            height: 0,
        };
    }

    if session.inline_lists_visible() {
        let split_context = SplitContext {
            width: viewport.width,
            max_panel_height,
        };
        if modal_eligible_for_inline_bottom(session) {
            if let Some(panel) = panel_from_split(
                session,
                split_context,
                BottomPanelKind::InlineModal,
                split_inline_modal_area_probe,
            ) {
                return panel;
            }
        } else if session.file_palette_active {
            if let Some(panel) = panel_from_split(
                session,
                split_context,
                BottomPanelKind::FilePalette,
                render::split_inline_file_palette_area,
            ) {
                return panel;
            }
        } else if session.history_picker_state.active {
            if let Some(panel) = panel_from_split(
                session,
                split_context,
                BottomPanelKind::HistoryPicker,
                render::split_inline_history_picker_area,
            ) {
                return panel;
            }
        } else if !session.slash_palette.is_empty()
            && let Some(panel) = panel_from_split(
                session,
                split_context,
                BottomPanelKind::SlashPalette,
                slash::split_inline_slash_area,
            )
        {
            return panel;
        }
    }

    BottomPanelSpec {
        kind: BottomPanelKind::None,
        height: 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SplitContext {
    width: u16,
    max_panel_height: u16,
}

fn panel_from_split(
    session: &mut Session,
    ctx: SplitContext,
    kind: BottomPanelKind,
    split_fn: fn(&mut Session, Rect) -> (Rect, Option<Rect>),
) -> Option<BottomPanelSpec> {
    let height = probe_panel_height(session, ctx, split_fn);
    if height == 0 {
        None
    } else {
        Some(BottomPanelSpec {
            kind,
            height: normalize_panel_height(height, ctx.max_panel_height),
        })
    }
}

fn normalize_panel_height(raw_height: u16, max_panel_height: u16) -> u16 {
    if raw_height == 0 || max_panel_height == 0 {
        return 0;
    }

    let min_floor = ui::INLINE_LIST_PANEL_MIN_HEIGHT
        .min(max_panel_height)
        .max(1);
    raw_height.max(min_floor).min(max_panel_height)
}

fn modal_eligible_for_inline_bottom(session: &Session) -> bool {
    session.wizard_modal.is_some()
        || session
            .modal
            .as_ref()
            .is_some_and(|modal| modal.list.is_some())
}

fn split_inline_modal_area_probe(session: &mut Session, area: Rect) -> (Rect, Option<Rect>) {
    render::split_inline_modal_area(session, area)
}

fn probe_panel_height(
    session: &mut Session,
    ctx: SplitContext,
    split_fn: fn(&mut Session, Rect) -> (Rect, Option<Rect>),
) -> u16 {
    if ctx.width == 0 || ctx.max_panel_height == 0 {
        return 0;
    }

    let probe_area = Rect::new(0, 0, ctx.width, ctx.max_panel_height.saturating_add(1));
    let (_, panel_area) = split_fn(session, probe_area);
    panel_area.map(|area| area.height).unwrap_or(0)
}

fn split_input_and_bottom_panel_area(area: Rect, panel_height: u16) -> (Rect, Option<Rect>) {
    if area.height == 0 || panel_height == 0 || area.height <= 1 {
        return (area, None);
    }

    let resolved_panel = panel_height.min(area.height.saturating_sub(1));
    if resolved_panel == 0 {
        return (area, None);
    }

    let input_height = area.height.saturating_sub(resolved_panel);
    let chunks = Layout::vertical([
        Constraint::Length(input_height.max(1)),
        Constraint::Length(resolved_panel),
    ])
    .split(area);
    (chunks[0], Some(chunks[1]))
}
