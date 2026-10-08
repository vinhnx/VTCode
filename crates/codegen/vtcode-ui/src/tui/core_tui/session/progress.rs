use super::*;
use ratatui::{buffer::Buffer, style::Modifier, widgets::Paragraph};
use vtcode_commons::ui_protocol::{ProgressOperation, ProgressPhase, ProgressUpdate};

#[derive(Clone, Copy)]
struct ActiveProgress {
    operation: ProgressOperation,
    phase: ProgressPhase,
    feedback_observed: bool,
}

#[derive(Default)]
pub(crate) struct TransientProgress {
    latest_id: u64,
    active: Option<ActiveProgress>,
    elapsed_secs: u64,
}

impl TransientProgress {
    pub(crate) fn apply(&mut self, update: ProgressUpdate) -> bool {
        match update {
            ProgressUpdate::Begin { operation, phase } if operation.id() > self.latest_id => {
                self.latest_id = operation.id();
                self.active = Some(ActiveProgress { operation, phase, feedback_observed: false });
                self.elapsed_secs = operation.started_at().elapsed().as_secs();
                true
            }
            ProgressUpdate::Phase { operation, phase }
                if self
                    .active
                    .is_some_and(|current| current.operation == operation && current.phase != phase) =>
            {
                if let Some(current) = self.active.as_mut() {
                    current.phase = phase;
                }
                self.elapsed_secs = operation.started_at().elapsed().as_secs();
                true
            }
            ProgressUpdate::Finish { operation }
                if self.active.is_some_and(|current| current.operation == operation) =>
            {
                self.active = None;
                true
            }
            _ => false,
        }
    }

    pub(crate) fn tick(&mut self) -> bool {
        let Some(ActiveProgress { operation, phase, .. }) = self.active else {
            return false;
        };
        if !phase.is_animated() {
            return false;
        }
        let elapsed_secs = operation.started_at().elapsed().as_secs();
        if elapsed_secs == self.elapsed_secs {
            return false;
        }
        self.elapsed_secs = elapsed_secs;
        true
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(crate) fn is_animated(&self) -> bool {
        self.active.is_some_and(|current| current.phase.is_animated())
    }

    pub(crate) fn text(&self) -> Option<String> {
        self.active.map(|current| current.phase.format(self.elapsed_secs))
    }
}

impl Session {
    pub(crate) fn set_progress_area(&mut self, area: Option<Rect>) {
        // Keep row ownership separate from feedback that survives overlays.
        self.areas.set_progress(area);
        if area.is_none() {
            self.areas.set_progress_feedback(None);
        }
    }

    pub(crate) fn progress_row_visible(&self) -> bool {
        self.progress.is_active() && self.areas.progress().is_some()
    }

    pub(crate) fn progress_footer_status_text(&self) -> Option<&str> {
        // Keep the configured status-line text while the transcript owns
        // foreground progress, bypassing ActivityState's loading labels.
        self.footer_context_status.as_deref()
    }

    pub(crate) fn set_progress_feedback_area(&mut self, area: Rect) {
        if !area.is_empty() {
            self.areas.set_progress_feedback(Some(area));
        }
    }

    pub(crate) fn occlude_progress_feedback(&mut self, area: Rect) {
        if self.areas.progress_feedback().is_some_and(|feedback| feedback.intersects(area)) {
            self.areas.set_progress_feedback(None);
        }
    }

    pub(crate) fn observe_progress_feedback(&mut self) {
        if self.areas.progress_feedback().is_some()
            && let Some(current) = self.progress.active.as_mut()
            && !current.feedback_observed
        {
            current.feedback_observed = true;
            tracing::debug!(target: "vtcode.response_latency", operation_id = current.operation.id(),
                accepted_to_feedback_ms = current.operation.started_at().elapsed().as_secs_f64() * 1000.0,
                "progress frame rendered");
        }
    }

    pub(crate) fn render_progress(&mut self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() || !self.progress.is_active() {
            return;
        }
        self.set_progress_area(Some(area));
        let Some(text) = self.progress.text() else { return };
        let feedback_columns = measure_text_width(&text).min(area.width);
        let style = self.styles.default_style().add_modifier(Modifier::DIM);
        let mut spans = if self.progress.is_animated() && self.appearance.should_animate_progress_status() {
            tui_shimmer::shimmer_spans_with_style_at_phase(&text, style, self.shimmer_state.phase())
        } else {
            vec![Span::styled(text, style)]
        };
        // Live background count rides the transcript loading row while it owns
        // foreground progress, so the bottom line stays reserved for configured
        // context. Static dim text (no shimmer) keeps the row width stable:
        // only task start/finish changes it, never per-second ticks. The
        // combined line truncates head-first, so the phase label wins on
        // narrow rows and the header badge remains the guaranteed home.
        if self.has_background_activity() {
            let suffix = format!(" · {} bg", self.background_activity_count);
            spans.push(Span::styled(suffix, style));
        }
        let line =
            utils::line_truncation::truncate_line_with_ellipsis_if_overflow(Line::from(spans), usize::from(area.width));
        Clear.render(area, buf);
        Paragraph::new(line).render(area, buf);
        self.set_progress_feedback_area(Rect::new(area.x, area.y, feedback_columns, 1));
    }
}

#[cfg(test)]
mod tests;
