use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    widgets::Widget,
};

use super::{FooterWidget, HeaderWidget, LayoutMode, SidebarWidget, TranscriptWidget, footer_hints, panel::new_panel};
use crate::tui::core_tui::blocked_status;
use crate::tui::ui::tui::session::{Session, pulse_spinner_frame_for_phase};

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
    header_lines: Option<std::sync::Arc<Vec<ratatui::text::Line<'static>>>>,
    header_area: Option<Rect>,
    transcript_area: Option<Rect>,
    navigation_area: Option<Rect>,
    layout_mode: Option<LayoutMode>,
    footer_hint_override: Option<&'static str>,
}

impl<'a> SessionWidget<'a> {
    /// Create a new SessionWidget with required parameters
    pub(crate) fn new(session: &'a mut Session) -> Self {
        Self {
            session,
            header_lines: None,
            header_area: None,
            transcript_area: None,
            navigation_area: None,
            layout_mode: None,
            footer_hint_override: None,
        }
    }

    /// Set the header lines to render
    #[must_use]
    pub(crate) fn header_lines(mut self, lines: Vec<ratatui::text::Line<'static>>) -> Self {
        self.header_lines = Some(std::sync::Arc::new(lines));
        self
    }

    /// Set pre-built header lines without cloning the `Vec`.
    #[must_use]
    pub(crate) fn header_lines_arc(mut self, lines: std::sync::Arc<Vec<ratatui::text::Line<'static>>>) -> Self {
        self.header_lines = Some(lines);
        self
    }

    /// Set the header area
    #[must_use]
    pub(crate) fn header_area(mut self, area: Rect) -> Self {
        self.header_area = Some(area);
        self
    }

    /// Set the transcript area
    #[must_use]
    pub(crate) fn transcript_area(mut self, area: Rect) -> Self {
        self.transcript_area = Some(area);
        self
    }

    /// Set the navigation area
    #[must_use]
    pub(crate) fn navigation_area(mut self, area: Rect) -> Self {
        self.navigation_area = Some(area);
        self
    }

    /// Override the layout mode (auto-detected by default)
    #[must_use]
    pub fn layout_mode(mut self, mode: LayoutMode) -> Self {
        self.layout_mode = Some(mode);
        self
    }

    /// Override the footer hint text (useful for app-specific panels).
    #[must_use]
    pub fn footer_hint_override(mut self, hint: &'static str) -> Self {
        self.footer_hint_override = Some(hint);
        self
    }

    /// Compute the layout regions based on viewport and layout mode
    /// Compute the layout regions based on viewport and layout mode
    fn compute_layout(&mut self, area: Rect, mode: LayoutMode) -> SessionLayout {
        let footer_h = mode.footer_height();
        let max_header_pct = mode.max_header_percent();

        // Compute header height (no Vec clone — borrow the Arc slice)
        let header_lines = if let Some(lines) = self.header_lines.as_ref() {
            std::sync::Arc::clone(lines)
        } else {
            self.session.header_lines()
        };

        let natural_header_h = self.session.header_height_from_lines(area.width, header_lines.as_slice());
        let max_header_h = ((area.height as f32) * max_header_pct) as u16;
        let header_h = natural_header_h.min(max_header_h).max(1);

        // Main region constraints
        let main_h = area.height.saturating_sub(header_h + footer_h);

        let [header_area, main_area, footer_area] = area
            .try_layout(&Layout::vertical([
                Constraint::Length(header_h),
                Constraint::Length(main_h),
                Constraint::Length(footer_h),
            ]))
            .unwrap_or([Rect::ZERO; 3]);

        // In wide mode, split main into transcript and sidebar
        // Respect appearance config for sidebar visibility
        let show_sidebar = mode.allow_sidebar() && self.session.appearance.should_show_sidebar();
        if show_sidebar {
            let sidebar_pct = mode.sidebar_width_percent();
            let [left, right] = main_area
                .try_layout(&Layout::horizontal([
                    Constraint::Percentage(100 - sidebar_pct),
                    Constraint::Percentage(sidebar_pct),
                ]))
                .unwrap_or([main_area; 2]);
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
    #[expect(
        dead_code,
        reason = "Intentional compatibility, platform, test, or API-shape suppression."
    )]
    mode: LayoutMode,
}

