//! Shared ANSI escape sequence constants and small builders for VT Code.
//!
//! See `docs/reference/ansi-in-vtcode.md` for the cross-crate integration map.

use once_cell::sync::Lazy;
use ratatui::crossterm::tty::IsTty;
use std::io::Write;

/// Escape character as a raw byte (ESC = 0x1B = 27)
pub const ESC_BYTE: u8 = 0x1b;

/// Escape character as a `char`
pub const ESC_CHAR: char = '\x1b';

/// Escape character as a string slice
pub const ESC: &str = "\x1b";

/// Control Sequence Introducer (CSI = ESC[)
pub const CSI: &str = "\x1b[";

/// Operating System Command (OSC = ESC])
pub const OSC: &str = "\x1b]";

/// Device Control String (DCS = ESC P)
pub const DCS: &str = "\x1bP";

/// String Terminator (ST = ESC \)
pub const ST: &str = "\x1b\\";

/// Bell character as a raw byte (BEL = 0x07)
pub const BEL_BYTE: u8 = 0x07;

/// Bell character as a `char`
pub const BEL_CHAR: char = '\x07';

/// Bell character as a string slice
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

/// Explicit terminal notification transport override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyMethodOverride {
    Auto,
    Bell,
    Osc9,
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
    notify_attention_with_mode(default_enabled, message, NotifyMethodOverride::Auto);
}

