//! Bottom terminal panel: visibility, drain pump, and grid rows.
//!
//! One session per workspace (no tabs): opening spawns lazily in the workspace
//! directory, hiding keeps it alive, switching workspaces closes it. Invariant:
//! `terminal_visible` implies a live `terminal` session. A generation-guarded
//! task drains PTY output and redraws on change; floods pace out over frames.

use super::*;

mod input;
use input::*;

/// Drain cadence: bounded redraws without a busy loop on floods.
const TERMINAL_PUMP_INTERVAL: Duration = Duration::from_millis(16);
/// Fallback grid rows when unmeasurable (xterm standard).
const DEFAULT_TERMINAL_ROWS: u16 = 24;
/// Header strip height in px. Fixed pixels, not rem: grid math must match
/// the laid-out height exactly at any UI scale (fixed pane geometry).
const TERMINAL_HEADER_HEIGHT: f32 = 32.0;
/// Grid side padding in px each side (fixed for the same reason).
const TERMINAL_GRID_PAD_X: f32 = 16.0;
/// Grid top/bottom padding in px each (fixed for the same reason).
const TERMINAL_GRID_PAD_Y: f32 = 8.0;
/// Grid columns at spawn, before the first paint measures the real width
/// (xterm standard); the pump corrects to measured columns immediately.
const TERMINAL_GRID_COLUMNS: u16 = 80;

