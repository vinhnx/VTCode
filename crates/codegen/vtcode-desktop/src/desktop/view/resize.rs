//! Pane resizing: the splitter handles, their drag state, and the size
//! clamping that keeps every pane inside its bounds.

use super::*;

/// Which divider is being dragged.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ResizeEdge {
    Sidebar,
    Inspector,
    Terminal,
    TerminalLeft,
    TerminalRight,
}

#[derive(Clone, Copy)]
pub(super) struct PaneResize {
    pub(super) edge: ResizeEdge,
    pub(super) start_pointer: Pixels,
    pub(super) start_size: Pixels,
}

/// The splitter has no drag preview; keeping its entity empty lets the handle
/// provide GPUI's drag lifecycle without covering the panels during resizing.
struct ResizeDragPreview;

impl Render for ResizeDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size(px(0.))
    }
}

impl DesktopView {
    /// The draggable divider between panes.
    pub(super) fn render_resize_handle(&self, edge: ResizeEdge, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, placement, label, size) = match edge {
            ResizeEdge::Sidebar => (
                "resize-sidebar",
                if self.preferences.navigation_on_right() {
                    HandleEdge::Leading
                } else {
                    HandleEdge::Trailing
                },
                rust_i18n::t!("app.navigation.resize_navigation").to_string(),
                self.sidebar_width,
            ),
            ResizeEdge::Inspector => (
                "resize-inspector",
                HandleEdge::Leading,
                rust_i18n::t!("app.navigation.resize_inspector").to_string(),
                self.inspector_width,
            ),
            // a11y-only label, exempt from localization per the design guides.
            ResizeEdge::Terminal => {
                ("terminal-resize", HandleEdge::Leading, "Resize terminal".to_string(), self.terminal_panel_height)
            }
            ResizeEdge::TerminalLeft => (
                "terminal-resize-left",
                HandleEdge::Trailing,
                "Resize terminal".to_string(),
                self.terminal_panel_width,
            ),
            ResizeEdge::TerminalRight => (
                "terminal-resize-right",
                HandleEdge::Leading,
                "Resize terminal".to_string(),
                self.terminal_panel_width,
            ),
        };
        let (min, max) = match edge {
            ResizeEdge::Terminal => (MIN_TERMINAL_PANEL_HEIGHT, MAX_TERMINAL_PANEL_HEIGHT),
            ResizeEdge::TerminalLeft | ResizeEdge::TerminalRight => {
                (MIN_TERMINAL_PANEL_WIDTH, MAX_TERMINAL_PANEL_WIDTH)
            }
            ResizeEdge::Sidebar | ResizeEdge::Inspector => (MIN_PANE_WIDTH, MAX_PANE_WIDTH),
        };
        let focus = match edge {
            ResizeEdge::Sidebar => &self.sidebar_resize_focus,
            ResizeEdge::Inspector => &self.inspector_resize_focus,
            ResizeEdge::Terminal | ResizeEdge::TerminalLeft | ResizeEdge::TerminalRight => &self.terminal_resize_focus,
        };
        let axis = match edge {
            ResizeEdge::Terminal => Axis::Vertical,
            ResizeEdge::Sidebar | ResizeEdge::Inspector | ResizeEdge::TerminalLeft | ResizeEdge::TerminalRight => {
                Axis::Horizontal
            }
        };
        let handle = resize_handle::<ResizeEdge, ResizeDragPreview>(id, axis);
        // The inspector is a separated card, not a docked pane: its edge
        // paints no resting hairline (see inspector_resize_appearance).
        let handle = if edge == ResizeEdge::Inspector {
            handle.with_appearance(inspector_resize_appearance())
        } else {
            handle
        };
        handle
            .inside(placement)
            .on_drag(edge, |_, _, _, cx| cx.new(|_| ResizeDragPreview))
            .on_drag_move(cx.listener(|this, event: &DragMoveEvent<ResizeEdge>, _, cx| {
                let edge = *event.drag(cx);
                if this.resizing.is_some_and(|drag| drag.edge == edge) {
                    let pointer = match edge {
                        ResizeEdge::Terminal => event.event.position.y,
                        ResizeEdge::Sidebar
                        | ResizeEdge::Inspector
                        | ResizeEdge::TerminalLeft
                        | ResizeEdge::TerminalRight => event.event.position.x,
                    };
                    let changed = this.resize_from_drag(edge, pointer);
                    cx.stop_propagation();
                    if changed {
                        cx.notify();
                    }
                }
            }))
            .track_focus(focus)
            .role(gpui_kit::Role::Splitter)
            .aria_label(label)
            // The shared hint names arrow keys; terminal needs its own translations.
            .when(edge != ResizeEdge::Terminal, |handle| {
                handle.aria_description(rust_i18n::t!("app.navigation.resize_hint").to_string())
            })
            .aria_numeric_value(size as f64)
            .aria_min_numeric_value(min as f64)
            .aria_max_numeric_value(max as f64)
            .focus_visible(|style| style.bg(cx.theme().colors.ring.opacity(0.2)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                let modifiers = event.keystroke.modifiers;
                if modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
                    return;
                }
                let vertical = edge == ResizeEdge::Terminal;
                let direction = match (vertical, event.keystroke.key.as_str()) {
                    (false, "left") => -1.,
                    (false, "right") => 1.,
                    (true, "up") => 1.,
                    (true, "down") => -1.,
                    _ => return,
                };
                let step = if modifiers.shift { 50. } else { 10. };
                let current_size = match edge {
                    ResizeEdge::Sidebar => this.sidebar_width,
                    ResizeEdge::Inspector => this.inspector_width,
                    ResizeEdge::Terminal => this.terminal_panel_height,
                    ResizeEdge::TerminalLeft | ResizeEdge::TerminalRight => this.terminal_panel_width,
                };
                let delta = match edge {
                    ResizeEdge::Sidebar if this.preferences.navigation_on_right() => -direction * step,
                    ResizeEdge::Sidebar | ResizeEdge::Terminal | ResizeEdge::TerminalLeft => direction * step,
                    ResizeEdge::Inspector | ResizeEdge::TerminalRight => -direction * step,
                };
                if this.resize_pane_to(edge, current_size + delta) {
                    this.save_preferences();
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    match edge {
                        ResizeEdge::Sidebar => this.sidebar_resize_focus.focus(window, cx),
                        ResizeEdge::Inspector => this.inspector_resize_focus.focus(window, cx),
                        ResizeEdge::Terminal | ResizeEdge::TerminalLeft | ResizeEdge::TerminalRight => {
                            this.terminal_resize_focus.focus(window, cx);
                        }
                    }
                    let (pointer, size) = match edge {
                        ResizeEdge::Terminal => (event.position.y, this.terminal_panel_height),
                        ResizeEdge::Sidebar => (event.position.x, this.sidebar_width),
                        ResizeEdge::Inspector => (event.position.x, this.inspector_width),
                        ResizeEdge::TerminalLeft | ResizeEdge::TerminalRight => {
                            (event.position.x, this.terminal_panel_width)
                        }
                    };
                    this.resizing = Some(PaneResize { edge, start_pointer: pointer, start_size: px(size) });
                    cx.notify();
                }),
            )
            .on_drop(cx.listener(|this, _: &ResizeEdge, _, cx| {
                if this.resizing.take().is_some() {
                    this.save_preferences();
                    cx.notify();
                }
            }))
    }

    fn resize_pane_to(&mut self, edge: ResizeEdge, size: f32) -> bool {
        if !size.is_finite() {
            return false;
        }
        let size = match edge {
            ResizeEdge::Terminal => size.clamp(MIN_TERMINAL_PANEL_HEIGHT, MAX_TERMINAL_PANEL_HEIGHT),
            ResizeEdge::TerminalLeft | ResizeEdge::TerminalRight => {
                size.clamp(MIN_TERMINAL_PANEL_WIDTH, MAX_TERMINAL_PANEL_WIDTH)
            }
            ResizeEdge::Sidebar | ResizeEdge::Inspector => size.clamp(MIN_PANE_WIDTH, MAX_PANE_WIDTH),
        };
        let current = match edge {
            ResizeEdge::Sidebar => &mut self.sidebar_width,
            ResizeEdge::Inspector => &mut self.inspector_width,
            ResizeEdge::Terminal => &mut self.terminal_panel_height,
            ResizeEdge::TerminalLeft | ResizeEdge::TerminalRight => &mut self.terminal_panel_width,
        };
        if (*current - size).abs() < f32::EPSILON {
            return false;
        }
        *current = size;
        true
    }

    fn resize_from_drag(&mut self, edge: ResizeEdge, pointer_window: Pixels) -> bool {
        let Some(drag) = self.resizing.filter(|drag| drag.edge == edge) else {
            return false;
        };
        let pointer_delta_window = pointer_window - drag.start_pointer;
        let size = match edge {
            ResizeEdge::Sidebar if self.preferences.navigation_on_right() => drag.start_size - pointer_delta_window,
            ResizeEdge::Sidebar | ResizeEdge::TerminalLeft => drag.start_size + pointer_delta_window,
            ResizeEdge::Inspector | ResizeEdge::Terminal | ResizeEdge::TerminalRight => {
                drag.start_size - pointer_delta_window
            }
        };
        // Preferences keep pane sizes as f32; convert only at that boundary.
        self.resize_pane_to(edge, size.as_f32())
    }
}
