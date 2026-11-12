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
/// Grid columns: fixed until measured-width lands.
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
        let palette = super::super::terminal_tab::palette_for_theme(&cx.theme().colors);
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

    /// Recompute grid rows from the stored panel height; no-op when unmeasurable.
    fn sync_terminal_grid(&mut self, cx: &mut Context<Self>) {
        let row_height = terminal_row_height(cx.theme().mono_font_size);
        let Some(rows) = terminal_rows_for_height(self.terminal_panel_height, row_height) else {
            return;
        };
        let Some(tab) = self.terminal.as_mut() else {
            return;
        };
        if tab.grid_size().rows != rows {
            let size = portable_pty::PtySize { rows, ..tab.grid_size() };
            let _resized = tab.resize(size);
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
        if let Some(bytes) = key_bytes(keystroke, tab.app_cursor()) {
            let _sent = tab.input(bytes);
            cx.stop_propagation();
        }
    }

    pub(super) fn render_terminal_panel(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let mono = cx.theme().mono_font_family.clone();
        let mono_size = cx.theme().mono_font_size;
        let mut rows = div().flex().flex_col().w_full();
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
                cx.listener(|view, _: &MouseDownEvent, window, cx| {
                    view.terminal_focus.focus(window, cx);
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
                    .child(rows),
            )
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
}
