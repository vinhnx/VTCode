//! ANSI escape sequence constants and utilities

use once_cell::sync::Lazy;
use ratatui::crossterm::tty::IsTty;
use std::io::Write;

/// Escape character (ESC = 0x1B = 27)
pub const ESC: &str = "\x1b";

/// Control Sequence Introducer (CSI = ESC[)
pub const CSI: &str = "\x1b[";

/// Operating System Command (OSC = ESC])
pub const OSC: &str = "\x1b]";

/// Device Control String (DCS = ESC P)
pub const DCS: &str = "\x1bP";

/// String Terminator (ST = ESC \)
pub const ST: &str = "\x1b\\";

/// Bell character (BEL = 0x07)
pub const BEL: &str = "\x07";

/// Notification preference (rich OSC vs bell-only)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitlNotifyMode {
    Off,
    Bell,
    Rich,
}

/// Terminal-specific notification capabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalNotifyKind {
    BellOnly,
    Osc9,
    Osc777,
}

static DETECTED_NOTIFY_KIND: Lazy<TerminalNotifyKind> = Lazy::new(detect_terminal_notify_kind);

/// Play the terminal bell when enabled.
#[inline]
pub fn play_bell(enabled: bool) {
    if !is_bell_enabled(enabled) {
        return;
    }
    emit_bell();
}

/// Determine whether the bell should play, honoring an env override.
#[inline]
pub fn is_bell_enabled(default_enabled: bool) -> bool {
    if let Ok(val) = std::env::var("VTCODE_HITL_BELL") {
        return !matches!(
            val.trim().to_ascii_lowercase().as_str(),
            "false" | "0" | "off"
        );
    }
    default_enabled
}

#[inline]
fn emit_bell() {
    print!("{}", BEL);
    let _ = std::io::stdout().flush();
}

#[inline]
pub fn notify_attention(default_enabled: bool, message: Option<&str>) {
    if !is_bell_enabled(default_enabled) {
        return;
    }

    if !std::io::stdout().is_tty() {
        return;
    }

    let mode = hitl_notify_mode(default_enabled);
    if matches!(mode, HitlNotifyMode::Off) {
        return;
    }

    if matches!(mode, HitlNotifyMode::Rich) {
        match *DETECTED_NOTIFY_KIND {
            TerminalNotifyKind::Osc9 => send_osc9_notification(message),
            TerminalNotifyKind::Osc777 => send_osc777_notification(message),
            TerminalNotifyKind::BellOnly => {} // No-op
        }
    }

    emit_bell();
}

fn hitl_notify_mode(default_enabled: bool) -> HitlNotifyMode {
    if let Ok(raw) = std::env::var("VTCODE_HITL_NOTIFY") {
        let v = raw.trim().to_ascii_lowercase();
        return match v.as_str() {
            "off" | "0" | "false" => HitlNotifyMode::Off,
            "bell" => HitlNotifyMode::Bell,
            "rich" | "osc" | "notify" => HitlNotifyMode::Rich,
            _ => HitlNotifyMode::Bell,
        };
    }

    if default_enabled {
        HitlNotifyMode::Rich
    } else {
        HitlNotifyMode::Off
    }
}