impl Widget for &mut SessionWidget<'_> {
    #[cfg_attr(feature = "profiling", hotpath::measure)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.session.set_progress_area(None);
        if area.width == 0 || area.height == 0 {
            return;
        }

        // Determine layout mode from viewport or override
        let mode = self.layout_mode.unwrap_or_else(|| self.session.resolved_layout_mode(area));

        if let (Some(header_area), Some(transcript_area)) = (self.header_area, self.transcript_area) {
            self.session.poll_log_entries();

            if header_area.width > 0 && header_area.height > 0 {
                let header_lines = if let Some(lines) = self.header_lines.as_ref() {
                    std::sync::Arc::clone(lines)
                } else {
                    self.session.header_lines()
                };
                HeaderWidget::new(self.session).lines(header_lines).render(header_area, buf);
            }

            if transcript_area.width > 0 && transcript_area.height > 0 {
                // The explicit layout path already supplies the measured
                // transcript rectangle. Updating `view_rows` here would
                // temporarily recalculate `transcript_rows` from that
                // rectangle as if it were the full viewport, which can make
                // scroll-anchor restoration observe the wrong geometry.
                let has_logs = self.session.show_logs && self.session.has_logs() && mode.show_logs_panel();
                if has_logs {
                    let [transcript, logs] = transcript_area
                        .try_layout(&Layout::vertical([Constraint::Percentage(70), Constraint::Percentage(30)]))
                        .unwrap_or([transcript_area, Rect::ZERO]);
                    TranscriptWidget::new(self.session).render(transcript, buf);
                    self.render_logs(logs, buf, mode);
                } else {
                    TranscriptWidget::new(self.session).render(transcript_area, buf);
                }
            } else {
                // Clear the stored hit-test area when a constrained layout leaves
                // no room for the transcript (for example, beneath a tiny modal).
                TranscriptWidget::new(self.session).render(transcript_area, buf);
            }

            if let Some(sidebar_area) = self.navigation_area
                && sidebar_area.width > 0
                && sidebar_area.height > 0
            {
                self.render_sidebar(sidebar_area, buf, mode);
            }
            return;
        }

        // Reserve input height so transcript/header never render under input
        let layout_height = area.height.saturating_sub(self.session.input_height);
        let layout_area = Rect::new(area.x, area.y, area.width, layout_height);
        if layout_area.height == 0 || layout_area.width == 0 {
            return;
        }

        // Pull log entries
        self.session.poll_log_entries();

        // Compute responsive layout
        let layout = self.compute_layout(layout_area, mode);

        // Update header rows if changed
        if layout.header.height != self.session.header_rows {
            self.session.header_rows = layout.header.height;
            self.session.recalculate_transcript_rows();
        }

        // Update view rows for transcript
        self.session.apply_view_rows(layout.main.height);

        // Render header
        let header_lines = if let Some(lines) = self.header_lines.as_ref() {
            std::sync::Arc::clone(lines)
        } else {
            self.session.header_lines()
        };
        HeaderWidget::new(self.session).lines(header_lines).render(layout.header, buf);

        // Render main content area (transcript + optional logs)
        let has_logs = self.session.show_logs && self.session.has_logs() && mode.show_logs_panel();

        if has_logs {
            let [transcript, logs] = layout
                .main
                .try_layout(&Layout::vertical([Constraint::Percentage(70), Constraint::Percentage(30)]))
                .unwrap_or([layout.main, Rect::ZERO]);
            TranscriptWidget::new(self.session).render(transcript, buf);
            self.render_logs(logs, buf, mode);
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
    }
}

impl<'a> SessionWidget<'a> {
    fn render_logs(&mut self, area: Rect, buf: &mut Buffer, mode: LayoutMode) {
        use ratatui::widgets::{Paragraph, Wrap};

        let inner = new_panel(&self.session.styles)
            .title("Logs")
            .active(false)
            .mode(mode)
            .render_and_get_inner(area, buf);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let paragraph = Paragraph::new((*self.session.log_text()).clone()).wrap(Wrap { trim: false });
        paragraph.render(inner, buf);
    }