impl DesktopView {
    pub(super) fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.terminal_visible {
            self.hide_terminal(window, cx);
        } else {
            self.show_terminal(window, cx);
        }
    }

    /// Show the panel, spawning the shell on first open (home directory
    /// when no workspace is open yet).
    fn show_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ensure_terminal(cx) {
            return;
        }
        self.terminal_pump_generation = self.terminal_pump_generation.wrapping_add(1);
        let generation = self.terminal_pump_generation;
        self.terminal_visible = true;
        self.terminal_focus.focus(window, cx);
        cx.notify();
        // Retained: dropping the task would cancel the loop before its
        // first tick and freeze the panel (no drains, ever).
        self.terminal_pump_task = Some(cx.spawn(async move |view, cx| {
            loop {
                let keep_going = view
                    .update(cx, |view, cx| view.pump_terminal_once(generation, cx))
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
                cx.background_executor().timer(TERMINAL_PUMP_INTERVAL).await;
            }
        }));
    }

    /// Show the panel (spawning if needed), focus it, and run one command
    /// line in the live shell. Powers the "continue in the built-in
    /// terminal" CTA; callers pass generated commands, never raw user text.
    pub(super) fn run_command_in_terminal(&mut self, command: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.terminal_visible {
            self.show_terminal(window, cx);
            if !self.terminal_visible {
                return;
            }
        }
        self.terminal_focus.focus(window, cx);
        if let Some(tab) = self.terminal.as_mut() {
            let mut bytes = command.as_bytes().to_vec();
            bytes.push(b'\r');
            let _sent = tab.input(bytes);
        }
        cx.notify();
    }

    fn hide_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_visible = false;
        self.terminal_pump_task = None;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    /// Spawn the workspace shell if none exists, falling back to the home
    /// directory when no workspace is open. Returns whether a session is
    /// live afterwards; failures surface through the shared error banner.
    fn ensure_terminal(&mut self, cx: &mut Context<Self>) -> bool {
        if self.terminal.is_some() {
            return true;
        }
        let Some(workspace) = terminal_shell_dir(self.workspace.as_ref()) else {
            return false;
        };
        let dark = terminal_palette_is_dark(self.preferences.terminal_ansi_theme(), self.theme_mode);
        let palette = super::super::terminal_tab::palette_for_theme(&cx.theme().colors, dark);
        let rows = terminal_content_rows(self.terminal_panel_height, cx.theme().mono_font_size)
            .unwrap_or(DEFAULT_TERMINAL_ROWS);
        let size = portable_pty::PtySize {
            rows,
            cols: TERMINAL_GRID_COLUMNS,
            pixel_width: 0,
            pixel_height: 0,
        };
        match super::super::terminal_tab::TerminalTab::spawn_login(
            &workspace,
            &super::super::terminal_process::default_shell(),
            size,
            palette,
        ) {
            Ok(tab) => {
                self.terminal = Some(tab);
                true
            }
            Err(error) => {
                self.error = Some(format!("Terminal: {error}"));
                cx.notify();
                false
            }
        }
    }

    /// Drain one batch and redraw on change. False stops the pump: stale
    /// generation, hidden panel, or released session.
    pub(super) fn pump_terminal_once(&mut self, generation: u64, cx: &mut Context<Self>) -> bool {
        if self.terminal_pump_generation != generation || !self.terminal_visible {
            return false;
        }
        let Some(tab) = self.terminal.as_mut() else {
            return false;
        };
        // View events (bell/title) have no v1a surface yet.
        if tab.drain().changed {
            cx.notify();
        }
        self.sync_terminal_grid(cx);
        true
    }

    /// Recompute grid rows and columns from the laid-out grid bounds;
    /// no-op until the first paint measures them. Either dimension resizes
    /// the PTY only when it actually changed.
    fn sync_terminal_grid(&mut self, cx: &mut Context<Self>) {
        let Some(grid) = self.terminal_grid_bounds.get() else {
            return;
        };
        let row_height = terminal_row_height(cx.theme().mono_font_size);
        let char_width =
            terminal_column_width(cx.text_system(), &cx.theme().mono_font_family, cx.theme().mono_font_size);
        let rows = terminal_rows_for_height(grid.size.height.as_f32(), row_height);
        let columns = terminal_cols_for_width(grid.size.width.as_f32(), char_width);
        let Some(tab) = self.terminal.as_mut() else {
            return;
        };
        let size = tab.grid_size();
        let rows = rows.unwrap_or(size.rows);
        let columns = columns.unwrap_or(size.cols);
        if size.rows != rows || size.cols != columns {
            let _resized = tab.resize(portable_pty::PtySize { rows, cols: columns, ..size });
        }
    }

    /// Rebuild the ANSI palette from the current theme and ANSI setting.
    /// Theme, contrast, and setting changes all funnel through here so the
    /// session never renders a stale table.
    pub(super) fn sync_terminal_palette(&mut self, cx: &mut Context<Self>) {
        let dark = terminal_palette_is_dark(self.preferences.terminal_ansi_theme(), self.theme_mode);
        let palette = super::super::terminal_tab::palette_for_theme(&cx.theme().colors, dark);
        if let Some(tab) = &mut self.terminal {
            tab.set_palette(palette);
        }
    }

    fn handle_terminal_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let Some(tab) = self.terminal.as_mut() else {
            return;
        };
        let keystroke = &event.keystroke;
        if is_paste(keystroke)
            && let Some(text) = cx.read_from_clipboard().and_then(|item| item.text())
        {
            tab.scroll_to(rio_vt::crosswords::grid::Scroll::Bottom);
            let _sent = tab.paste(text.as_ref());
            cx.stop_propagation();
            return;
        }
        if is_select_all(keystroke) {
            tab.select_all();
            self.terminal_select_anchor = Some((0, 0));
            cx.notify();
            cx.stop_propagation();
            return;
        }
        // Clearing never swallows the key: the shell still receives Escape below.
        if is_bare_escape(keystroke) && tab.clear_selection() {
            self.terminal_select_anchor = None;
            cx.notify();
        }
        if is_copy(keystroke) {
            if let Some(text) = copyable_text(tab.selected_text()) {
                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
                cx.stop_propagation();
                return;
            }
        }
        if let Some(target) = scroll_target(keystroke) {
            tab.scroll_to(target);
            cx.notify();
            cx.stop_propagation();
            return;
        }
        if let Some(bytes) = key_bytes(keystroke, tab.app_cursor()) {
            tab.scroll_to(rio_vt::crosswords::grid::Scroll::Bottom);
            let _sent = tab.input(bytes);
            cx.stop_propagation();
        }
    }

    fn handle_terminal_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
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

    fn handle_terminal_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
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

    fn handle_terminal_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
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

    fn handle_terminal_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
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

    pub(super) fn render_terminal_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mono = cx.theme().mono_font_family.clone();
        let mono_size = cx.theme().mono_font_size;
        // flex_1 (not shrink-wrap) so the absolutely-positioned probe
        // measures available space rather than rendered rows.
        let mut rows = div().flex().flex_col().w_full().flex_1().min_h_0();
        rows = rows.child(TerminalOriginProbe { sink: self.terminal_grid_bounds.clone() });
        if let Some(tab) = &self.terminal {
            for (index, runs) in tab.display_rows().iter().enumerate() {
                let mut row = div().id(("terminal-row", index)).test_support().flex().w_full();
                for run in runs {
                    row = row.child(
                        div()
                            .text_size(mono_size)
                            .font_family(mono.clone())
                            .when(run.bold, |cell| cell.font_weight(gpui_kit::FontWeight::SEMIBOLD))
                            .text_color(gpui_kit::Hsla::from(run.fg))
                            .bg(gpui_kit::Hsla::from(run.bg))
                            .child(run.text.clone()),
                    );
                }
                rows = rows.child(row);
            }
        }
        let hide_label = rust_i18n::t!("app.navigation.terminal_hide").to_string();
        let cwd_label = self
            .terminal
            .as_ref()
            .and_then(|tab| tab.current_directory())
            .or_else(|| terminal_shell_dir(self.workspace.as_ref()))
            .map(|dir| terminal_short_dir(&dir))
            .unwrap_or_default();
        let position = self.preferences.terminal_position();
        let collapse_icon = match position {
            preferences::TerminalPosition::Bottom => IconName::ChevronDown,
            preferences::TerminalPosition::Left => IconName::ChevronLeft,
            preferences::TerminalPosition::Right => IconName::ChevronRight,
        };
        div()
            .id("terminal-panel")
            .test_support()
            .track_focus(&self.terminal_focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, window, cx| {
                    view.handle_terminal_mouse_down(event, window, cx);
                }),
            )
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                view.handle_terminal_mouse_move(event, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, event: &MouseUpEvent, _, cx| {
                    view.handle_terminal_mouse_up(event, cx);
                }),
            )
            .focus_visible(|style| style.border_color(colors.ring))
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                view.handle_terminal_key(event, cx);
            }))
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.navigation.terminal").to_string())
            .when(position == preferences::TerminalPosition::Bottom, |panel| {
                panel.w_full().h(px(self.terminal_panel_height))
            })
            .when(position != preferences::TerminalPosition::Bottom, |panel| {
                panel
                    .w(px(self.terminal_panel_width))
                    .min_w(px(MIN_TERMINAL_PANEL_WIDTH))
                    .h_full()
            })
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(colors.group_box)
            .child(
                div()
                    .id("terminal-header")
                    .test_support()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(TERMINAL_HEADER_HEIGHT))
                    .flex_shrink_0()
                    .px(px(8.))
                    .gap_2()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .id("terminal-tab")
                            .test_support()
                            .flex()
                            .items_center()
                            .gap_1()
                            .min_w_0()
                            .child(Icon::new(IconName::Folder).small())
                            .child(div().text_sm().truncate().min_w_0().child(cwd_label))
                            .child(
                                desktop_button("terminal-tab-close")
                                    .icon(IconName::X)
                                    .ghost()
                                    .small()
                                    .accessibility_label(hide_label.clone())
                                    .tooltip_with_action(
                                        hide_label.clone(),
                                        &ToggleTerminal,
                                        Some(key_context::DESKTOP),
                                    )
                                    .dispatches_desktop_action(ToggleTerminal, self.focus_handle.clone()),
                            ),
                    )
                    .child(
                        desktop_button("terminal-collapse")
                            .icon(collapse_icon)
                            .ghost()
                            .small()
                            .accessibility_label(hide_label.clone())
                            .tooltip_with_action(hide_label, &ToggleTerminal, Some(key_context::DESKTOP))
                            .dispatches_desktop_action(ToggleTerminal, self.focus_handle.clone()),
                    ),
            )
            .child(
                div()
                    .id("terminal-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.terminal_scroll)
                    .px(px(TERMINAL_GRID_PAD_X))
                    .pt(px(TERMINAL_GRID_PAD_Y))
                    .pb(px(TERMINAL_GRID_PAD_Y))
                    .on_scroll(cx.listener(|view, event: &ScrollEvent, _, cx| {
                        view.handle_terminal_scroll(event, cx);
                    }))
                    .child(rows),
            )
    }
}

