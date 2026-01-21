use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    widgets::Widget,
};

use super::{
    FilePaletteWidget, FooterWidget, HeaderWidget, LayoutMode, Panel, PromptPaletteWidget,
    SidebarWidget, TranscriptWidget, footer_hints,
};
use crate::ui::tui::session::{Session, render::apply_view_rows};

/// Root compositor widget that orchestrates rendering of the entire session UI
///
/// This widget follows the compositional pattern recommended by Ratatui where
/// a single root widget manages the layout and delegates rendering to child widgets.
///
/// It handles:
/// - Responsive layout based on terminal size (Compact/Standard/Wide)
/// - Layout calculation (header, main, footer regions)
/// - Coordinating child widget rendering
/// - Modal and palette overlay management
/// - Sidebar rendering in wide mode
///
/// # Layout Modes
///
/// - **Compact** (< 80 cols): Minimal chrome, no borders, no sidebar
/// - **Standard** (80-119 cols): Borders, titles, optional logs panel
/// - **Wide** (>= 120 cols): Full layout with sidebar for queue/context
///
/// # Example
/// ```ignore
/// SessionWidget::new(session)
///     .header_lines(lines)
///     .header_area(header_area)
///     .transcript_area(transcript_area)
///     .render(area, buf);
/// ```
pub struct SessionWidget<'a> {
    session: &'a mut Session,
    header_lines: Option<Vec<ratatui::text::Line<'static>>>,
    header_area: Option<Rect>,
    transcript_area: Option<Rect>,
    navigation_area: Option<Rect>,
    layout_mode: Option<LayoutMode>,
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
            layout_mode: None,
        }
    }

    /// Set the header lines to render
    #[must_use]
    pub fn header_lines(mut self, lines: Vec<ratatui::text::Line<'static>>) -> Self {
        self.header_lines = Some(lines);
        self
    }

    /// Set the header area
    #[must_use]
    pub fn header_area(mut self, area: Rect) -> Self {
        self.header_area = Some(area);
        self
    }

    /// Set the transcript area
    #[must_use]
    pub fn transcript_area(mut self, area: Rect) -> Self {
        self.transcript_area = Some(area);
        self
    }

    /// Set the navigation area
    #[must_use]
    pub fn navigation_area(mut self, area: Rect) -> Self {
        self.navigation_area = Some(area);
        self
    }

    /// Override the layout mode (auto-detected by default)
    #[must_use]
    pub fn layout_mode(mut self, mode: LayoutMode) -> Self {
        self.layout_mode = Some(mode);
        self
    }

    /// Compute the layout regions based on viewport and layout mode
    fn compute_layout(&self, area: Rect, mode: LayoutMode) -> SessionLayout {
        let footer_h = mode.footer_height();
        let max_header_pct = mode.max_header_percent();

        // Compute header height
        let header_lines = self
            .header_lines
            .as_ref()
            .unwrap_or(&self.session.header_lines())
            .clone();
        let natural_header_h = self
            .session
            .header_height_from_lines(area.width, &header_lines);
        let max_header_h = ((area.height as f32) * max_header_pct) as u16;
        let header_h = natural_header_h.min(max_header_h).max(1);

        // Main region constraints
        let main_h = area.height.saturating_sub(header_h + footer_h);

        let [header_area, main_area, footer_area] = Layout::vertical([
            Constraint::Length(header_h),
            Constraint::Length(main_h),
            Constraint::Length(footer_h),
        ])
        .split(area)[..] else {
            return SessionLayout {
                header: Rect::ZERO,
                main: Rect::ZERO,
                sidebar: None,
                footer: Rect::ZERO,
                mode,
            };
        };

        // In wide mode, split main into transcript and sidebar
        // Respect appearance config for sidebar visibility
        let show_sidebar = mode.allow_sidebar() && self.session.appearance.should_show_sidebar();
        if show_sidebar {
            let sidebar_pct = mode.sidebar_width_percent();
            let [left, right] = Layout::horizontal([
                Constraint::Percentage(100 - sidebar_pct),
                Constraint::Percentage(sidebar_pct),
            ])
            .split(main_area)[..] else {
                return SessionLayout {
                    header: header_area,
                    main: main_area,
                    sidebar: None,
                    footer: footer_area,
                    mode,
                };
            };
            return SessionLayout {
                header: header_area,
                main: left,
                sidebar: Some(right),
                footer: footer_area,
                mode,
            };
        }

        SessionLayout {
            header: header_area,
            main: main_area,
            sidebar: None,
            footer: footer_area,
            mode,
        }
    }
}

/// Computed layout regions for the session UI
struct SessionLayout {
    header: Rect,
    main: Rect,
    sidebar: Option<Rect>,
    footer: Rect,
    #[allow(dead_code)]
    mode: LayoutMode,
}