    fn render_sidebar(&mut self, area: Rect, buf: &mut Buffer, mode: LayoutMode) {
        let queue_items: Vec<String> = if let Some(cached) = &self.session.queued_inputs_preview_cache {
            cached.clone()
        } else {
            let items: Vec<String> = self
                .session
                .queued_inputs
                .iter()
                .take(5)
                .map(|input| {
                    if input.chars().count() > 50 {
                        let preview: String = input.chars().take(50).collect();
                        format!("{preview}{}", crate::design::constants::ELLIPSIS)
                    } else {
                        input.clone()
                    }
                })
                .collect();
            self.session.queued_inputs_preview_cache = Some(items.clone());
            items
        };

        let context_info = self.session.input_status_right.as_deref().unwrap_or("Ready");

        SidebarWidget::new(&self.session.styles)
            .local_agents(self.session.local_agents.clone())
            .queue_items(queue_items)
            .context_info(context_info)
            .mode(mode)
            .render(area, buf);
    }

    fn render_footer(&mut self, area: Rect, buf: &mut Buffer, mode: LayoutMode) {
        let left_status = if self.session.progress_row_visible() {
            self.session.progress_footer_status_text()
        } else {
            self.session
                .status_left_text()
                .filter(|status| !status.trim_start().starts_with("git:"))
        }
        .unwrap_or_else(|| {
            if self.session.thinking_spinner.is_active && !self.session.progress_row_visible() {
                "Thinking"
            } else {
                ""
            }
        });
        let right_status = self.session.status_right_text().unwrap_or("");

        // Blocked/recovery detection combines the first-class activity state
        // with the status-text needles shared with the header badge (see
        // blocked_status) so a reword cannot silently break the hint.
        let is_blocked = matches!(self.session.activity_state, vtcode_commons::ui_protocol::ActivityState::Blocked)
            || blocked_status::right_status_is_blocked(right_status)
            || blocked_status::left_status_is_blocked(left_status)
            || blocked_status::left_status_mentions_tools_disabled(left_status);
        let is_recovery = matches!(self.session.activity_state, vtcode_commons::ui_protocol::ActivityState::Recovery)
            || blocked_status::left_status_is_recovery(left_status);
        let hint = if let Some(hint) = self.footer_hint_override {
            hint
        } else if is_blocked {
            footer_hints::BLOCKED
        } else if is_recovery {
            footer_hints::RECOVERY
        } else if self.session.thinking_spinner.is_active
            || self.session.has_status_spinner()
            || self.session.is_running_activity()
        {
            footer_hints::PROCESSING
        } else if self.session.has_active_overlay() {
            footer_hints::MODAL
        } else if self.session.input_manager.content().is_empty() {
            footer_hints::IDLE
        } else {
            footer_hints::EDITING
        };

        let shimmer_phase = footer_shimmer_phase(self.session);

        let mut footer = FooterWidget::new(&self.session.styles)
            .left_status(left_status)
            .right_status(right_status)
            .hint(hint)
            .mode(mode);

        if self.session.appearance.should_animate_progress_status()
            && self.session.thinking_spinner.is_active
            && !self.session.progress_row_visible()
        {
            footer = footer.spinner(self.session.thinking_spinner.current_frame());
        } else if self.session.appearance.should_animate_progress_status()
            && self.session.activity_state.is_stage()
            && self.session.is_shimmer_active()
            && !self.session.progress_row_visible()
        {
            footer = footer.spinner(pulse_spinner_frame_for_phase(self.session.shimmer_state.phase()));
        }

        if let Some(phase) = shimmer_phase {
            footer = footer.shimmer_phase(phase);
        }

        footer.render(area, buf);
    }
}

fn footer_shimmer_phase(session: &Session) -> Option<f32> {
    (session.appearance.should_animate_progress_status() && session.is_shimmer_active())
        .then_some(session.shimmer_state.phase())
}

#[expect(
    dead_code,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