#[inline]
pub fn notify_attention_with_mode(
    default_enabled: bool,
    message: Option<&str>,
    method: NotifyMethodOverride,
) {
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
        let notify_kind = match method {
            NotifyMethodOverride::Auto => *DETECTED_NOTIFY_KIND,
            NotifyMethodOverride::Bell => TerminalNotifyKind::BellOnly,
            NotifyMethodOverride::Osc9 => TerminalNotifyKind::Osc9,
        };
        match notify_kind {
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

    // Ghostty doesn't officially support OSC 9 or OSC 777 notifications
    // Use bell-only to avoid "unknown error" messages
    if term_program.contains("ghostty") {
        return TerminalNotifyKind::BellOnly;
    }

    if term_program.contains("iterm")
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
        // Ghostty doesn't support OSC 9/777, use bell-only to avoid "unknown error"
        assert_eq!(
            detect_terminal_notify_kind_from(
                "xterm-ghostty",
                "ghostty",
                false,
                false,
                false,
                false
            ),
            TerminalNotifyKind::BellOnly
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

// === Scroll Region ===
/// Set Scrolling Region (DECSTBM) — CSI Ps ; Ps r
pub const SCROLL_REGION_RESET: &str = "\x1b[r";

// === Insert / Delete ===
/// Insert Ps Line(s) (default = 1) (IL)
pub const INSERT_LINE: &str = "\x1b[L";
/// Delete Ps Line(s) (default = 1) (DL)
pub const DELETE_LINE: &str = "\x1b[M";
/// Insert Ps Character(s) (default = 1) (ICH)
pub const INSERT_CHAR: &str = "\x1b[@";
/// Delete Ps Character(s) (default = 1) (DCH)
pub const DELETE_CHAR: &str = "\x1b[P";
/// Erase Ps Character(s) (default = 1) (ECH)
pub const ERASE_CHAR: &str = "\x1b[X";

// === Scroll Control ===
/// Scroll up Ps lines (default = 1) (SU)
pub const SCROLL_UP: &str = "\x1b[S";
/// Scroll down Ps lines (default = 1) (SD)
pub const SCROLL_DOWN: &str = "\x1b[T";

// === ESC-level Controls (C1 equivalents) ===
/// Index — move cursor down one line, scroll if at bottom (IND)
pub const INDEX: &str = "\x1bD";
/// Next Line — move to first position of next line (NEL)
pub const NEXT_LINE: &str = "\x1bE";
/// Horizontal Tab Set (HTS)
pub const TAB_SET: &str = "\x1bH";
/// Reverse Index — move cursor up one line, scroll if at top (RI)
pub const REVERSE_INDEX: &str = "\x1bM";
/// Full Reset (RIS) — reset terminal to initial state
pub const FULL_RESET: &str = "\x1bc";
/// Application Keypad (DECPAM)
pub const KEYPAD_APPLICATION: &str = "\x1b=";
/// Normal Keypad (DECPNM)
pub const KEYPAD_NUMERIC: &str = "\x1b>";

// === Mouse Tracking Modes (DECSET/DECRST) ===
/// X10 mouse reporting — button press only (mode 9)
pub const MOUSE_X10_ENABLE: &str = "\x1b[?9h";
pub const MOUSE_X10_DISABLE: &str = "\x1b[?9l";
/// Normal mouse tracking — press and release (mode 1000)
pub const MOUSE_NORMAL_ENABLE: &str = "\x1b[?1000h";
pub const MOUSE_NORMAL_DISABLE: &str = "\x1b[?1000l";
/// Button-event mouse tracking (mode 1002)
pub const MOUSE_BUTTON_EVENT_ENABLE: &str = "\x1b[?1002h";
pub const MOUSE_BUTTON_EVENT_DISABLE: &str = "\x1b[?1002l";
/// Any-event mouse tracking (mode 1003)
pub const MOUSE_ANY_EVENT_ENABLE: &str = "\x1b[?1003h";
pub const MOUSE_ANY_EVENT_DISABLE: &str = "\x1b[?1003l";
/// SGR extended mouse coordinates (mode 1006)
pub const MOUSE_SGR_ENABLE: &str = "\x1b[?1006h";
pub const MOUSE_SGR_DISABLE: &str = "\x1b[?1006l";
/// URXVT extended mouse coordinates (mode 1015)
pub const MOUSE_URXVT_ENABLE: &str = "\x1b[?1015h";
pub const MOUSE_URXVT_DISABLE: &str = "\x1b[?1015l";

// === Terminal Mode Controls (DECSET/DECRST) ===
/// Bracketed Paste Mode (mode 2004)
pub const BRACKETED_PASTE_ENABLE: &str = "\x1b[?2004h";
pub const BRACKETED_PASTE_DISABLE: &str = "\x1b[?2004l";
/// Focus Event Tracking (mode 1004)
pub const FOCUS_EVENT_ENABLE: &str = "\x1b[?1004h";
pub const FOCUS_EVENT_DISABLE: &str = "\x1b[?1004l";
/// Synchronized Output (mode 2026) — batch rendering
pub const SYNC_OUTPUT_BEGIN: &str = "\x1b[?2026h";
pub const SYNC_OUTPUT_END: &str = "\x1b[?2026l";
/// Application Cursor Keys (DECCKM, mode 1)
pub const APP_CURSOR_KEYS_ENABLE: &str = "\x1b[?1h";
pub const APP_CURSOR_KEYS_DISABLE: &str = "\x1b[?1l";
/// Origin Mode (DECOM, mode 6)
pub const ORIGIN_MODE_ENABLE: &str = "\x1b[?6h";
pub const ORIGIN_MODE_DISABLE: &str = "\x1b[?6l";
/// Auto-Wrap Mode (DECAWM, mode 7)
pub const AUTO_WRAP_ENABLE: &str = "\x1b[?7h";
pub const AUTO_WRAP_DISABLE: &str = "\x1b[?7l";

// === Device Status / Attributes ===
/// Primary Device Attributes (DA1) — request
pub const DEVICE_ATTRIBUTES_REQUEST: &str = "\x1b[c";
/// Device Status Report — request cursor position (DSR CPR)
pub const CURSOR_POSITION_REQUEST: &str = "\x1b[6n";
/// Device Status Report — request terminal status
pub const DEVICE_STATUS_REQUEST: &str = "\x1b[5n";

// === OSC Sequences (Operating System Commands) ===
/// Set window title — OSC 2 ; Pt BEL
pub const OSC_SET_TITLE_PREFIX: &str = "\x1b]2;";
/// Set icon name — OSC 1 ; Pt BEL
pub const OSC_SET_ICON_PREFIX: &str = "\x1b]1;";
/// Set icon name and title — OSC 0 ; Pt BEL
pub const OSC_SET_ICON_AND_TITLE_PREFIX: &str = "\x1b]0;";
/// Query/set foreground color — OSC 10
pub const OSC_FG_COLOR_PREFIX: &str = "\x1b]10;";
/// Query/set background color — OSC 11
pub const OSC_BG_COLOR_PREFIX: &str = "\x1b]11;";
/// Query/set cursor color — OSC 12
pub const OSC_CURSOR_COLOR_PREFIX: &str = "\x1b]12;";
/// Hyperlink — OSC 8
pub const OSC_HYPERLINK_PREFIX: &str = "\x1b]8;";
/// Clipboard access — OSC 52
pub const OSC_CLIPBOARD_PREFIX: &str = "\x1b]52;";

// === Character Set Designation (ISO 2022) ===
/// Select UTF-8 character set
pub const CHARSET_UTF8: &str = "\x1b%G";
/// Select default (ISO 8859-1) character set
pub const CHARSET_DEFAULT: &str = "\x1b%@";

// === Helper Functions ===

#[inline]
pub fn cursor_up(n: u16) -> String {
    format!("{CSI}{n}A")
}

#[inline]
pub fn cursor_down(n: u16) -> String {
    format!("{CSI}{n}B")
}

#[inline]
pub fn cursor_right(n: u16) -> String {
    format!("{CSI}{n}C")
}

#[inline]
pub fn cursor_left(n: u16) -> String {
    format!("{CSI}{n}D")
}

#[inline]
pub fn cursor_to(row: u16, col: u16) -> String {
    format!("{CSI}{row};{col}H")
}

/// Build a portable in-place redraw prefix (`CR` + `EL2`).
///
/// This is the common CLI pattern for one-line progress updates.
pub const REDRAW_LINE_PREFIX: &str = "\r\x1b[2K";

#[inline]
pub fn redraw_line_prefix() -> &'static str {
    REDRAW_LINE_PREFIX
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
    format!("{CSI}38;5;{color_id}m")
}

#[inline]
pub fn bg_256(color_id: u8) -> String {
    format!("{CSI}48;5;{color_id}m")
}

#[inline]
pub fn fg_rgb(r: u8, g: u8, b: u8) -> String {
    format!("{CSI}38;2;{r};{g};{b}m")
}

#[inline]
pub fn bg_rgb(r: u8, g: u8, b: u8) -> String {
    format!("{CSI}48;2;{r};{g};{b}m")
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
    text.contains(ESC_CHAR)
}

#[inline]
pub fn starts_with_ansi(text: &str) -> bool {
    text.starts_with(ESC_CHAR)
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
pub fn write_styled<W: Write>(writer: &mut W, text: &str, style: &str) -> std::io::Result<()> {
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

/// Set scrolling region (DECSTBM) — top and bottom rows (1-indexed)
#[inline]
pub fn set_scroll_region(top: u16, bottom: u16) -> String {
    format!("{CSI}{top};{bottom}r")
}

/// Insert Ps lines at cursor position
#[inline]
pub fn insert_lines(n: u16) -> String {
    format!("{CSI}{n}L")
}

/// Delete Ps lines at cursor position
#[inline]
pub fn delete_lines(n: u16) -> String {
    format!("{CSI}{n}M")
}

/// Scroll up Ps lines
#[inline]
pub fn scroll_up(n: u16) -> String {
    format!("{CSI}{n}S")
}

/// Scroll down Ps lines
#[inline]
pub fn scroll_down(n: u16) -> String {
    format!("{CSI}{n}T")
}

/// Build an OSC sequence to set the terminal window title
#[inline]
pub fn set_window_title(title: &str) -> String {
    format!("{OSC_SET_TITLE_PREFIX}{title}{BEL}")
}

/// Build an OSC 8 hyperlink open sequence
#[inline]
pub fn hyperlink_open(url: &str) -> String {
    format!("{OSC_HYPERLINK_PREFIX};{url}{ST}")
}

/// Build an OSC 8 hyperlink close sequence
#[inline]
pub fn hyperlink_close() -> String {
    format!("{OSC_HYPERLINK_PREFIX};{ST}")
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