/// Active mouse-drag selection: anchor cell plus the column width measured
/// when the drag started (font metrics cannot change mid-gesture).
#[derive(Clone, Copy, Debug)]
pub(super) struct TerminalSelectDrag {
    anchor: (usize, usize),
    char_width: f32,
}

/// Geometry probe, mounted as the first grid child. Absolutely positioned
/// at full size, it reports the rows area bounds without disturbing layout.
/// Mouse handlers use the origin for cell mapping; grid sync uses the size
/// for rows and columns; the shared sink carries them back to the view.
struct TerminalOriginProbe {
    sink: Rc<Cell<Option<gpui_kit::Bounds<Pixels>>>>,
}

impl Element for TerminalOriginProbe {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<gpui_kit::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&gpui_kit::GlobalElementId>,
        _inspector_id: Option<&gpui_kit::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui_kit::LayoutId, ()) {
        let style = gpui_kit::Style {
            position: gpui_kit::Position::Absolute,
            size: Size::full(),
            ..Default::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&gpui_kit::GlobalElementId>,
        _inspector_id: Option<&gpui_kit::InspectorElementId>,
        bounds: gpui_kit::Bounds<Pixels>,
        _layout: &mut (),
        _window: &mut Window,
        _cx: &mut App,
    ) {
        self.sink.set(Some(bounds));
    }

    fn paint(
        &mut self,
        _id: Option<&gpui_kit::GlobalElementId>,
        _inspector_id: Option<&gpui_kit::InspectorElementId>,
        _bounds: gpui_kit::Bounds<Pixels>,
        _layout: &mut (),
        _prepaint: &mut (),
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }
}