fn has_input_status(session: &Session) -> bool {
    let left_present = session.input_status_left.as_ref().is_some_and(|value| !value.trim().is_empty());
    if left_present {
        return true;
    }
    session
        .input_status_right
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::core_tui::types::{InlineCommand, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme};
    use std::sync::Arc;

    fn segment(text: &str) -> InlineSegment {
        InlineSegment {
            text: text.to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }
    }

    #[test]
    fn compatibility_footer_keeps_git_status_while_transcript_owns_progress() {
        use vtcode_commons::ui_protocol::{ActivityState, ProgressOperation, ProgressPhase, ProgressUpdate};

        let mut session = Session::new(InlineTheme::default(), None, 24);
        session.handle_command(InlineCommand::SetConfiguredInputStatus {
            left: Some("topic/footer*".to_owned()),
            right: Some("10:30".to_owned()),
        });
        session.handle_command(InlineCommand::SetActivityState(ActivityState::Building));
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: ProgressOperation::start(),
            phase: ProgressPhase::WaitingForModel,
        }));
        let area = Rect::new(0, 0, 120, 24);
        let mut buf = Buffer::empty(area);
        session.render_progress(Rect::new(0, 0, 120, 1), &mut buf);
        SessionWidget::new(&mut session).render_footer(Rect::new(0, 1, 120, 4), &mut buf, LayoutMode::Wide);
        let text: String = buf.content.iter().map(|cell| cell.symbol()).collect();
        assert_eq!(text.matches("Waiting for model").count(), 1, "{text}");
        assert!(text.contains("topic/footer*"), "{text}");
        assert!(text.contains("10:30"), "{text}");
        assert!(!text.contains("Building..."), "{text}");
    }

    #[test]
    fn auto_layout_resize_recomputes_transcript_area_and_keeps_content_visible() {
        let wide_area = Rect::new(0, 0, 120, 24);
        let standard_area = Rect::new(0, 0, 100, 24);
        let mut wide_buf = Buffer::empty(wide_area);
        let mut standard_buf = Buffer::empty(standard_area);
        let mut session = Session::new(InlineTheme::default(), None, 24);

        for index in 0..8 {
            session.push_line(InlineMessageKind::Agent, vec![segment(&format!("line {index}"))]);
        }

        let mut wide_widget = SessionWidget::new(&mut session);
        (&mut wide_widget).render(wide_area, &mut wide_buf);
        let wide_transcript = session.transcript_area().expect("wide transcript area");

        let mut standard_widget = SessionWidget::new(&mut session);
        (&mut standard_widget).render(standard_area, &mut standard_buf);
        let standard_transcript = session.transcript_area().expect("standard transcript area");

        assert!(wide_transcript.width < standard_transcript.width);
        assert!(standard_transcript.height > 0);
    }

    #[test]
    fn footer_shimmer_respects_reduce_motion_override_and_screen_reader_mode() {
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.handle_command(InlineCommand::SetInputStatus {
            left: Some("Running tool: edit_file".to_string()),
            right: None,
        });
        session.thinking_spinner.start();

        assert!(footer_shimmer_phase(&session).is_some());

        session.appearance.reduce_motion_mode = true;
        assert!(session.is_shimmer_active(), "thinking indicator keeps the shared phase active");
        assert_eq!(footer_shimmer_phase(&session), None);

        session.appearance.reduce_motion_keep_progress_animation = true;
        assert!(footer_shimmer_phase(&session).is_some());

        session.appearance.screen_reader_mode = true;
        assert_eq!(footer_shimmer_phase(&session), None);
    }
    #[test]
    fn thinking_surfaces_respect_accessibility_policy_and_preserve_labels() {
        for (reduce_motion, keep_animation, screen_reader, animated) in [
            (false, false, false, true),
            (true, false, false, false),
            (true, true, false, true),
            (false, true, true, false),
            (true, true, true, false),
        ] {
            for configured in [None, Some("Custom status")] {
                let mut session = Session::new(InlineTheme::default(), None, 12);
                session.input_status_left = configured.map(str::to_string);
                session.appearance.reduce_motion_mode = reduce_motion;
                session.appearance.reduce_motion_keep_progress_animation = keep_animation;
                session.appearance.screen_reader_mode = screen_reader;
                session.thinking_spinner.start();
                let area = Rect::new(0, 0, 100, 2);
                let mut buf = Buffer::empty(area);
                SessionWidget::new(&mut session).render_footer(area, &mut buf, LayoutMode::Standard);
                let footer: String = buf.content.iter().map(|cell| cell.symbol()).collect();
                let composer = session
                    .render_input_status_line(100)
                    .unwrap()
                    .spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>();
                for (text, expected_spinner) in [(&footer, animated), (&composer, animated && configured.is_none())] {
                    assert!(text.contains(configured.unwrap_or("Thinking")), "{text}");
                    let has_braille = text.chars().any(|ch| ('\u{2800}'..='\u{28ff}').contains(&ch));
                    assert_eq!(has_braille, expected_spinner);
                }
            }
        }
    }
}
