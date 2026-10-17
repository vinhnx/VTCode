use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget},
};

use crate::tui::config::constants::ui;
use crate::tui::ui::tui::session::{Session, TranscriptLine};
use tui_shimmer::shimmer_spans_with_style_at_phase;
use vtcode_config::constants::tools;

/// Widget for rendering the transcript area with conversation history
///
/// This widget handles:
/// - Scroll viewport management
/// - Content caching and optimization
/// - Text wrapping and overflow
/// - Queue overlay rendering
///
/// # Example
/// ```ignore
/// TranscriptWidget::new(session)
///     .show_scrollbar(true)
///     .custom_style(style)
///     .render(area, buf);
/// ```
pub struct TranscriptWidget<'a> {
    session: &'a mut Session,
    show_scrollbar: bool,
    custom_style: Option<Style>,
}

impl<'a> TranscriptWidget<'a> {
    /// Create a new TranscriptWidget with required parameters
    pub fn new(session: &'a mut Session) -> Self {
        Self { session, show_scrollbar: false, custom_style: None }
    }

    /// Enable or disable scrollbar rendering
    #[must_use]
    pub fn show_scrollbar(mut self, show: bool) -> Self {
        self.show_scrollbar = show;
        self
    }

    /// Set a custom style for the transcript
    #[must_use]
    pub fn custom_style(mut self, style: Style) -> Self {
        self.custom_style = Some(style);
        self
    }
}

impl<'a> Widget for TranscriptWidget<'a> {
    #[cfg_attr(feature = "profiling", hotpath::measure)]
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.session.set_progress_area(None);
        if area.height == 0 || area.width == 0 {
            self.session.set_transcript_area(None);
            self.session.clear_transcript_file_link_targets();
            return;
        }

        let mut inner = transcript_content_area(area);
        if self.session.progress.is_active() && inner.height > 0 && inner.width > 0 {
            let progress_area = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
            self.session.render_progress(progress_area, buf);
            inner.height = inner.height.saturating_sub(1);
        }

        if inner.height == 0 || inner.width == 0 {
            self.session.set_transcript_area(None);
            self.session.clear_transcript_file_link_targets();
            return;
        }
        let layout = self.session.layout_sticky_prompt(inner);
        let scroll_area = layout.body;
        let content_width = scroll_area.width;
        let viewport_rows = usize::from(scroll_area.height);
        let visible_start = layout.source_row;
        if let Some(header) = layout.header {
            let header_area = Rect::new(inner.x, inner.y, inner.width, 1);
            let style = self.session.styles.sticky_prompt_style();
            Clear.render(header_area, buf);
            Paragraph::new(header).style(style).render(header_area, buf);
        }

        // Use cached visible lines to avoid rebuilding on every frame
        let cached_lines = self
            .session
            .collect_transcript_window_cached(content_width, visible_start, viewport_rows);

        // Check if we need to mutate the lines (queue overlay). Bottom padding
        // rows need no mutation — the buffer is already default-styled empty
        // space, so padding the line list only forced a per-frame clone.
        let needs_mutation = !self.session.queued_inputs.is_empty();

        // Fast path: no queue overlay, no live indicator shimmer, and no
        // explicit links — paint cached rows in place without cloning Lines
        // into Paragraph (hotpath: that clone was ~15KB/frame).
        let has_links = cached_lines.iter().any(|line| !line.explicit_links.is_empty());
        let spinner_active = active_indicator_shimmer_phase(self.session).is_some();
        if !needs_mutation && !has_links && !spinner_active {
            self.session.clear_transcript_file_link_targets();
            if self.session.transcript_clear_required {
                Clear.render(scroll_area, buf);
                self.session.transcript_clear_required = false;
            }
            let default_style = self.session.styles.default_style();
            let default_bg = default_style.bg;
            paint_pre_wrapped_lines(cached_lines.as_slice(), scroll_area, buf, default_style);
            apply_borrowed_line_backgrounds(buf, scroll_area, cached_lines.as_slice(), default_bg);
            clear_transcript_gutters(area, inner, default_style, buf);
            return;
        }

        let mut visible_lines = if needs_mutation {
            // Need to mutate (queue overlay), so clone, pad to the viewport
            // (overlay paints at the bottom), and modify.
            let mut lines = cached_lines.to_vec();
            let fill_count = viewport_rows.saturating_sub(lines.len());
            if fill_count > 0 {
                let target_len = lines.len() + fill_count;
                lines.resize_with(target_len, TranscriptLine::default);
            }
            self.session.overlay_queue_lines(&mut lines, content_width);
            self.session.decorate_visible_cached_transcript_links(lines, scroll_area)
        } else {
            self.session
                .decorate_borrowed_cached_transcript_links(cached_lines.as_slice(), scroll_area)
        };
        apply_active_file_operation_spinner(self.session, &mut visible_lines);

