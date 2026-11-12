//! Bottom terminal panel: visibility, drain pump, and grid rows.
//!
//! One session per workspace (no tabs): opening spawns lazily in the workspace
//! directory, hiding keeps it alive, switching workspaces closes it. Invariant:
//! `terminal_visible` implies a live `terminal` session. A generation-guarded
//! task drains PTY output and redraws on change; floods pace out over frames.

use super::*;

/// Fixed panel height until the region becomes resizable. Pixels are the
/// sanctioned exception for fixed pane geometry.
const TERMINAL_PANEL_HEIGHT: f32 = 240.0;

/// Drain cadence: bounded redraws without a busy loop on floods.
const TERMINAL_PUMP_INTERVAL: Duration = Duration::from_millis(16);
/// Fallback grid rows when unmeasurable (xterm standard).
const DEFAULT_TERMINAL_ROWS: u16 = 24;
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

    /// Show the panel, spawning the workspace shell on first open. Without a
    /// workspace there is nothing to scope a shell to, so this is a no-op.
    fn show_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_none() {
            return;
        }
        if !self.ensure_terminal(cx) {
            return;
        }
        self.terminal_pump_generation = self.terminal_pump_generation.wrapping_add(1);
        let generation = self.terminal_pump_generation;
        self.terminal_visible = true;
        self.terminal_focus.focus(window, cx);
        cx.notify();
        let _pump = cx.spawn(async move |view, cx| {
            loop {
                let keep_going = view
                    .update(cx, |view, cx| view.pump_terminal_once(generation, cx))
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
                cx.background_executor().timer(TERMINAL_PUMP_INTERVAL).await;
            }
        });
    }

    fn hide_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_visible = false;
        self.focus_handle.focus(window, cx);
        cx.notify();
    }

    /// Spawn the workspace shell if none exists. Returns whether a session is
    /// live afterwards; failures surface through the shared error banner.
    fn ensure_terminal(&mut self, cx: &mut Context<Self>) -> bool {
        if self.terminal.is_some() {
            return true;
        }
        let Some(workspace) = self.workspace.clone() else {
            return false;
        };
        let dark = terminal_palette_is_dark(self.preferences.terminal_ansi_theme(), self.theme_mode);
        let palette = super::super::terminal_tab::palette_for_theme(&cx.theme().colors, dark);
        let rows = terminal_rows_for_height(self.terminal_panel_height, terminal_row_height(cx.theme().mono_font_size))
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

    /// Recompute grid rows from the stored panel height and columns from
    /// the laid-out grid width; no-op until measurable. Either dimension
    /// resizes the PTY only when it actually changed.
    fn sync_terminal_grid(&mut self, cx: &mut Context<Self>) {
        let row_height = terminal_row_height(cx.theme().mono_font_size);
        let Some(rows) = terminal_rows_for_height(self.terminal_panel_height, row_height) else {
            return;
        };
        let columns = self.terminal_grid_bounds.get().and_then(|grid| {
            let char_width =
                terminal_column_width(cx.text_system(), &cx.theme().mono_font_family, cx.theme().mono_font_size);
            terminal_cols_for_width(grid.size.width.as_f32(), char_width)
        });
        let Some(tab) = self.terminal.as_mut() else {
            return;
        };
        let size = tab.grid_size();
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
            cx.notify();
            cx.stop_propagation();
            return;
        }
        // Clearing never swallows the key: the shell still receives Escape below.
        if is_bare_escape(keystroke) && tab.clear_selection() {
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
        tab.start_selection(anchor.0, anchor.1, rio_vt::crosswords::pos::Side::Left);
        self.terminal_select_drag = Some(TerminalSelectDrag { anchor, char_width });
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
        tab.start_selection(start.0, start.1, rio_vt::crosswords::pos::Side::Left);
        tab.update_selection(end.0, end.1, rio_vt::crosswords::pos::Side::Right);
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
        let mut rows = div().flex().flex_col().w_full();
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
            .aria_label("Terminal")
            .w_full()
            .h(px(TERMINAL_PANEL_HEIGHT))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(colors.group_box)
            .child(
                div()
                    .id("terminal-scroll")
                    .test_support()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.terminal_scroll)
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

/// Zero-height geometry probe, mounted as the first grid child at full
/// content width. Its paint-time bounds give mouse handlers the content
/// origin for cell mapping and grid sync the content width for columns;
/// the shared sink carries them back to the view.
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
        let mut style = gpui_kit::Style::default();
        style.size.width = gpui_kit::relative(1.).into();
        style.size.height = px(0.).into();
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

/// Wheel pixels to whole history lines. Sign follows GPUI lists
/// (`scroll_top - delta.y`): wheel-up is positive `delta.y`, which is also
/// Rio's `Delta` direction into history, so no flip is needed. Returns the
/// whole lines plus the kept fractional remainder.
fn wheel_line_delta(delta: &gpui_kit::ScrollDelta, row_height: f32, remainder: f32) -> (i32, f32) {
    let dy_px = match delta {
        gpui_kit::ScrollDelta::Pixels(point) => f32::from(point.y),
        gpui_kit::ScrollDelta::Lines(point) => point.y * row_height,
    };
    if !dy_px.is_finite() || !remainder.is_finite() || row_height <= 0.0 {
        return (0, 0.0);
    }
    let total = (remainder + dy_px / row_height).clamp(-10_000.0, 10_000.0);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped integral -10000..=10000; conversion is exact"
    )]
    let lines = total.trunc() as i32;
    (lines, total - lines as f32)
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

/// Shifted navigation scrolls history instead of reaching the shell.
fn scroll_target(keystroke: &gpui_kit::Keystroke) -> Option<rio_vt::crosswords::grid::Scroll> {
    use rio_vt::crosswords::grid::Scroll;
    let modifiers = &keystroke.modifiers;
    if !modifiers.shift || modifiers.platform || modifiers.control || modifiers.alt {
        return None;
    }
    match keystroke.key.as_str() {
        "pageup" => Some(Scroll::PageUp),
        "pagedown" => Some(Scroll::PageDown),
        "home" => Some(Scroll::Top),
        "end" => Some(Scroll::Bottom),
        _ => None,
    }
}

/// Copy chord: plain `cmd-c` only.
fn is_copy(keystroke: &gpui_kit::Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    modifiers.platform && !modifiers.control && !modifiers.alt && keystroke.key == "c"
}

/// Select-all chord: plain `cmd-a` only.
fn is_select_all(keystroke: &gpui_kit::Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    modifiers.platform && !modifiers.control && !modifiers.alt && keystroke.key == "a"
}

/// Clipboard-bound selection text: blank selections never clobber the clipboard.
fn copyable_text(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

/// Bare `escape` clears a selection instead of reaching the shell.
fn is_bare_escape(keystroke: &gpui_kit::Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    !modifiers.platform && !modifiers.control && !modifiers.alt && !modifiers.function && keystroke.key == "escape"
}

/// Paste chord: plain `cmd-v` only, so modified variants keep bubbling.
fn is_paste(keystroke: &gpui_kit::Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    modifiers.platform && !modifiers.control && !modifiers.alt && keystroke.key == "v"
}

/// Translate one keystroke to terminal input bytes, or `None` to let it
/// bubble (menus, reserved chords, keys the shell must never see). Paste is
/// handled by the caller, which owns the clipboard.
fn key_bytes(keystroke: &gpui_kit::Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
    let modifiers = &keystroke.modifiers;
    if modifiers.platform || (modifiers.control && modifiers.alt) {
        return None;
    }
    if modifiers.control {
        return control_bytes(&keystroke.key);
    }
    match keystroke.key.as_str() {
        "enter" => Some(b"\r".to_vec()),
        "backspace" => Some(vec![0x7f]),
        "delete" => Some(b"\x1b[3~".to_vec()),
        "tab" => Some(b"\t".to_vec()),
        "escape" => Some(b"\x1b".to_vec()),
        "space" => Some(b" ".to_vec()),
        "left" => Some(cursor_bytes(app_cursor, b'D')),
        "right" => Some(cursor_bytes(app_cursor, b'C')),
        "up" => Some(cursor_bytes(app_cursor, b'A')),
        "down" => Some(cursor_bytes(app_cursor, b'B')),
        "home" => Some(b"\x1b[H".to_vec()),
        "end" => Some(b"\x1b[F".to_vec()),
        "pageup" => Some(b"\x1b[5~".to_vec()),
        "pagedown" => Some(b"\x1b[6~".to_vec()),
        "f1" => Some(b"\x1bOP".to_vec()),
        "f2" => Some(b"\x1bOQ".to_vec()),
        "f3" => Some(b"\x1bOR".to_vec()),
        "f4" => Some(b"\x1bOS".to_vec()),
        "f5" => Some(b"\x1b[15~".to_vec()),
        "f6" => Some(b"\x1b[17~".to_vec()),
        "f7" => Some(b"\x1b[18~".to_vec()),
        "f8" => Some(b"\x1b[19~".to_vec()),
        "f9" => Some(b"\x1b[20~".to_vec()),
        "f10" => Some(b"\x1b[21~".to_vec()),
        "f11" => Some(b"\x1b[23~".to_vec()),
        "f12" => Some(b"\x1b[24~".to_vec()),
        _ => {
            if let Some(text) = keystroke.key_char.as_deref().filter(|text| !text.is_empty()) {
                return Some(text.as_bytes().to_vec());
            }
            let mut chars = keystroke.key.chars();
            match (chars.next(), chars.next()) {
                (Some(single), None) => Some(single.to_string().into_bytes()),
                _ => None,
            }
        }
    }
}

fn cursor_bytes(app_cursor: bool, final_byte: u8) -> Vec<u8> {
    if app_cursor {
        vec![0x1b, b'O', final_byte]
    } else {
        vec![0x1b, b'[', final_byte]
    }
}

/// Control codes for the allowlisted keys only; anything else bubbles rather
/// than risk a wrong byte (a wrong control code can kill foreground jobs).
fn control_bytes(key: &str) -> Option<Vec<u8>> {
    let key = if key == "space" { " " } else { key };
    let mut chars = key.chars();
    let key = chars.next().filter(|_| chars.next().is_none())?;
    if key == '?' {
        return Some(vec![0x7f]);
    }
    if !matches!(key, 'a'..='z' | 'A'..='Z' | ' ' | '@' | '[' | '\\' | ']' | '^' | '_') {
        return None;
    }
    u8::try_from(key as u32 & 0x1f).ok().map(|code| vec![code])
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

    fn translated(chord: &'static str, app_cursor: bool) -> Option<Vec<u8>> {
        let keystroke = gpui_kit::Keystroke::parse(chord).unwrap().with_simulated_ime();
        key_bytes(&keystroke, app_cursor)
    }

    #[test]
    fn terminal_keys_translate_printables_edits_and_arrows() {
        assert_eq!(translated("a", false).unwrap(), b"a");
        assert_eq!(translated("shift-a", false).unwrap(), b"A");
        assert_eq!(translated("space", false).unwrap(), b" ");
        assert_eq!(translated("enter", false).unwrap(), b"\r");
        assert_eq!(translated("backspace", false).unwrap(), [0x7f]);
        assert_eq!(translated("delete", false).unwrap(), b"\x1b[3~");
        assert_eq!(translated("tab", false).unwrap(), b"\t");
        assert_eq!(translated("escape", false).unwrap(), b"\x1b");
        assert_eq!(translated("left", false).unwrap(), b"\x1b[D");
        assert_eq!(translated("left", true).unwrap(), b"\x1bOD");
        assert_eq!(translated("up", true).unwrap(), b"\x1bOA");
        assert_eq!(translated("home", false).unwrap(), b"\x1b[H");
        assert_eq!(translated("end", false).unwrap(), b"\x1b[F");
        assert_eq!(translated("pageup", false).unwrap(), b"\x1b[5~");
        assert_eq!(translated("pagedown", false).unwrap(), b"\x1b[6~");
        assert_eq!(translated("f1", false).unwrap(), b"\x1bOP");
        assert_eq!(translated("f5", false).unwrap(), b"\x1b[15~");
        assert_eq!(translated("f12", false).unwrap(), b"\x1b[24~");
    }

    #[test]
    fn terminal_keys_translate_control_codes_and_reserve_the_rest() {
        assert_eq!(translated("ctrl-a", false).unwrap(), [0x01]);
        assert_eq!(translated("ctrl-c", false).unwrap(), [0x03]);
        assert_eq!(translated("ctrl-h", false).unwrap(), [0x08]);
        assert_eq!(translated("ctrl-m", false).unwrap(), [0x0d]);
        assert_eq!(translated("ctrl-space", false).unwrap(), [0x00]);
        assert_eq!(translated("ctrl-shift-a", false).unwrap(), [0x01]);
        assert_eq!(translated("ctrl-?", false).unwrap(), [0x7f]);
        assert_eq!(translated("ctrl-2", false), None);
        assert_eq!(translated("cmd-v", false), None);
        assert_eq!(translated("cmd-c", false), None);
        assert_eq!(translated("cmd-a", false), None);
    }

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
    fn terminal_copy_skips_blank_text() {
        assert_eq!(copyable_text(None), None);
        assert_eq!(copyable_text(Some(String::new())), None);
        assert_eq!(copyable_text(Some("  \n  ".into())), None);
        assert_eq!(copyable_text(Some("x".into())), Some("x".into()));
    }

    #[test]
    fn terminal_scroll_chords_need_plain_shift() {
        use rio_vt::crosswords::grid::Scroll;
        let target =
            |chord: &'static str| scroll_target(&gpui_kit::Keystroke::parse(chord).unwrap().with_simulated_ime());
        assert!(matches!(target("shift-pageup"), Some(Scroll::PageUp)));
        assert!(matches!(target("shift-pagedown"), Some(Scroll::PageDown)));
        assert!(matches!(target("shift-home"), Some(Scroll::Top)));
        assert!(matches!(target("shift-end"), Some(Scroll::Bottom)));
        assert!(target("pageup").is_none());
        assert!(target("cmd-shift-pageup").is_none());
        assert!(target("shift-a").is_none());
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
    fn terminal_palette_auto_follows_mode_and_overrides_win() {
        use gpui_kit::component::theme::ThemeMode;
        use preferences::TerminalAnsiTheme;
        assert!(terminal_palette_is_dark(TerminalAnsiTheme::Auto, ThemeMode::Dark));
        assert!(!terminal_palette_is_dark(TerminalAnsiTheme::Auto, ThemeMode::Light));
        assert!(terminal_palette_is_dark(TerminalAnsiTheme::Dark, ThemeMode::Light));
        assert!(!terminal_palette_is_dark(TerminalAnsiTheme::Light, ThemeMode::Dark));
    }

    #[test]
    fn terminal_wheel_delta_accumulates_fractional_rows() {
        use gpui_kit::{ScrollDelta, point, px};
        // Wheel-up is positive delta.y and scrolls into history (positive lines).
        let pixels = |dy: f32| ScrollDelta::Pixels(point(px(0.), px(dy)));
        assert_eq!(wheel_line_delta(&pixels(42.), 21.0, 0.0), (2, 0.0));
        assert_eq!(wheel_line_delta(&pixels(-21.), 21.0, 0.0), (-1, 0.0));
        // Sub-line events accumulate instead of dying: 0.4 + 0.4 + 0.4 rows.
        let (lines, rem) = wheel_line_delta(&pixels(8.4), 21.0, 0.0);
        assert_eq!(lines, 0);
        assert!((rem - 0.4).abs() < 0.001);
        let (lines, rem) = wheel_line_delta(&pixels(8.4), 21.0, rem);
        assert_eq!(lines, 0);
        let (lines, rem) = wheel_line_delta(&pixels(8.4), 21.0, rem);
        assert_eq!(lines, 1);
        assert!(rem < 0.21 && rem > 0.19);
        // Inexact line deltas scale by the terminal row height, not the UI's.
        assert_eq!(wheel_line_delta(&ScrollDelta::Lines(point(0.0, 3.0)), 21.0, 0.0), (3, 0.0));
        // Poison in, zero out: no NaN remainder, no jump.
        assert_eq!(wheel_line_delta(&pixels(f32::NAN), 21.0, 0.0), (0, 0.0));
        assert_eq!(wheel_line_delta(&pixels(42.), 21.0, f32::INFINITY), (0, 0.0));
        assert_eq!(wheel_line_delta(&pixels(42.), 0.0, 0.0), (0, 0.0));
    }
}