fn detect_terminal_notify_kind() -> TerminalNotifyKind {
    if let Ok(explicit_kind) = std::env::var("VTCODE_NOTIFY_KIND") {
        let explicit = explicit_kind.trim().to_ascii_lowercase();
        return match explicit.as_str() {
            "osc9" => TerminalNotifyKind::Osc9,
            "osc777" => TerminalNotifyKind::Osc777,
            "bell" | "off" => TerminalNotifyKind::BellOnly,
            _ => TerminalNotifyKind::BellOnly,
        };
    }

    let term = std::env::var("TERM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let term_program = std::env::var("TERM_PROGRAM")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let has_kitty = std::env::var("KITTY_WINDOW_ID").is_ok();
    let has_iterm = std::env::var("ITERM_SESSION_ID").is_ok();
    let has_wezterm = std::env::var("WEZTERM_PANE").is_ok();
    let has_vte = std::env::var("VTE_VERSION").is_ok();

    detect_terminal_notify_kind_from(
        &term,
        &term_program,
        has_kitty,
        has_iterm,
        has_wezterm,
        has_vte,
    )
}

fn send_osc777_notification(message: Option<&str>) {
    let body = sanitize_notification_text(message.unwrap_or("Human approval required"));
    let title = sanitize_notification_text("VT Code");
    let payload = build_osc777_payload(&title, &body);
    print!("{}{}", payload, BEL);
    let _ = std::io::stdout().flush();
}

fn send_osc9_notification(message: Option<&str>) {
    let body = sanitize_notification_text(message.unwrap_or("Human approval required"));
    let payload = build_osc9_payload(&body);
    print!("{}{}", payload, BEL);
    let _ = std::io::stdout().flush();
}

fn sanitize_notification_text(raw: &str) -> String {
    const MAX_LEN: usize = 200;
    let mut cleaned = raw
        .chars()
        .filter(|c| *c >= ' ' && *c != '\u{007f}')
        .collect::<String>();
    if cleaned.len() > MAX_LEN {
        cleaned.truncate(MAX_LEN);
    }
    cleaned.replace(';', ":")
}

fn detect_terminal_notify_kind_from(
    term: &str,
    term_program: &str,
    has_kitty: bool,
    has_iterm: bool,
    has_wezterm: bool,
    has_vte: bool,
) -> TerminalNotifyKind {
    if term.contains("kitty") || has_kitty {
        return TerminalNotifyKind::Osc777;
    }

    if term_program.contains("ghostty")
        || term_program.contains("iterm")
        || term_program.contains("wezterm")
        || term_program.contains("warp")
        || term_program.contains("apple_terminal")
        || has_iterm
        || has_wezterm
    {
        return TerminalNotifyKind::Osc9;
    }

    if has_vte {
        return TerminalNotifyKind::Osc777;
    }

    TerminalNotifyKind::BellOnly
}

fn build_osc777_payload(title: &str, body: &str) -> String {
    format!("{}777;notify;{};{}", OSC, title, body)
}

fn build_osc9_payload(body: &str) -> String {
    format!("{}9;{}", OSC, body)
}

#[cfg(test)]
mod redraw_tests {
    use super::*;

    #[test]
    fn terminal_mapping_is_deterministic() {
        assert_eq!(
            detect_terminal_notify_kind_from("xterm-kitty", "", false, false, false, false),
            TerminalNotifyKind::Osc777
        );
        assert_eq!(
            detect_terminal_notify_kind_from(
                "xterm-ghostty",
                "ghostty",
                false,
                false,
                false,
                false
            ),
            TerminalNotifyKind::Osc9
        );
        assert_eq!(
            detect_terminal_notify_kind_from(
                "xterm-256color",
                "wezterm",
                false,
                false,
                false,
                false
            ),
            TerminalNotifyKind::Osc9
        );
        assert_eq!(
            detect_terminal_notify_kind_from("xterm-256color", "", false, false, false, true),
            TerminalNotifyKind::Osc777
        );
        assert_eq!(
            detect_terminal_notify_kind_from("xterm-256color", "", false, false, false, false),
            TerminalNotifyKind::BellOnly
        );
    }

    #[test]
    fn osc_payload_format_is_stable() {
        assert_eq!(build_osc9_payload("done"), format!("{}9;done", OSC));
        assert_eq!(
            build_osc777_payload("VT Code", "finished"),
            format!("{}777;notify;VT Code;finished", OSC)
        );
    }
}

// === Reset ===
pub const RESET: &str = "\x1b[0m";

// === Text Styles ===
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const ITALIC: &str = "\x1b[3m";
pub const UNDERLINE: &str = "\x1b[4m";
pub const BLINK: &str = "\x1b[5m";
pub const REVERSE: &str = "\x1b[7m";
pub const HIDDEN: &str = "\x1b[8m";
pub const STRIKETHROUGH: &str = "\x1b[9m";

pub const RESET_BOLD_DIM: &str = "\x1b[22m";
pub const RESET_ITALIC: &str = "\x1b[23m";
pub const RESET_UNDERLINE: &str = "\x1b[24m";
pub const RESET_BLINK: &str = "\x1b[25m";
pub const RESET_REVERSE: &str = "\x1b[27m";
pub const RESET_HIDDEN: &str = "\x1b[28m";
pub const RESET_STRIKETHROUGH: &str = "\x1b[29m";

// === Foreground Colors (30-37) ===
pub const FG_BLACK: &str = "\x1b[30m";
pub const FG_RED: &str = "\x1b[31m";
pub const FG_GREEN: &str = "\x1b[32m";
pub const FG_YELLOW: &str = "\x1b[33m";
pub const FG_BLUE: &str = "\x1b[34m";
pub const FG_MAGENTA: &str = "\x1b[35m";
pub const FG_CYAN: &str = "\x1b[36m";
pub const FG_WHITE: &str = "\x1b[37m";
pub const FG_DEFAULT: &str = "\x1b[39m";

// === Background Colors (40-47) ===
pub const BG_BLACK: &str = "\x1b[40m";
pub const BG_RED: &str = "\x1b[41m";
pub const BG_GREEN: &str = "\x1b[42m";
pub const BG_YELLOW: &str = "\x1b[43m";
pub const BG_BLUE: &str = "\x1b[44m";
pub const BG_MAGENTA: &str = "\x1b[45m";
pub const BG_CYAN: &str = "\x1b[46m";
pub const BG_WHITE: &str = "\x1b[47m";
pub const BG_DEFAULT: &str = "\x1b[49m";

// === Bright Foreground Colors (90-97) ===
pub const FG_BRIGHT_BLACK: &str = "\x1b[90m";
pub const FG_BRIGHT_RED: &str = "\x1b[91m";
pub const FG_BRIGHT_GREEN: &str = "\x1b[92m";
pub const FG_BRIGHT_YELLOW: &str = "\x1b[93m";
pub const FG_BRIGHT_BLUE: &str = "\x1b[94m";
pub const FG_BRIGHT_MAGENTA: &str = "\x1b[95m";
pub const FG_BRIGHT_CYAN: &str = "\x1b[96m";
pub const FG_BRIGHT_WHITE: &str = "\x1b[97m";

// === Bright Background Colors (100-107) ===
pub const BG_BRIGHT_BLACK: &str = "\x1b[100m";
pub const BG_BRIGHT_RED: &str = "\x1b[101m";
pub const BG_BRIGHT_GREEN: &str = "\x1b[102m";
pub const BG_BRIGHT_YELLOW: &str = "\x1b[103m";
pub const BG_BRIGHT_BLUE: &str = "\x1b[104m";
pub const BG_BRIGHT_MAGENTA: &str = "\x1b[105m";
pub const BG_BRIGHT_CYAN: &str = "\x1b[106m";
pub const BG_BRIGHT_WHITE: &str = "\x1b[107m";

// === Cursor Control ===
pub const CURSOR_HOME: &str = "\x1b[H";
pub const CURSOR_HIDE: &str = "\x1b[?25l";
pub const CURSOR_SHOW: &str = "\x1b[?25h";
pub const CURSOR_SAVE_DEC: &str = "\x1b7";
pub const CURSOR_RESTORE_DEC: &str = "\x1b8";
pub const CURSOR_SAVE_SCO: &str = "\x1b[s";
pub const CURSOR_RESTORE_SCO: &str = "\x1b[u";

// === Erase Functions ===
pub const CLEAR_SCREEN: &str = "\x1b[2J";
pub const CLEAR_TO_END_OF_SCREEN: &str = "\x1b[0J";
pub const CLEAR_TO_START_OF_SCREEN: &str = "\x1b[1J";
pub const CLEAR_SAVED_LINES: &str = "\x1b[3J";
pub const CLEAR_LINE: &str = "\x1b[2K";
pub const CLEAR_TO_END_OF_LINE: &str = "\x1b[0K";
pub const CLEAR_TO_START_OF_LINE: &str = "\x1b[1K";

// === Screen Modes ===
pub const ALT_BUFFER_ENABLE: &str = "\x1b[?1049h";
pub const ALT_BUFFER_DISABLE: &str = "\x1b[?1049l";
pub const SCREEN_SAVE: &str = "\x1b[?47h";
pub const SCREEN_RESTORE: &str = "\x1b[?47l";
pub const LINE_WRAP_ENABLE: &str = "\x1b[=7h";
pub const LINE_WRAP_DISABLE: &str = "\x1b[=7l";

// === Helper Functions ===

#[inline]
pub fn cursor_up(n: u16) -> String {
    format!("\x1b[{}A", n)
}

#[inline]
pub fn cursor_down(n: u16) -> String {
    format!("\x1b[{}B", n)
}

#[inline]
pub fn cursor_right(n: u16) -> String {
    format!("\x1b[{}C", n)
}

#[inline]
pub fn cursor_left(n: u16) -> String {
    format!("\x1b[{}D", n)
}

#[inline]
pub fn cursor_to(row: u16, col: u16) -> String {
    format!("\x1b[{};{}H", row, col)
}

/// Build a portable in-place redraw prefix (`CR` + `EL2`).
///
/// This is the common CLI pattern for one-line progress updates.
#[inline]
pub fn redraw_line_prefix() -> &'static str {
    "\r\x1b[2K"
}