        // Only clear if content actually changed, not on viewport-only scroll
        // This is a significant optimization: avoids expensive Clear operation on most scrolls
        if self.session.transcript_clear_required {
            Clear.render(scroll_area, buf);
            self.session.transcript_clear_required = false;
        }
        // Paint full-width line tints AFTER Paragraph. Paragraph::render
        // first fills the whole area with `default_style` (terminal bg),
        // which would wipe a pre-painted band on cells past the line text.
        // Precompute per-row tints so the owned lines can move into Paragraph
        // without a second clone.
        let default_style = self.session.styles.default_style();
        let default_bg = default_style.bg;
        let row_tints: Vec<Option<Color>> = visible_lines.iter().map(line_background).collect();
        // Lines are already wrapped to `content_width` == `scroll_area.width` in
        // reflow. Re-wrapping in Paragraph every frame was the dominant
        // steady-state render cost (hotpath: ~21KB/frame).
        let paragraph = Paragraph::new(visible_lines).style(default_style);
        paragraph.render(scroll_area, buf);
        apply_precomputed_line_backgrounds(buf, scroll_area, &row_tints, default_bg);
        clear_transcript_gutters(area, inner, default_style, buf);
    }
}

fn transcript_content_area(area: Rect) -> Rect {
    let gutter = transcript_horizontal_gutter(area.width);
    Rect::new(
        area.x.saturating_add(gutter),
        area.y,
        area.width.saturating_sub(gutter.saturating_mul(2)),
        area.height,
    )
}

fn transcript_horizontal_gutter(width: u16) -> u16 {
    if width >= ui::INLINE_TRANSCRIPT_WIDE_GUTTER_MIN_WIDTH {
        ui::INLINE_TRANSCRIPT_WIDE_GUTTER_COLUMNS
    } else if width >= ui::INLINE_TRANSCRIPT_STANDARD_GUTTER_MIN_WIDTH {
        ui::INLINE_TRANSCRIPT_STANDARD_GUTTER_COLUMNS
    } else {
        0
    }
}

fn clear_transcript_gutters(area: Rect, content_area: Rect, style: Style, buf: &mut Buffer) {
    let left_width = content_area.x.saturating_sub(area.x);
    if left_width > 0 {
        let left = Rect::new(area.x, area.y, left_width, area.height);
        Clear.render(left, buf);
        buf.set_style(left, style);
    }

    let right_x = content_area.right();
    let right_width = area.right().saturating_sub(right_x);
    if right_width > 0 {
        let right = Rect::new(right_x, area.y, right_width, area.height);
        Clear.render(right, buf);
        buf.set_style(right, style);
    }
}

/// Paint pre-wrapped transcript rows by writing spans directly into the buffer.
/// Avoids cloning `Line`s into `Paragraph` on the common no-link path.
/// Span styles patch `default_style` (same merge order as Paragraph).
fn paint_pre_wrapped_lines(lines: &[TranscriptLine], area: Rect, buf: &mut Buffer, default_style: Style) {
    buf.set_style(area, default_style);
    let max_rows = usize::from(area.height).min(lines.len());
    for (row, transcript_line) in lines.iter().take(max_rows).enumerate() {
        let y = area.y + row as u16;
        let mut x = area.x;
        for span in &transcript_line.line.spans {
            if x >= area.right() {
                break;
            }
            let remaining = area.right().saturating_sub(x);
            if remaining == 0 {
                break;
            }
            let merged = default_style.patch(span.style);
            let (end_x, _end_y) = buf.set_stringn(x, y, span.content.as_ref(), remaining as usize, merged);
            x = end_x;
        }
    }
}

/// Fill untinted cells on a diff row using `line_background` from borrowed rows.
fn apply_borrowed_line_backgrounds(buf: &mut Buffer, area: Rect, lines: &[TranscriptLine], default_bg: Option<Color>) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let max_rows = usize::from(area.height).min(lines.len());
    for (row, transcript_line) in lines.iter().take(max_rows).enumerate() {
        let Some(bg) = line_background(&transcript_line.line) else {
            continue;
        };
        let y = area.y + row as u16;
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            if cell.bg == Color::Reset || Some(cell.bg) == default_bg {
                cell.bg = bg;
            }
        }
    }
}