impl IntoElement for TerminalOriginProbe {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

/// Monospace column width from the text system (`M` advance). Falls back
/// to the 0.6 ratio (the headless platform's exact advance) when the font
/// cannot resolve, so mapping stays total instead of failing closed.
fn terminal_column_width(
    text_system: &gpui_kit::TextSystem,
    mono_family: &gpui_kit::SharedString,
    mono_size: Pixels,
) -> f32 {
    let font_id = text_system.resolve_font(&gpui_kit::font(mono_family.clone()));
    text_system
        .advance(font_id, mono_size, 'M')
        .ok()
        .map(|advance| f32::from(advance.width))
        .filter(|width| width.is_finite() && *width > 0.0)
        .unwrap_or(mono_size.as_f32() * 0.6)
}

/// Window position to visible grid cell. `None` before the grid or past
/// its end (fail-closed); short lines clamp to the last column so clicks
/// past the text still select.
fn terminal_cell_for_point(
    position: gpui_kit::Point<Pixels>,
    origin: gpui_kit::Point<Pixels>,
    row_height: f32,
    char_width: f32,
    rows: usize,
    columns: usize,
) -> Option<(usize, usize)> {
    if rows == 0 || columns == 0 || row_height <= 0.0 || char_width <= 0.0 {
        return None;
    }
    let dx = f32::from(position.x - origin.x);
    let dy = f32::from(position.y - origin.y);
    if dx < 0.0 || dy < 0.0 {
        return None;
    }
    let row = (dy / row_height).floor();
    if row >= rows as f32 {
        return None;
    }
    let column = (dx / char_width).floor().clamp(0.0, (columns - 1) as f32);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "both operands are finite and within small grid bounds"
    )]
    let cell = (row as usize, column as usize);
    Some(cell)
}

/// Effective palette darkness: Auto follows the desktop mode, otherwise
/// the explicit override wins.
fn terminal_palette_is_dark(setting: preferences::TerminalAnsiTheme, mode: ThemeMode) -> bool {
    match setting {
        preferences::TerminalAnsiTheme::Auto => mode == ThemeMode::Dark,
        preferences::TerminalAnsiTheme::Dark => true,
        preferences::TerminalAnsiTheme::Light => false,
    }
}