impl Widget for &mut SessionWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        // Determine layout mode from viewport or override
        let mode = self
            .layout_mode
            .unwrap_or_else(|| LayoutMode::from_area(area));

        // Update spinner animation if active
        self.session.thinking_spinner.update();
        if self.session.thinking_spinner.is_active {
            self.session.needs_redraw = true;
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

        // Compute responsive layout
        let layout = self.compute_layout(area, mode);

        // Update header rows if changed
        if layout.header.height != self.session.header_rows {
            self.session.header_rows = layout.header.height;
            crate::ui::tui::session::render::recalculate_transcript_rows(self.session);
        }

        // Update view rows for transcript
        apply_view_rows(self.session, layout.main.height);

        // Check if overlays are active (dim background panels when true)
        let _overlays_active =
            self.session.file_palette_active || self.session.prompt_palette_active;

        // Render header
        let header_lines = self
            .header_lines
            .as_ref()
            .unwrap_or(&self.session.header_lines())
            .clone();
        HeaderWidget::new(self.session)
            .lines(header_lines)
            .render(layout.header, buf);

        // Render main content area (transcript + optional logs)
        let has_logs = self.session.show_logs && self.session.has_logs() && mode.show_logs_panel();

        if has_logs {
            let chunks = Layout::vertical([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(layout.main);
            TranscriptWidget::new(self.session).render(chunks[0], buf);
            self.render_logs(chunks[1], buf, mode);
        } else {
            TranscriptWidget::new(self.session).render(layout.main, buf);
        }

        // Render sidebar in wide mode
        if let Some(sidebar_area) = layout.sidebar {
            self.render_sidebar(sidebar_area, buf, mode);
        }

        // Render footer only in wide mode (preserves transcript space in smaller terminals)
        if mode.show_footer() && layout.footer.height > 0 {
            self.render_footer(layout.footer, buf, mode);
        }

        // Render overlays (modals, palettes, etc.)
        self.render_overlays(area, buf);
    }
}

impl<'a> SessionWidget<'a> {
    fn render_logs(&mut self, area: Rect, buf: &mut Buffer, mode: LayoutMode) {
        use ratatui::widgets::{Paragraph, Wrap};

        let inner = Panel::new(&self.session.styles)
            .title("Logs")
            .active(false)
            .mode(mode)
            .render_and_get_inner(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let paragraph =
            Paragraph::new((*self.session.log_text()).clone()).wrap(Wrap { trim: false });
        paragraph.render(inner, buf);
    }

    fn render_sidebar(&mut self, area: Rect, buf: &mut Buffer, mode: LayoutMode) {
        let queue_items: Vec<String> = self
            .session
            .queued_inputs
            .iter()
            .take(5)
            .map(|input| {
                let preview: String = input.chars().take(50).collect();
                if input.len() > 50 {
                    format!("{}...", preview)
                } else {
                    preview
                }
            })
            .collect();

        let context_info = self
            .session
            .input_status_right
            .as_deref()
            .unwrap_or("Ready");

        SidebarWidget::new(&self.session.styles)
            .queue_items(queue_items)
            .context_info(context_info)
            .mode(mode)
            .render(area, buf);
    }

    fn render_footer(&mut self, area: Rect, buf: &mut Buffer, mode: LayoutMode) {
        let left_status = self.session.input_status_left.as_deref().unwrap_or("");
        let right_status = self.session.input_status_right.as_deref().unwrap_or("");

        let hint = if self.session.thinking_spinner.is_active {
            footer_hints::PROCESSING
        } else if self.session.file_palette_active || self.session.prompt_palette_active {
            footer_hints::MODAL
        } else if self.session.input_manager.content().is_empty() {
            footer_hints::IDLE
        } else {
            footer_hints::EDITING
        };

        let spinner = if self.session.thinking_spinner.is_active {
            Some(self.session.thinking_spinner.current_frame())
        } else {
            None
        };

        let mut footer = FooterWidget::new(&self.session.styles)
            .left_status(left_status)
            .right_status(right_status)
            .hint(hint)
            .mode(mode);

        if let Some(spinner_frame) = spinner {
            footer = footer.spinner(spinner_frame);
        }

        footer.render(area, buf);
    }

    fn render_overlays(&mut self, viewport: Rect, buf: &mut Buffer) {
        // Render file palette using builder pattern
        if self.session.file_palette_active
            && let Some(palette) = self.session.file_palette.as_ref()
        {
            FilePaletteWidget::new(self.session, palette, viewport).render(viewport, buf);
        }

        // Render prompt palette using builder pattern
        if self.session.prompt_palette_active
            && let Some(palette) = self.session.prompt_palette.as_ref()
        {
            PromptPaletteWidget::new(self.session, palette, viewport).render(viewport, buf);
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
