//! Terminal input translation: keystrokes to PTY bytes, reserved-chord
//! recognizers, scroll targets, wheel deltas, and copy guards. Split from
//! `terminal_panel.rs` to hold the 1000-line ceiling.

/// Wheel pixels to whole history lines. Sign follows GPUI lists
/// (`scroll_top - delta.y`): wheel-up is positive `delta.y`, which is also
/// Rio's `Delta` direction into history, so no flip is needed. Returns the
/// whole lines plus the kept fractional remainder.
pub(super) fn wheel_line_delta(delta: &gpui_kit::ScrollDelta, row_height: f32, remainder: f32) -> (i32, f32) {
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

/// Shifted navigation scrolls history instead of reaching the shell.
pub(super) fn scroll_target(keystroke: &gpui_kit::Keystroke) -> Option<rio_vt::crosswords::grid::Scroll> {
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
pub(super) fn is_copy(keystroke: &gpui_kit::Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    modifiers.platform && !modifiers.control && !modifiers.alt && keystroke.key == "c"
}

/// Select-all chord: plain `cmd-a` only.
pub(super) fn is_select_all(keystroke: &gpui_kit::Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    modifiers.platform && !modifiers.control && !modifiers.alt && keystroke.key == "a"
}

/// Clipboard-bound selection text: blank selections never clobber the clipboard.
pub(super) fn copyable_text(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

/// Bare `escape` clears a selection instead of reaching the shell.
pub(super) fn is_bare_escape(keystroke: &gpui_kit::Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    !modifiers.platform && !modifiers.control && !modifiers.alt && !modifiers.function && keystroke.key == "escape"
}

/// Paste chord: plain `cmd-v` only, so modified variants keep bubbling.
pub(super) fn is_paste(keystroke: &gpui_kit::Keystroke) -> bool {
    let modifiers = &keystroke.modifiers;
    modifiers.platform && !modifiers.control && !modifiers.alt && keystroke.key == "v"
}

/// Translate one keystroke to terminal input bytes, or `None` to let it
/// bubble (menus, reserved chords, keys the shell must never see). Paste is
/// handled by the caller, which owns the clipboard.
pub(super) fn key_bytes(keystroke: &gpui_kit::Keystroke, app_cursor: bool) -> Option<Vec<u8>> {
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

pub(super) fn cursor_bytes(app_cursor: bool, final_byte: u8) -> Vec<u8> {
    if app_cursor {
        vec![0x1b, b'O', final_byte]
    } else {
        vec![0x1b, b'[', final_byte]
    }
}

/// Control codes for the allowlisted keys only; anything else bubbles rather
/// than risk a wrong byte (a wrong control code can kill foreground jobs).
pub(super) fn control_bytes(key: &str) -> Option<Vec<u8>> {
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