/// GPUI default text line height as a multiple of font size
/// (`gpui_pre::geometry::phi`, pinned crate). The laid-out test trips on drift.
#[allow(clippy::approx_constant, reason = "must match the framework literal bitwise")]
const TERMINAL_LINE_HEIGHT_FACTOR: f32 = 1.618_034;

/// Terminal row height: GPUI's default line height for the mono size,
/// mirroring style resolution exactly (factor times size, rounded).
pub(super) fn terminal_row_height(mono_size: Pixels) -> f32 {
    (mono_size.as_f32() * TERMINAL_LINE_HEIGHT_FACTOR).round()
}

/// Shell working directory: the workspace, or home when none is open.
fn terminal_shell_dir(workspace: Option<&PathBuf>) -> Option<PathBuf> {
    workspace.cloned().or_else(dirs::home_dir)
}

/// Shorten a directory for the tab label: home becomes `~`, anything else
/// renders whole (the tab truncates). Control characters are stripped:
/// live directories arrive from shell output, and a newline in the label
/// would break the single-line tab layout.
fn terminal_short_dir(path: &Path) -> String {
    let text = if let Some(home) = dirs::home_dir()
        && let Ok(stripped) = path.strip_prefix(&home)
    {
        format!("~/{}", stripped.display())
    } else {
        path.to_string_lossy().into_owned()
    };
    text.chars().filter(|cell| !cell.is_control()).collect()
}

/// Grid rows for a panel height: floor toward fit (a partial row would clip),
/// at least one, capped for sanity. `None` when uncomputable (caller keeps
/// current rows).
pub(super) fn terminal_rows_for_height(height_px: f32, row_height_px: f32) -> Option<u16> {
    if !height_px.is_finite() || height_px <= 0.0 || !row_height_px.is_finite() || row_height_px <= 0.0 {
        return None;
    }
    let rows = (height_px / row_height_px).floor().clamp(1.0, 512.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped integral 1..=512; conversion is exact and positive"
    )]
    Some(rows as u16)
}

/// Grid rows for the current panel geometry: the panel height minus the
/// header strip and vertical grid padding, over the measured row height.
pub(super) fn terminal_content_rows(panel_height_px: f32, mono_size: Pixels) -> Option<u16> {
    terminal_rows_for_height(
        panel_height_px - TERMINAL_HEADER_HEIGHT - 2.0 * TERMINAL_GRID_PAD_Y,
        terminal_row_height(mono_size),
    )
}