const FILE_OPERATION_STATUS_TOOLS: &[&str] = &[
    tools::WRITE_FILE,
    tools::CREATE_FILE,
    tools::EDIT_FILE,
    tools::APPLY_PATCH,
    tools::SEARCH_REPLACE,
    tools::DELETE_FILE,
    tools::UNIFIED_FILE,
];

const FILE_OPERATION_INDICATORS: &[&str] = &[
    "❋ Writing ",
    "❋ Editing ",
    "❋ Applying patch to ",
    "❋ Search/replace in ",
    "❋ Deleting ",
    "❋ Drafting plan",
    "❋ Validating plan",
    "❋ Persisting plan",
    "❋ Preparing approval",
];

fn apply_active_file_operation_spinner(session: &Session, lines: &mut [Line<'static>]) {
    let Some(phase) = active_indicator_shimmer_phase(session) else {
        return;
    };

    // Sweep a shimmer across the whole indicator line (icon + verb + target)
    // at the shared shimmer phase, so the transcript pulses in sync with the
    // footer. Only the newest indicator row animates and only while its work
    // is live; the cached transcript is never mutated, so the static `❋`
    // row (with its links/colors) returns untouched when activity ends.
    for line in lines.iter_mut().rev() {
        if !is_file_operation_indicator_line(line) {
            continue;
        }
        let text = line.spans.iter().map(|span| span.content.as_ref()).collect::<String>();
        let base_style = line.spans.first().map(|span| span.style).unwrap_or_default();
        line.spans = shimmer_spans_with_style_at_phase(&text, base_style, phase);
        break;
    }
}

/// Shimmer phase for the active transcript indicator row, if any.
///
/// Returns `None` when animation is suppressed (reduced motion, screen
/// reader) or nothing is live, so idle frames skip the span rebuild
/// entirely. File tools key off `Running tool: <name>`; planning phases key
/// off their footer statuses (`Drafting/Validating/Persisting plan...`,
/// `Preparing approval...`), which the runloop keeps live while the long
/// synthesis runs.
fn active_indicator_shimmer_phase(session: &Session) -> Option<f32> {
    if !session.appearance.should_animate_progress_status() {
        return None;
    }

    let left = session.input_status_left.as_deref()?.to_ascii_lowercase();
    if let Some(tool_name) = left.strip_prefix("running tool: ") {
        return FILE_OPERATION_STATUS_TOOLS
            .contains(&tool_name)
            .then(|| session.shimmer_state.phase());
    }

    let is_planning_status = left.contains("drafting plan")
        || left.contains("validating plan")
        || left.contains("persisting plan")
        || left.contains("preparing approval");
    is_planning_status.then(|| session.shimmer_state.phase())
}

fn is_file_operation_indicator_line(line: &Line<'_>) -> bool {
    let text = line.spans.iter().map(|span| span.content.as_ref()).collect::<String>();
    FILE_OPERATION_INDICATORS.iter().any(|pattern| text.contains(pattern))
}

/// Full-row tint for a diff line.
///
/// Uses the marker/gutter background rather than a changed-word chip so a row
/// with several syntax spans does not extend the stronger chip across padding.
///
/// Returns `None` for side-by-side rows — those mix coloured panes with an
/// uncoloured sibling (or two different colours), and full-width fill would
/// paint the empty pane with the other side's tint.
fn line_background(line: &Line<'_>) -> Option<Color> {
    // Fast reject: plain prose has no tinted spans and no side-by-side divider.
    if !line.spans.iter().any(|span| span.style.bg.is_some()) {
        return None;
    }
    let mut first_background = None;
    let mut marker_background = None;
    let mut has_uncolored_divider = false;
    for span in &line.spans {
        match span.style.bg {
            Some(Color::Reset) if span.content.trim() == "│" => has_uncolored_divider = true,
            Some(Color::Reset) => {}
            Some(bg) => {
                first_background.get_or_insert(bg);
                if marker_background.is_none() && matches!(span.content.chars().next(), Some('+' | '-')) {
                    marker_background = Some(bg);
                }
            }
            None if span.content.trim() == "│" => has_uncolored_divider = true,
            None => {}
        }
    }
    // Side-by-side rows carry an uncoloured divider. Full-width fill would
    // bleed one pane's tint into the other. Do not inspect every later span
    // for a marker: a changed word can legitimately begin with `+` or `-`
    // (for example `--last`) without turning a unified row into two panes.
    if has_uncolored_divider {
        return None;
    }
    marker_background.or(first_background)
}

/// Fill untinted cells on a diff row with the line tint.
///
/// Only cells still on the terminal default background are painted. Word-chip
/// cells (stronger red/green) must keep their colour so the two-level band
/// survives the full-width fill.
fn apply_precomputed_line_backgrounds(
    buf: &mut Buffer,
    area: Rect,
    row_tints: &[Option<Color>],
    default_bg: Option<Color>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let max_rows = usize::from(area.height).min(row_tints.len());
    for (row, bg) in row_tints.iter().take(max_rows).enumerate() {
        let Some(bg) = bg else {
            continue;
        };
        let y = area.y + row as u16;
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            // Reset / terminal default → line tint. Any explicit paint
            // (word chip, already-tinted span, etc.) is left alone.
            if cell.bg == Color::Reset || Some(cell.bg) == default_bg {
                cell.bg = *bg;
            }
        }
    }
}