/// Format a one-line in-place update payload.
///
/// Equivalent to: `\\r\\x1b[2K{content}`.
#[inline]
pub fn format_redraw_line(content: &str) -> String {
    format!("{}{}", redraw_line_prefix(), content)
}

#[inline]
pub fn fg_256(color_id: u8) -> String {
    format!("\x1b[38;5;{}m", color_id)
}

#[inline]
pub fn bg_256(color_id: u8) -> String {
    format!("\x1b[48;5;{}m", color_id)
}

#[inline]
pub fn fg_rgb(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[38;2;{};{};{}m", r, g, b)
}

#[inline]
pub fn bg_rgb(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[48;2;{};{};{}m", r, g, b)
}

#[inline]
pub fn colored(text: &str, color: &str) -> String {
    format!("{}{}{}", color, text, RESET)
}

#[inline]
pub fn bold(text: &str) -> String {
    format!("{}{}{}", BOLD, text, RESET_BOLD_DIM)
}

#[inline]
pub fn italic(text: &str) -> String {
    format!("{}{}{}", ITALIC, text, RESET_ITALIC)
}

#[inline]
pub fn underline(text: &str) -> String {
    format!("{}{}{}", UNDERLINE, text, RESET_UNDERLINE)
}

#[inline]
pub fn dim(text: &str) -> String {
    format!("{}{}{}", DIM, text, RESET_BOLD_DIM)
}

#[inline]
pub fn combine_styles(text: &str, styles: &[&str]) -> String {
    let mut result = String::with_capacity(text.len() + styles.len() * 10);
    for style in styles {
        result.push_str(style);
    }
    result.push_str(text);
    result.push_str(RESET);
    result
}

pub mod semantic {
    use super::*;
    pub const ERROR: &str = FG_BRIGHT_RED;
    pub const SUCCESS: &str = FG_BRIGHT_GREEN;
    pub const WARNING: &str = FG_BRIGHT_YELLOW;
    pub const INFO: &str = FG_BRIGHT_CYAN;
    pub const MUTED: &str = DIM;
    pub const EMPHASIS: &str = BOLD;
    pub const DEBUG: &str = FG_BRIGHT_BLACK;
}

#[inline]
pub fn contains_ansi(text: &str) -> bool {
    text.contains(ESC)
}

#[inline]
pub fn starts_with_ansi(text: &str) -> bool {
    text.starts_with(ESC)
}

#[inline]
pub fn ends_with_ansi(text: &str) -> bool {
    text.ends_with('m') && text.contains(ESC)
}

#[inline]
pub fn display_width(text: &str) -> usize {
    crate::ansi::strip_ansi(text).len()
}

pub fn pad_to_width(text: &str, width: usize, pad_char: char) -> String {
    let current_width = display_width(text);
    if current_width >= width {
        text.to_string()
    } else {
        let padding = pad_char.to_string().repeat(width - current_width);
        format!("{}{}", text, padding)
    }
}

pub fn truncate_to_width(text: &str, max_width: usize, ellipsis: &str) -> String {
    let stripped = crate::ansi::strip_ansi(text);
    if stripped.len() <= max_width {
        return text.to_string();
    }

    let truncate_at = max_width.saturating_sub(ellipsis.len());
    let truncated_plain: String = stripped.chars().take(truncate_at).collect();

    if starts_with_ansi(text) {
        let mut ansi_prefix = String::new();
        for ch in text.chars() {
            ansi_prefix.push(ch);
            if ch == '\x1b' {
                continue;
            }
            if ch.is_alphabetic() && ansi_prefix.contains('\x1b') {
                break;
            }
        }
        format!("{}{}{}{}", ansi_prefix, truncated_plain, ellipsis, RESET)
    } else {
        format!("{}{}", truncated_plain, ellipsis)
    }
}

#[inline]
pub fn write_styled<W: std::io::Write>(
    writer: &mut W,
    text: &str,
    style: &str,
) -> std::io::Result<()> {
    writer.write_all(style.as_bytes())?;
    writer.write_all(text.as_bytes())?;
    writer.write_all(RESET.as_bytes())?;
    Ok(())
}

#[inline]
pub fn format_styled_into(buffer: &mut String, text: &str, style: &str) {
    buffer.push_str(style);
    buffer.push_str(text);
    buffer.push_str(RESET);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redraw_prefix_matches_cli_pattern() {
        assert_eq!(redraw_line_prefix(), "\r\x1b[2K");
    }

    #[test]
    fn redraw_line_formats_expected_sequence() {
        assert_eq!(format_redraw_line("Done"), "\r\x1b[2KDone");
    }
}