/// Grid columns for a content width: floor toward fit (a partial column
/// would clip), at least one, capped for sanity. `None` when uncomputable
/// (caller keeps current columns).
pub(super) fn terminal_cols_for_width(width_px: f32, char_width_px: f32) -> Option<u16> {
    if !width_px.is_finite() || width_px <= 0.0 || !char_width_px.is_finite() || char_width_px <= 0.0 {
        return None;
    }
    let columns = (width_px / char_width_px).floor().clamp(1.0, 512.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped integral 1..=512; conversion is exact and positive"
    )]
    Some(columns as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_rows_for_height_floors_and_guards() {
        assert_eq!(terminal_rows_for_height(240.0, 18.0), Some(13));
        assert_eq!(terminal_rows_for_height(36.0, 18.0), Some(2));
        assert_eq!(terminal_rows_for_height(10.0, 18.0), Some(1));
        assert_eq!(terminal_rows_for_height(10000.0, 18.0), Some(512));
        assert_eq!(terminal_rows_for_height(-5.0, 18.0), None);
        assert_eq!(terminal_rows_for_height(240.0, 0.0), None);
        assert_eq!(terminal_rows_for_height(f32::NAN, 18.0), None);
        assert_eq!(terminal_rows_for_height(240.0, f32::NAN), None);
    }

    #[test]
    fn terminal_row_height_scales_mono_size() {
        assert_eq!(terminal_row_height(px(13.)), 21.0);
        assert_eq!(terminal_row_height(px(16.)), 26.0);
    }

    #[test]
    fn terminal_cell_for_point_maps_offsets_and_clamps() {
        use gpui_kit::{point, px};
        let origin = point(px(10.), px(20.));
        // Exact cell corners floor into their cell; centers land inside.
        assert_eq!(terminal_cell_for_point(point(px(10.), px(20.)), origin, 21.0, 8.0, 4, 40), Some((0, 0)));
        assert_eq!(terminal_cell_for_point(point(px(34.), px(62.)), origin, 21.0, 8.0, 4, 40), Some((2, 3)));
        // Past the text but inside the grid: clamp to the last column.
        assert_eq!(
            terminal_cell_for_point(point(px(10. + 80. * 8.), px(20.)), origin, 21.0, 8.0, 4, 80),
            Some((0, 79))
        );
        // Before the grid or past its rows: fail closed.
        assert_eq!(terminal_cell_for_point(point(px(9.), px(20.)), origin, 21.0, 8.0, 4, 40), None);
        assert_eq!(terminal_cell_for_point(point(px(10.), px(19.)), origin, 21.0, 8.0, 4, 40), None);
        assert_eq!(terminal_cell_for_point(point(px(10.), px(20. + 4. * 21.)), origin, 21.0, 8.0, 4, 40), None);
        // Degenerate metrics or grids never map.
        assert_eq!(terminal_cell_for_point(origin, origin, 0.0, 8.0, 4, 40), None);
        assert_eq!(terminal_cell_for_point(origin, origin, 21.0, 0.0, 4, 40), None);
        assert_eq!(terminal_cell_for_point(origin, origin, 21.0, 8.0, 0, 40), None);
    }

    #[test]
    fn terminal_cols_for_width_floors_and_clamps() {
        assert_eq!(terminal_cols_for_width(800.0, 8.0), Some(100));
        assert_eq!(terminal_cols_for_width(803.0, 8.0), Some(100));
        assert_eq!(terminal_cols_for_width(7.0, 8.0), Some(1));
        assert_eq!(terminal_cols_for_width(100_000.0, 8.0), Some(512));
        assert_eq!(terminal_cols_for_width(0.0, 8.0), None);
        assert_eq!(terminal_cols_for_width(800.0, 0.0), None);
        assert_eq!(terminal_cols_for_width(f32::NAN, 8.0), None);
        assert_eq!(terminal_cols_for_width(800.0, f32::INFINITY), None);
    }

    #[test]
    fn terminal_content_rows_subtracts_header_and_padding() {
        // 240px panel - 32px header - 16px vertical padding = 192px of rows.
        assert_eq!(terminal_content_rows(240.0, px(13.)), Some(9));
        assert_eq!(terminal_content_rows(48.0, px(13.)), None);
        assert_eq!(terminal_content_rows(240.0, px(0.)), None);
    }

    #[test]
    fn terminal_short_dir_collapses_home_prefix() {
        let home = dirs::home_dir().expect("test env must have a home");
        assert_eq!(terminal_short_dir(&home), "~/");
        assert_eq!(terminal_short_dir(&home.join("code/vtcode")), "~/code/vtcode");
        assert_eq!(terminal_short_dir(Path::new("/tmp/x")), "/tmp/x");
        assert_eq!(terminal_short_dir(Path::new("/tmp/a\nb\tc")), "/tmp/abc");
    }

    #[test]
    fn terminal_palette_auto_follows_mode_and_overrides_win() {
        use gpui_kit::component::theme::ThemeMode;
        use preferences::TerminalAnsiTheme;
        assert!(terminal_palette_is_dark(TerminalAnsiTheme::Auto, ThemeMode::Dark));
        assert!(!terminal_palette_is_dark(TerminalAnsiTheme::Auto, ThemeMode::Light));
        assert!(terminal_palette_is_dark(TerminalAnsiTheme::Dark, ThemeMode::Light));
        assert!(!terminal_palette_is_dark(TerminalAnsiTheme::Light, ThemeMode::Dark));
    }
}
