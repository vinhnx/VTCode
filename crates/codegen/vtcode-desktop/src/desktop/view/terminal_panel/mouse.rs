//! Terminal pointer and wheel handlers, split for the 1000-line ceiling.

use super::*;

impl DesktopView {
    pub(super) fn handle_terminal_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminal_focus.focus(window, cx);
        let (Some(tab), Some(grid)) = (self.terminal.as_mut(), self.terminal_grid_bounds.get()) else {
            return;
        };
        let origin = grid.origin;
        let char_width =
            terminal_column_width(cx.text_system(), &cx.theme().mono_font_family, cx.theme().mono_font_size);
        let row_height = terminal_row_height(cx.theme().mono_font_size);
        let size = tab.grid_size();
        let Some(anchor) = terminal_cell_for_point(
            event.position,
            origin,
            row_height,
            char_width,
            usize::from(size.rows),
            usize::from(size.cols),
        ) else {
            return;
        };
        use rio_vt::{crosswords::pos::Side, selection::SelectionType};
        // Shift-click extends from the retained anchor instead of
        // re-anchoring; without a selection it behaves as a plain click.
        // Multi-clicks ignore shift (word/line select wins).
        if event.modifiers.shift
            && event.click_count == 1
            && let Some(retained) = self.terminal_select_anchor
        {
            let (start, end) = if anchor >= retained {
                (retained, anchor)
            } else {
                (anchor, retained)
            };
            tab.start_selection(start.0, start.1, Side::Left, SelectionType::Simple);
            tab.update_selection(end.0, end.1, Side::Right);
            self.terminal_select_drag = Some(TerminalSelectDrag { anchor: retained, char_width });
            cx.notify();
            return;
        }
        // Clicks select by count; only single clicks start a drag. Clearing
        // the drag on multi-clicks keeps a later up from collapsing the
        // word or line just selected.
        let (ty, dragging) = match event.click_count {
            2 => (SelectionType::Semantic, false),
            3.. => (SelectionType::Lines, false),
            _ => (SelectionType::Simple, true),
        };
        tab.start_selection(anchor.0, anchor.1, Side::Left, ty);
        self.terminal_select_drag = dragging.then_some(TerminalSelectDrag { anchor, char_width });
        self.terminal_select_anchor = Some(anchor);
        cx.notify();
    }

    pub(super) fn handle_terminal_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.terminal_select_drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            // Released outside the panel: the up handler never fires.
            self.terminal_select_drag = None;
            return;
        }
        let (Some(tab), Some(grid)) = (self.terminal.as_mut(), self.terminal_grid_bounds.get()) else {
            return;
        };
        let origin = grid.origin;
        let row_height = terminal_row_height(cx.theme().mono_font_size);
        let size = tab.grid_size();
        let Some(endpoint) = terminal_cell_for_point(
            event.position,
            origin,
            row_height,
            drag.char_width,
            usize::from(size.rows),
            usize::from(size.cols),
        ) else {
            return;
        };
        // Rebuild from scratch each move: the anchor side must flip with the
        // drag direction (an incremental update keeps the stale side and
        // drops the anchor cell on backward drags).
        let (start, end) = if endpoint >= drag.anchor {
            (drag.anchor, endpoint)
        } else {
            (endpoint, drag.anchor)
        };
        use rio_vt::{crosswords::pos::Side, selection::SelectionType};
        tab.start_selection(start.0, start.1, Side::Left, SelectionType::Simple);
        tab.update_selection(end.0, end.1, Side::Right);
        cx.notify();
    }

    pub(super) fn handle_terminal_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.terminal_select_drag.take() else {
            return;
        };
        let (Some(tab), Some(grid)) = (self.terminal.as_mut(), self.terminal_grid_bounds.get()) else {
            return;
        };
        let origin = grid.origin;
        let row_height = terminal_row_height(cx.theme().mono_font_size);
        let size = tab.grid_size();
        let endpoint = terminal_cell_for_point(
            event.position,
            origin,
            row_height,
            drag.char_width,
            usize::from(size.rows),
            usize::from(size.cols),
        );
        if endpoint == Some(drag.anchor) {
            // Plain click: collapse the single-cell selection.
            let _collapsed = tab.clear_selection();
            self.terminal_select_anchor = None;
        }
        cx.notify();
    }

    pub(super) fn handle_terminal_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let row_height = terminal_row_height(cx.theme().mono_font_size);
        let (lines, remainder) = wheel_line_delta(&event.delta, row_height, self.terminal_wheel_remainder);
        self.terminal_wheel_remainder = remainder;
        let Some(tab) = self.terminal.as_mut() else {
            return;
        };
        if lines == 0 {
            return;
        }
        // One event never moves more than a viewport; Rio clamps the rest.
        let rows = i32::from(tab.grid_size().rows.max(1));
        let lines = lines.clamp(-rows, rows);
        if tab.in_alternate_screen() {
            let arrow = if lines > 0 { b'A' } else { b'B' };
            let step = cursor_bytes(tab.app_cursor(), arrow);
            let mut bytes = Vec::with_capacity(step.len() * lines.unsigned_abs() as usize);
            for _ in 0..lines.unsigned_abs() {
                bytes.extend_from_slice(&step);
            }
            let _sent = tab.input(bytes);
        } else {
            tab.scroll_to(rio_vt::crosswords::grid::Scroll::Delta(lines));
            cx.notify();
        }
        cx.stop_propagation();
    }
}
