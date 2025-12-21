use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    widgets::Widget,
};

use super::{FilePaletteWidget, HeaderWidget, PromptPaletteWidget, TranscriptWidget};
use crate::ui::tui::session::{Session, render::apply_view_rows};

/// Root compositor widget that orchestrates rendering of the entire session UI
///
/// This widget follows the compositional pattern recommended by Ratatui where
/// a single root widget manages the layout and delegates rendering to child widgets.
///
/// It handles:
/// - Layout calculation (header, transcript, input areas)
/// - Coordinating child widget rendering
/// - Modal and palette overlay management
///
/// # Example
/// ```ignore
/// SessionWidget::new(session)
///     .header_lines(lines)
///     .header_area(header_area)
///     .transcript_area(transcript_area)
///     .navigation_area(navigation_area)
///     .render(area, buf);
/// ```
pub struct SessionWidget<'a> {
    session: &'a mut Session,
    header_lines: Option<Vec<ratatui::text::Line<'static>>>,
    header_area: Option<Rect>,
    transcript_area: Option<Rect>,
    navigation_area: Option<Rect>,
}

impl<'a> SessionWidget<'a> {
    /// Create a new SessionWidget with required parameters
    pub fn new(session: &'a mut Session) -> Self {
        Self { 
            session,
            header_lines: None,
            header_area: None,
            transcript_area: None,
            navigation_area: None,
        }
    }

    /// Set the header lines to render
    pub fn header_lines(mut self, lines: Vec<ratatui::text::Line<'static>>) -> Self {
        self.header_lines = Some(lines);
        self
    }

    /// Set the header area
    pub fn header_area(mut self, area: Rect) -> Self {
        self.header_area = Some(area);
        self
    }

    /// Set the transcript area
    pub fn transcript_area(mut self, area: Rect) -> Self {
        self.transcript_area = Some(area);
        self
    }

    /// Set the navigation area
    pub fn navigation_area(mut self, area: Rect) -> Self {
        self.navigation_area = Some(area);
        self
    }
}

impl<'a> Widget for &'a mut SessionWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        // Update spinner animation if active
        self.session.thinking_spinner.update();
        if self.session.thinking_spinner.is_active {
            self.session.needs_redraw = true;
            if let Some(spinner_idx) = self.session.thinking_spinner.spinner_line_index {
                if spinner_idx < self.session.lines.len() {
                    let frame = self.session.thinking_spinner.current_frame();
                    let revision = self.session.next_revision();
                    if let Some(line) = self.session.lines.get_mut(spinner_idx) {
                        if !line.segments.is_empty() {
                            line.segments[0].text = format!("{} Thinking...", frame);
                            line.revision = revision;
                            self.session.mark_line_dirty(spinner_idx);
                        }
                    }
                }
            }
        }

        // Handle deferred triggers
        if self.session.deferred_file_browser_trigger {
            self.session.deferred_file_browser_trigger = false;
            self.session.input_manager.insert_char('@');
            self.session.check_file_reference_trigger();
            self.session.mark_dirty();
        }

        if self.session.deferred_prompt_browser_trigger {
            self.session.deferred_prompt_browser_trigger = false;
            self.session.input_manager.insert_char('#');
            self.session.check_prompt_reference_trigger();
            self.session.mark_dirty();
        }

        // Pull log entries
        self.session.poll_log_entries();

        // Use provided areas or fall back to calculated layout
        let header_area = self.header_area.unwrap_or_else(|| {
            // Calculate header height if not provided
            let header_lines = self.session.header_lines();
            let header_height = self.session.header_height_from_lines(area.width, &header_lines);
            if header_height != self.session.header_rows {
                self.session.header_rows = header_height;
                crate::ui::tui::session::render::recalculate_transcript_rows(self.session);
            }
            Rect::new(area.x, area.y, area.width, header_height)
        });

        let transcript_area = self.transcript_area.unwrap_or_else(|| {
            // Calculate remaining area for transcript
            let header_bottom = header_area.bottom();
            let remaining_height = area.height.saturating_sub(header_area.height);
            Rect::new(area.x, header_bottom, area.width, remaining_height)
        });

        let _navigation_area = self.navigation_area.unwrap_or(Rect::new(0, 0, 0, 0));

        // Update view rows for transcript
        apply_view_rows(self.session, transcript_area.height);

        // Render header using builder pattern
        let header_lines = self.header_lines.as_ref().unwrap_or(&self.session.header_lines()).clone();
        HeaderWidget::new(self.session)
            .lines(header_lines)
            .render(header_area, buf);

        // Render transcript with optional splits for timeline/logs
        let has_logs = self.session.show_logs && self.session.has_logs();
        
        if self.session.show_timeline_pane && has_logs {
            // Both timeline and logs visible - split horizontally
            let timeline_chunks =
                Layout::horizontal([Constraint::Percentage(70), Constraint::Percentage(30)])
                    .split(transcript_area);
            TranscriptWidget::new(self.session).render(timeline_chunks[0], buf);
            self.render_logs(timeline_chunks[1], buf);
        } else if has_logs {
            // Only logs visible (no timeline) - split vertically
            let split = Layout::vertical([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(transcript_area);
            TranscriptWidget::new(self.session).render(split[0], buf);
            self.render_logs(split[1], buf);
        } else {
            // Logs hidden or empty - full transcript area
            TranscriptWidget::new(self.session).render(transcript_area, buf);
        }

        // Render overlays (modals, palettes, etc.)
        self.render_overlays(area, buf);
    }
}

impl<'a> SessionWidget<'a> {
    fn render_logs(&mut self, area: Rect, buf: &mut Buffer) {
        use ratatui::widgets::{Block, Paragraph, Wrap};

        let block = Block::bordered()
            .title("Logs")
            .border_type(crate::ui::tui::session::terminal_capabilities::get_border_type())
            .style(self.session.styles.default_style())
            .border_style(self.session.styles.border_style());
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let paragraph = Paragraph::new((*self.session.log_text()).clone()).wrap(Wrap { trim: false });
        paragraph.render(inner, buf);
    }



    fn render_overlays(&mut self, viewport: Rect, buf: &mut Buffer) {
        // Note: Modal and slash palette still use Frame API, so they're handled separately
        // Only render the palette widgets that work with Buffer

        // Render file palette using builder pattern
        if self.session.file_palette_active {
            if let Some(palette) = self.session.file_palette.as_ref() {
                FilePaletteWidget::new(self.session, palette, viewport).render(viewport, buf);
            }
        }

        // Render prompt palette using builder pattern
        if self.session.prompt_palette_active {
            if let Some(palette) = self.session.prompt_palette.as_ref() {
                PromptPaletteWidget::new(self.session, palette, viewport).render(viewport, buf);
            }
        }
    }
}

#[allow(dead_code)]
fn has_input_status(session: &Session) -> bool {
    let left_present = session
        .input_status_left
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    if left_present {
        return true;
    }
    session
        .input_status_right
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
}