/// Fill untinted cells on a diff row with the line tint (test helper).
#[cfg(test)]
fn apply_full_width_line_backgrounds(buf: &mut Buffer, area: Rect, lines: &[Line<'_>], default_bg: Option<Color>) {
    let row_tints: Vec<Option<Color>> = lines.iter().map(line_background).collect();
    apply_precomputed_line_backgrounds(buf, area, &row_tints, default_bg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::core_tui::types::{InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme};
    use std::sync::Arc;

    fn segment(text: &str) -> InlineSegment {
        InlineSegment {
            text: text.to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }
    }

    #[test]
    fn full_width_diff_tint_survives_paragraph_default_bg() {
        use ratatui::style::Style as RatStyle;
        use ratatui::text::{Line as RatLine, Span as RatSpan};

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        // Simulate Paragraph painting the whole area with the terminal default.
        buf.set_style(area, RatStyle::default().bg(Color::Black));
        let line = RatLine::from(vec![RatSpan::styled("+ hi", RatStyle::default().bg(Color::Rgb(20, 58, 45)))]);
        let lines = vec![line];
        // Correct order: Paragraph (already simulated) then full-width tint.
        apply_full_width_line_backgrounds(&mut buf, area, &lines, Some(Color::Black));

        let bg = Color::Rgb(20, 58, 45);
        assert_eq!(buf[(0, 0)].bg, bg, "left edge must keep the tint");
        assert_eq!(buf[(19, 0)].bg, bg, "right edge must be full-width tinted");
    }

    #[test]
    fn full_width_fill_preserves_word_chip_backgrounds() {
        use ratatui::style::Style as RatStyle;
        use ratatui::text::{Line as RatLine, Span as RatSpan};

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let line_bg = Color::Rgb(20, 58, 45);
        let chip_bg = Color::Rgb(36, 100, 70);
        buf.set_style(area, RatStyle::default().bg(Color::Black));
        // Simulate Paragraph writing: line tint on part, chip on "words".
        buf[(3, 0)].bg = chip_bg;
        buf[(4, 0)].bg = chip_bg;
        buf[(5, 0)].bg = line_bg;

        let line = RatLine::from(vec![RatSpan::styled("ab", RatStyle::default().bg(line_bg))]);
        apply_full_width_line_backgrounds(&mut buf, area, &[line], Some(Color::Black));

        assert_eq!(buf[(3, 0)].bg, chip_bg, "word chip must not be overwritten");
        assert_eq!(buf[(4, 0)].bg, chip_bg, "word chip must not be overwritten");
        assert_eq!(buf[(19, 0)].bg, line_bg, "empty cells fill with line tint");
        assert_eq!(buf[(0, 0)].bg, line_bg);
    }

    #[test]
    fn full_width_fill_uses_the_row_tint_when_word_chips_are_present() {
        use ratatui::style::Style as RatStyle;
        use ratatui::text::{Line as RatLine, Span as RatSpan};

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let line_bg = Color::Rgb(20, 58, 45);
        let chip_bg = Color::Rgb(36, 100, 70);
        buf.set_style(area, RatStyle::default().bg(Color::Black));
        for x in 1..8 {
            buf[(x, 0)].bg = chip_bg;
        }
        let line = RatLine::from(vec![
            RatSpan::styled("+", RatStyle::default().bg(line_bg)),
            RatSpan::styled("changed", RatStyle::default().bg(chip_bg)),
            RatSpan::styled(" ", RatStyle::default().bg(line_bg)),
        ]);

        apply_full_width_line_backgrounds(&mut buf, area, &[line], Some(Color::Black));

        assert_eq!(buf[(1, 0)].bg, chip_bg, "word chip must remain stronger");
        assert_eq!(buf[(19, 0)].bg, line_bg, "row tint must fill after word chip");
    }

    #[test]
    fn full_width_fill_uses_the_marker_tint_when_syntax_chips_outnumber_it() {
        use ratatui::style::Style as RatStyle;
        use ratatui::text::{Line as RatLine, Span as RatSpan};

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let line_bg = Color::Rgb(20, 58, 45);
        let chip_bg = Color::Rgb(36, 100, 70);
        buf.set_style(area, RatStyle::default().bg(Color::Black));
        let line = RatLine::from(vec![
            RatSpan::styled("+", RatStyle::default().bg(line_bg)),
            RatSpan::styled("changed", RatStyle::default().bg(chip_bg)),
            RatSpan::styled(" syntax", RatStyle::default().bg(chip_bg)),
            RatSpan::styled(" tokens", RatStyle::default().bg(chip_bg)),
        ]);

        apply_full_width_line_backgrounds(&mut buf, area, &[line], Some(Color::Black));

        assert_eq!(buf[(19, 0)].bg, line_bg, "padding must use the marker tint");
    }

    #[test]
    fn full_width_fill_does_not_misclassify_dash_prefixed_word_chip() {
        use ratatui::style::Style as RatStyle;
        use ratatui::text::{Line as RatLine, Span as RatSpan};

        let area = Rect::new(0, 0, 36, 1);
        let mut buf = Buffer::empty(area);
        let line_bg = Color::Rgb(70, 38, 42);
        let chip_bg = Color::Rgb(140, 52, 58);
        buf.set_style(area, RatStyle::default().bg(Color::Black));
        let line = RatLine::from(vec![
            RatSpan::styled("-", RatStyle::default().bg(line_bg)),
            RatSpan::styled(" 256 │ vtcode trajectory ", RatStyle::default().bg(line_bg)),
            RatSpan::styled("--last", RatStyle::default().bg(chip_bg)),
        ]);
        for x in 25..31 {
            buf[(x, 0)].bg = chip_bg;
        }

        apply_full_width_line_backgrounds(&mut buf, area, &[line], Some(Color::Black));

        assert_eq!(buf[(26, 0)].bg, chip_bg, "intraline chip must remain stronger");
        assert_eq!(buf[(35, 0)].bg, line_bg, "a dash-prefixed chip must not create a tint gap");
    }

    #[test]
    fn full_width_fill_survives_an_uncolored_indent_before_the_marker() {
        use ratatui::style::Style as RatStyle;
        use ratatui::text::{Line as RatLine, Span as RatSpan};

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let line_bg = Color::Rgb(20, 58, 45);
        buf.set_style(area, RatStyle::default().bg(Color::Black));
        let line = RatLine::from(vec![
            RatSpan::raw("    "),
            RatSpan::styled("+", RatStyle::default().bg(line_bg)),
            RatSpan::styled("new", RatStyle::default().bg(line_bg)),
        ]);

        apply_full_width_line_backgrounds(&mut buf, area, &[line], Some(Color::Black));

        assert_eq!(buf[(19, 0)].bg, line_bg, "indentation must not disable row tinting");
    }

    #[test]
    fn full_width_fill_skips_side_by_side_dividers() {
        use ratatui::style::Style as RatStyle;
        use ratatui::text::{Line as RatLine, Span as RatSpan};

        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        let left_bg = Color::Rgb(70, 38, 42);
        let right_bg = Color::Rgb(20, 58, 45);
        buf.set_style(area, RatStyle::default().bg(Color::Black));
        let line = RatLine::from(vec![
            RatSpan::styled("-", RatStyle::default().bg(left_bg)),
            RatSpan::raw("old       "),
            RatSpan::raw("│"),
            RatSpan::styled("+", RatStyle::default().bg(right_bg)),
            RatSpan::styled("new", RatStyle::default().bg(right_bg)),
        ]);

        apply_full_width_line_backgrounds(&mut buf, area, &[line], Some(Color::Black));

        assert_eq!(buf[(19, 0)].bg, Color::Black, "pane tint must not bleed across the row");
    }

    fn row_text(buf: &Buffer, area: Rect, row: u16) -> String {
        (area.left()..area.right()).map(|x| buf[(x, row)].symbol()).collect::<String>()
    }

    #[test]
    fn transcript_gutters_adapt_to_available_width() {
        assert_eq!(
            transcript_content_area(Rect::new(7, 3, 120, 12)),
            Rect::new(9, 3, 116, 12),
            "wide terminals get a modest two-column gutter",
        );
        assert_eq!(
            transcript_content_area(Rect::new(7, 3, 80, 12)),
            Rect::new(8, 3, 78, 12),
            "standard terminals use a smaller gutter",
        );
        assert_eq!(
            transcript_content_area(Rect::new(7, 3, 48, 12)),
            Rect::new(7, 3, 48, 12),
            "very narrow terminals keep the full width",
        );

        let area = Rect::new(0, 0, 120, 6);
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(InlineMessageKind::Agent, vec![segment("wide transcript")]);
        let mut buf = Buffer::empty(area);
        for x in [0, 1, 118, 119] {
            buf[(x, 0)].set_symbol("X");
        }

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        assert_eq!(session.transcript_area(), Some(Rect::new(2, 0, 116, 6)));
        assert_eq!(buf[(0, 0)].symbol(), " ", "left gutter must stay clear");
        assert_eq!(buf[(1, 0)].symbol(), " ", "left gutter must stay clear");
        assert_eq!(buf[(2, 0)].symbol(), "w", "message text starts inside the gutter");
        assert_eq!(buf[(118, 0)].symbol(), " ", "right gutter must stay clear");
        assert_eq!(buf[(119, 0)].symbol(), " ", "right gutter must stay clear");
    }

    #[test]
    fn transcript_content_area_keeps_link_and_render_coordinates_aligned() {
        let area = Rect::new(10, 2, 80, 8);
        let content_area = transcript_content_area(area);
        let url = "https://example.com/docs";
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(InlineMessageKind::Agent, vec![segment(&format!("Open {url}"))]);
        let mut buf = Buffer::empty(area);

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        assert_eq!(session.transcript_area(), Some(content_area));
        let link_start = content_area.x + "Open ".len() as u16;
        assert_eq!(buf[(link_start, area.y)].symbol(), "h");
        assert!(
            session.update_transcript_file_link_hover(link_start, area.y),
            "the rendered URL cell should hit the matching transcript link target",
        );
        assert_eq!(row_text(&buf, area, area.y).chars().next(), Some(' '));
    }

    #[test]
    fn very_narrow_transcript_keeps_wrapped_content_inside_the_viewport() {
        let area = Rect::new(0, 0, 40, 6);
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(
            InlineMessageKind::Agent,
            vec![segment("alpha beta gamma delta epsilon zeta eta theta iota kappa")],
        );
        let mut buf = Buffer::empty(area);

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        assert_eq!(session.transcript_area(), Some(area));
        let rows: Vec<String> = (area.y..area.bottom())
            .map(|row| row_text(&buf, area, row).trim_end().to_string())
            .filter(|row| !row.is_empty())
            .collect();
        assert!(rows.len() >= 2, "narrow transcript should wrap into multiple rows: {rows:?}");
        assert!(rows.iter().all(|row| row.chars().count() <= usize::from(area.width)));
    }

    #[test]
    fn scroll_metric_invalidation_does_not_request_transcript_clear() {
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.transcript_clear_required = false;

        session.invalidate_scroll_metrics();

        assert!(!session.transcript_clear_required);
    }

    #[test]
    fn render_clears_stale_wrapped_rows_when_requested() {
        let area = Rect::new(0, 0, 14, 6);
        let inner = area;
        let mut buf = Buffer::empty(area);
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(InlineMessageKind::Agent, vec![segment("this line wraps across several rows")]);

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        let revision = session.next_revision();
        session.lines[0].segments = vec![segment("short")];
        session.lines[0].revision = revision;
        session.mark_line_dirty(0);
        session.invalidate_transcript_cache();
        for row in inner.y + 1..inner.bottom() {
            for x in inner.left()..inner.right() {
                buf[(x, row)].set_symbol("X");
            }
        }

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        assert!((inner.y + 1..inner.bottom()).all(|row| row_text(&buf, inner, row).trim().is_empty()));
    }

    #[test]
    fn render_preserves_queue_overlay_lines() {
        let area = Rect::new(0, 0, 20, 6);
        let inner = area;
        let mut buf = Buffer::empty(area);
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(InlineMessageKind::Agent, vec![segment("alpha")]);
        session.push_queued_input("queued follow-up".to_string());

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        let has_queued = (inner.y..inner.bottom()).any(|row| row_text(&buf, inner, row).contains("queued"));
        assert!(has_queued, "queued overlay should be visible");
    }

    #[test]
    fn render_queue_overlay_orders_items_fifo_oldest_on_top() {
        let area = Rect::new(0, 0, 30, 8);
        let inner = area;
        let mut buf = Buffer::empty(area);
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(InlineMessageKind::Agent, vec![segment("alpha")]);
        session.push_queued_input("first".to_string());
        session.push_queued_input("second".to_string());
        session.push_queued_input("third".to_string());

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        // The queue is strict FIFO, so the overlay shows the oldest message at
        // the top and the newest directly above the input field.
        let overlay_rows: Vec<String> = (inner.y..inner.bottom())
            .map(|row| row_text(&buf, inner, row))
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .collect();
        let first_row = overlay_rows.iter().position(|row| row.contains("first"));
        let second_row = overlay_rows.iter().position(|row| row.contains("second"));
        let third_row = overlay_rows.iter().position(|row| row.contains("third"));
        assert!(
            first_row.is_some() && second_row.is_some() && third_row.is_some(),
            "all three queued items must be visible in the overlay, got: {overlay_rows:?}"
        );
        assert!(
            first_row.unwrap() < second_row.unwrap() && second_row.unwrap() < third_row.unwrap(),
            "expected FIFO order first < second < third, got: {overlay_rows:?}"
        );
    }

    #[test]
    fn render_queue_overlay_flattens_multiline_entries() {
        let area = Rect::new(0, 0, 40, 8);
        let inner = area;
        let mut buf = Buffer::empty(area);
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(InlineMessageKind::Agent, vec![segment("alpha")]);
        session.push_queued_input("line one\nline two".to_string());

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        let overlay_text: String = (inner.y..inner.bottom()).map(|row| row_text(&buf, inner, row)).collect();
        assert!(
            overlay_text.contains("line one") && overlay_text.contains("line two"),
            "both lines of the queued entry must be visible, got: {overlay_text:?}"
        );
        assert!(
            overlay_text.contains("⏎"),
            "newlines must be flattened into a visible separator, got: {overlay_text:?}"
        );
    }

    #[test]
    fn render_clears_stale_queue_overlay_rows_when_queue_is_removed() {
        let area = Rect::new(0, 0, 20, 6);
        let inner = area;
        let mut buf = Buffer::empty(area);
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.push_line(InlineMessageKind::Agent, vec![segment("alpha")]);
        session.push_queued_input("queued follow-up".to_string());

        TranscriptWidget::new(&mut session).render(area, &mut buf);
        let has_queued_before = (inner.y..inner.bottom()).any(|row| row_text(&buf, inner, row).contains("queued"));
        assert!(has_queued_before, "queued should be visible before pop");

        let _ = session.pop_latest_queued_input();

        TranscriptWidget::new(&mut session).render(area, &mut buf);

        let has_queued_after = (inner.y..inner.bottom()).any(|row| row_text(&buf, inner, row).contains("queued"));
        assert!(!has_queued_after, "queued should be cleared after pop");
    }

    #[test]
    fn resize_larger_keeps_existing_transcript_lines_visible() {
        let small_area = Rect::new(0, 0, 20, 4);
        let large_area = Rect::new(0, 0, 20, 10);
        let small_inner = small_area;
        let large_inner = large_area;
        let mut small_buf = Buffer::empty(small_area);
        let mut large_buf = Buffer::empty(large_area);
        let mut session = Session::new(InlineTheme::default(), None, 12);

        for index in 0..6 {
            session.push_line(InlineMessageKind::Agent, vec![segment(&format!("line {index}"))]);
        }

        TranscriptWidget::new(&mut session).render(small_area, &mut small_buf);
        let small_rendered: Vec<String> = (small_inner.y..small_inner.bottom())
            .map(|row| row_text(&small_buf, small_inner, row).trim().to_string())
            .filter(|row| !row.is_empty())
            .collect();
        TranscriptWidget::new(&mut session).render(large_area, &mut large_buf);

        let rendered: Vec<String> = (large_inner.y..large_inner.bottom())
            .map(|row| row_text(&large_buf, large_inner, row).trim().to_string())
            .filter(|row| !row.is_empty())
            .collect();

        assert!(rendered.len() > small_rendered.len());
        assert!(rendered.iter().any(|row| row == "line 1"));
        assert!(rendered.iter().any(|row| row == "line 5"));
    }

    #[test]
    fn width_resize_keeps_transcript_visible() {
        let wide_area = Rect::new(0, 0, 28, 8);
        let narrow_area = Rect::new(0, 0, 16, 8);
        let narrow_inner = narrow_area;
        let mut wide_buf = Buffer::empty(wide_area);
        let mut narrow_buf = Buffer::empty(narrow_area);
        let mut session = Session::new(InlineTheme::default(), None, 12);

        for index in 0..4 {
            session.push_line(InlineMessageKind::Agent, vec![segment(&format!("line {index}"))]);
        }

        TranscriptWidget::new(&mut session).render(wide_area, &mut wide_buf);
        TranscriptWidget::new(&mut session).render(narrow_area, &mut narrow_buf);

        let rendered: Vec<String> = (narrow_inner.y..narrow_inner.bottom())
            .map(|row| row_text(&narrow_buf, narrow_inner, row).trim().to_string())
            .filter(|row| !row.is_empty())
            .collect();

        assert!(!rendered.is_empty());
        assert!(rendered.iter().any(|row| row == "line 1"));
        assert!(rendered.iter().any(|row| row == "line 3"));
    }

    fn shimmer_session_with_status(status: &str) -> Session {
        let mut session = Session::new(InlineTheme::default(), None, 12);
        session.input_status_left = Some(status.to_string());
        session
    }

    fn shimmer_line_text(line: &Line) -> String {
        line.spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn indicator_shimmer_preserves_text_and_restyles_active_row() {
        use ratatui::text::Line as RatLine;
        let session = shimmer_session_with_status("Running tool: edit_file");
        let original = RatLine::from("❋ Editing vtcode.toml...");
        let mut lines = vec![original.clone()];
        apply_active_file_operation_spinner(&session, &mut lines);

        assert_eq!(shimmer_line_text(&lines[0]), "❋ Editing vtcode.toml...");
        assert_ne!(lines[0].spans, original.spans, "active row must shimmer");
    }

    #[test]
    fn indicator_shimmer_sweep_moves_with_phase() {
        use ratatui::text::Line as RatLine;
        let mut session = shimmer_session_with_status("Drafting plan... (42 chars)");
        // The sweep head starts off-text (10 chars of padding), so advance
        // until it has travelled into the line. Bounded: ~12 ticks minimum.
        for _ in 0..40 {
            std::thread::sleep(std::time::Duration::from_millis(40));
            session.handle_tick();
            if session.shimmer_state.phase() >= 0.2 {
                break;
            }
        }
        assert!(
            session.shimmer_state.phase() >= 0.2,
            "phase must advance while drafting, got {}",
            session.shimmer_state.phase()
        );

        let mut advanced = vec![RatLine::from("❋ Drafting plan — researching codebase...")];
        apply_active_file_operation_spinner(&session, &mut advanced);

        let fresh = shimmer_session_with_status("Drafting plan... (42 chars)");
        let mut at_zero = vec![RatLine::from("❋ Drafting plan — researching codebase...")];
        apply_active_file_operation_spinner(&fresh, &mut at_zero);

        assert_eq!(shimmer_line_text(&advanced[0]), "❋ Drafting plan — researching codebase...");
        assert_ne!(advanced[0].spans, at_zero[0].spans, "sweep position must follow the shared shimmer phase");
    }

    #[test]
    fn indicator_shimmer_animates_only_the_newest_row() {
        use ratatui::text::Line as RatLine;
        let session = shimmer_session_with_status("Validating plan...");
        let first = RatLine::from("❋ Drafting plan — researching codebase...");
        let second = RatLine::from("❋ Validating plan...");
        let mut lines = vec![first.clone(), second.clone()];
        apply_active_file_operation_spinner(&session, &mut lines);

        assert_eq!(lines[0].spans, first.spans, "older row must stay static");
        assert_ne!(lines[1].spans, second.spans, "newest row must shimmer");
    }

    #[test]
    fn indicator_shimmer_stays_static_without_live_status() {
        use ratatui::text::Line as RatLine;
        for status in ["Running tool: code_search", "main*", "Ready"] {
            let session = shimmer_session_with_status(status);
            let original = RatLine::from("❋ Drafting plan — researching codebase...");
            let mut lines = vec![original.clone()];
            apply_active_file_operation_spinner(&session, &mut lines);
            assert_eq!(lines[0].spans, original.spans, "stale row must not shimmer for {status:?}");
        }

        let mut session = shimmer_session_with_status("Drafting plan... (42 chars)");
        session.appearance.reduce_motion_mode = true;
        let original = RatLine::from("❋ Drafting plan — researching codebase...");
        let mut lines = vec![original.clone()];
        apply_active_file_operation_spinner(&session, &mut lines);
        assert_eq!(lines[0].spans, original.spans, "reduced motion must keep the row static");
    }
}
