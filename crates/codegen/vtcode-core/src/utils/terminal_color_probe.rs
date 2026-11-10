//! Startup OSC probe for terminal color-scheme and 256-color harmony.
//!
//! This follows the same interactive detection path as the reference script:
//! - Query OSC 10/11 for foreground/background
//! - Query OSC 4;16 and OSC 4;231 for palette endpoints
//! - Query the Contour color-scheme mode (`CSI ? 996 n`) for an explicit
//!   dark/light answer (`CSI ? 997 ; Ps n`) from terminals that support it
//! - Query rich-clipboard support (`CSI ? 5522 $ p`, DECRQM) so the TUI can
//!   fall back to Kitty OSC 5522 reads for image paste over SSH/headless
//! - Send DA1 (`ESC [ c`) as a flush sentinel
//! - Read until a DA1 response begins (`ESC [ ?`)
//! - Infer terminal light/dark scheme and palette harmony; the explicit
//!   Contour report wins over luminance inference when available

#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use anyhow::{Context, Result, anyhow};
#[cfg(unix)]
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
#[cfg(unix)]
use nix::sys::termios::{self, SetArg};
#[cfg(unix)]
use std::io::IsTerminal;
#[cfg(unix)]
use std::os::fd::AsFd;
#[cfg(unix)]
use vtcode_commons::ansi_capabilities::{ColorScheme, set_color_scheme_override};
#[cfg(unix)]
use vtcode_commons::ansi_codes::{
    COLOR_SCHEME_MODE_REQUEST, COLOR_SCHEME_REPORT_PREFIX, DEVICE_ATTRIBUTES_REQUEST, ESC_BYTE, ESC_CHAR, OSC, ST,
};
#[cfg(unix)]
use vtcode_commons::clipboard_5522::{DECRQM_5522_QUERY, parse_decrpm_5522, set_clipboard_5522_hint};
#[cfg(unix)]
use vtcode_commons::color256_theme::set_harmonious_runtime_hint;

#[cfg(unix)]
const DA1_RESPONSE_PREFIX: [u8; 3] = [ESC_BYTE, b'[', b'?'];
#[cfg(unix)]
const RESPONSE_SETTLE_WINDOW: Duration = Duration::from_millis(40);

/// Run OSC probe once at startup and cache results in shared runtime hints.
pub fn probe_and_cache_terminal_palette_harmony() {
    #[cfg(unix)]
    {
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return;
        }

        let timeout = Duration::from_millis(50);
        match probe_terminal_colors(timeout) {
            Ok(result) => {
                // The Contour color-scheme report is an explicit answer from the
                // terminal; fall back to foreground/background luminance inference
                // for terminals that ignore the `CSI ? 996 n` query.
                let scheme = result.scheme_report.unwrap_or(if result.is_term_light_theme {
                    ColorScheme::Light
                } else {
                    ColorScheme::Dark
                });
                set_color_scheme_override(Some(scheme));
                set_harmonious_runtime_hint(Some(result.is_harmonious));
                set_clipboard_5522_hint(result.clipboard_5522);
                tracing::trace!(
                    scheme = scheme.name(),
                    explicit_scheme = result.scheme_report.is_some(),
                    clipboard_5522 = result.clipboard_5522,
                    term_light = result.is_term_light_theme,
                    palette_light = result.is_palette_light_theme,
                    harmonious = result.is_harmonious,
                    generated = result.is_generated,
                    "terminal OSC color probe completed"
                );
            }
            Err(err) => {
                tracing::trace!(error = %err, "terminal OSC color probe skipped");
            }
        }
    }
}

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProbeResult {
    is_term_light_theme: bool,
    is_palette_light_theme: bool,
    is_harmonious: bool,
    is_generated: bool,
    /// Explicit dark/light answer from a `CSI ? 997 ; Ps n` reply, when the
    /// terminal supports the Contour color-scheme query.
    scheme_report: Option<ColorScheme>,
    /// Rich-clipboard verdict from a `CSI ? 5522 ; Ps $ y` reply; `None`
    /// when the terminal sent no 5522 report (reads as unsupported).
    clipboard_5522: Option<bool>,
}

#[cfg(unix)]
fn probe_terminal_colors(timeout: Duration) -> Result<ProbeResult> {
    let mut tty = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .context("failed to open /dev/tty for OSC probe")?;

    let _raw_guard = RawModeGuard::activate(&tty)?;

    for code in ["10", "11", "4;16", "4;231"] {
        write!(tty, "{OSC}{code};?{ST}").context("failed to write OSC query")?;
    }
    tty.write_all(COLOR_SCHEME_MODE_REQUEST.as_bytes())
        .context("failed to write Contour color-scheme mode query")?;
    tty.write_all(DECRQM_5522_QUERY.as_bytes())
        .context("failed to write clipboard 5522 support query")?;
    tty.write_all(DEVICE_ATTRIBUTES_REQUEST.as_bytes())
        .context("failed to write DA1 sentinel query")?;
    tty.flush().context("failed to flush OSC probe queries")?;

    let result = read_until_da1(&mut tty, timeout);
    if result.is_err() {
        // Drain any pending responses so they don't leak as visible escape codes
        // after the process exits and the terminal restores cooked mode.
        drain_tty_input(&mut tty);
    }
    let response = result?;
    let parsed = match parse_four_colors(&response) {
        Ok(parsed) => parsed,
        Err(error) => {
            drain_tty_input(&mut tty);
            return Err(error);
        }
    };
    let [fg, bg, c16, c231] = parsed;

    let is_term_light_theme = lightness(bg) > lightness(fg);
    let is_palette_light_theme = lightness(c16) > lightness(c231);

    Ok(ProbeResult {
        is_term_light_theme,
        is_palette_light_theme,
        is_harmonious: is_term_light_theme == is_palette_light_theme,
        is_generated: bg == c16 && fg == c231,
        scheme_report: parse_scheme_report(&response),
        clipboard_5522: parse_decrpm_5522(&response),
    })
}

/// Parse the Contour color-scheme report (`CSI ? 997 ; Ps n`) out of the probe
/// response. `Ps` 1 means dark mode and 2 means light mode; anything else is
/// ignored. The last valid report wins if the terminal emits several.
#[cfg(unix)]
fn parse_scheme_report(response: &[u8]) -> Option<ColorScheme> {
    let decoded = String::from_utf8_lossy(response);
    let mut scheme = None;
    for (pos, _) in decoded.match_indices(COLOR_SCHEME_REPORT_PREFIX) {
        // `get` keeps the strict workspace slicing/indexing lints happy.
        let Some(after) = decoded.get(pos + COLOR_SCHEME_REPORT_PREFIX.len()..) else {
            continue;
        };
        // The payload must be exactly `Ps` followed by the `n` final byte;
        // `split_once` fails when the terminator is missing entirely.
        let Some((payload, _)) = after.split_once('n') else {
            continue;
        };
        match payload {
            "1" => scheme = Some(ColorScheme::Dark),
            "2" => scheme = Some(ColorScheme::Light),
            _ => {}
        }
    }
    scheme
}

/// Best-effort drain of pending TTY input to prevent escape code leakage.
#[cfg(unix)]
fn drain_tty_input(tty: &mut File) {
    let drain_tty = match tty.try_clone() {
        Ok(f) => f,
        Err(_) => return,
    };
    let mut pollfd = [PollFd::new(drain_tty.as_fd(), PollFlags::POLLIN)];
    let drain_deadline = Instant::now() + Duration::from_millis(100);
    let mut discard = [0_u8; 4096];
    loop {
        let now = Instant::now();
        if now >= drain_deadline {
            break;
        }
        let remaining_ms = (drain_deadline - now).as_millis().min(i32::MAX as u128) as i32;
        let timeout = match PollTimeout::try_from(remaining_ms) {
            Ok(t) => t,
            Err(_) => break,
        };
        if poll(&mut pollfd, timeout).unwrap_or(0) == 0 {
            break;
        }
        let ready = pollfd[0].revents().unwrap_or(PollFlags::empty()).contains(PollFlags::POLLIN);
        if !ready {
            break;
        }
        if tty.read(&mut discard).unwrap_or(0) == 0 {
            break;
        }
    }
}

#[cfg(unix)]
fn read_until_da1(tty: &mut File, timeout: Duration) -> Result<Vec<u8>> {
    let poll_tty = tty.try_clone().context("failed to duplicate tty for polling")?;
    let overall_deadline = Instant::now() + timeout;
    let mut buffer = Vec::with_capacity(1024);
    let mut settle_deadline = None;

    loop {
        if has_complete_probe_response(&buffer) && settle_deadline.is_none() {
            settle_deadline = Some(Instant::now() + RESPONSE_SETTLE_WINDOW);
        }

        let now = Instant::now();
        let active_deadline = settle_deadline
            .map(|deadline| deadline.min(overall_deadline))
            .unwrap_or(overall_deadline);
        if now >= active_deadline {
            if settle_deadline.is_some() {
                return Ok(buffer);
            }
            return Err(anyhow!("timed out waiting for DA1 sentinel response"));
        }

        let remaining_ms = (active_deadline - now).as_millis().min(i32::MAX as u128) as i32;
        let timeout = PollTimeout::try_from(remaining_ms).unwrap_or(PollTimeout::MAX);
        let mut pollfd = [PollFd::new(poll_tty.as_fd(), PollFlags::POLLIN)];
        let poll_result = poll(&mut pollfd, timeout).context("poll failed during OSC probe")?;
        let ready = pollfd[0].revents().unwrap_or(PollFlags::empty()).contains(PollFlags::POLLIN);
        if poll_result == 0 || !ready {
            continue;
        }

        let mut chunk = [0_u8; 4096];
        let bytes_read = tty.read(&mut chunk).context("failed to read OSC probe response")?;
        if bytes_read == 0 {
            return Err(anyhow!("terminal closed while waiting for OSC probe response"));
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
        if settle_deadline.is_some() {
            settle_deadline = Some(Instant::now() + RESPONSE_SETTLE_WINDOW);
        }
    }
}

#[cfg(unix)]
fn has_complete_da1_response(buffer: &[u8]) -> bool {
    buffer
        .windows(DA1_RESPONSE_PREFIX.len())
        .position(|window| window == DA1_RESPONSE_PREFIX)
        .is_some_and(|pos| buffer[pos..].contains(&b'c'))
}

#[cfg(unix)]
fn has_complete_probe_response(buffer: &[u8]) -> bool {
    has_complete_da1_response(buffer) && parse_four_colors(buffer).is_ok()
}

#[cfg(unix)]
fn parse_four_colors(response: &[u8]) -> Result<[(u8, u8, u8); 4]> {
    let mut parsed = Vec::with_capacity(4);
    let decoded = String::from_utf8_lossy(response);

    for part in decoded.split("rgb:").skip(1) {
        if let Some(rgb) = parse_rgb_part(part) {
            parsed.push(rgb);
            if parsed.len() == 4 {
                break;
            }
        }
    }

    if parsed.len() != 4 {
        return Err(anyhow!("expected 4 colors from OSC probe, got {}", parsed.len()));
    }

    Ok([parsed[0], parsed[1], parsed[2], parsed[3]])
}

#[cfg(unix)]
fn parse_rgb_part(part: &str) -> Option<(u8, u8, u8)> {
    let payload = part.split(ESC_CHAR).next()?;
    let mut channels = payload.split('/');

    Some((
        normalize_channel(channels.next()?)?,
        normalize_channel(channels.next()?)?,
        normalize_channel(channels.next()?)?,
    ))
}

#[cfg(unix)]
fn normalize_channel(channel: &str) -> Option<u8> {
    let trimmed = channel.trim();
    if trimmed.is_empty() {
        return None;
    }

    let hex = if trimmed.len() == 1 {
        let mut repeated = String::with_capacity(2);
        repeated.push(trimmed.as_bytes()[0] as char);
        repeated.push(trimmed.as_bytes()[0] as char);
        repeated
    } else {
        trimmed[..2].to_string()
    };

    u8::from_str_radix(&hex, 16).ok()
}

#[cfg(unix)]
fn lightness((r, g, b): (u8, u8, u8)) -> f64 {
    0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)
}

/// Set when crossterm takes over the TTY so a late probe restore is a no-op.
#[cfg(unix)]
static CROSSTERM_OWNS_RAW_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Mark the TTY as owned by crossterm raw mode (probe restore becomes a no-op).
#[cfg(unix)]
pub fn note_crossterm_raw_mode() {
    CROSSTERM_OWNS_RAW_MODE.store(true, std::sync::atomic::Ordering::Release);
}

#[cfg(unix)]
struct RawModeGuard {
    tty: File,
    original: termios::Termios,
}

#[cfg(unix)]
impl RawModeGuard {
    fn activate(tty: &File) -> Result<Self> {
        let tty = tty.try_clone().context("failed to duplicate tty handle")?;
        let original = termios::tcgetattr(&tty).context("tcgetattr failed")?;
        let mut raw = original.clone();
        termios::cfmakeraw(&mut raw);
        termios::tcsetattr(&tty, SetArg::TCSANOW, &raw).context("tcsetattr raw mode failed")?;

        Ok(Self { tty, original })
    }
}

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        // If crossterm already enabled raw mode, restoring the pre-probe
        // termios would undo terminal setup. Skip restore in that case.
        if CROSSTERM_OWNS_RAW_MODE.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        let _ = termios::tcsetattr(&self.tty, SetArg::TCSANOW, &self.original);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use vtcode_commons::ansi_codes::CSI;

    #[test]
    fn parse_rgb_handles_16bit_channels() {
        let response = format!(
            "{OSC}10;rgb:dddd/dddd/dddd{ST}{OSC}11;rgb:1111/1111/1111{ST}{OSC}4;16;rgb:1111/1111/1111{ST}{OSC}4;231;rgb:dddd/dddd/dddd{ST}{CSI}?62;4c"
        );
        let [fg, bg, c16, c231] = parse_four_colors(response.as_bytes()).expect("valid colors");
        assert_eq!(fg, (0xdd, 0xdd, 0xdd));
        assert_eq!(bg, (0x11, 0x11, 0x11));
        assert_eq!(c16, (0x11, 0x11, 0x11));
        assert_eq!(c231, (0xdd, 0xdd, 0xdd));
    }

    #[test]
    fn parse_rgb_handles_single_digit_channels() {
        let response = format!(
            "{OSC}10;rgb:a/b/c{ST}{OSC}11;rgb:0/0/0{ST}{OSC}4;16;rgb:0/0/0{ST}{OSC}4;231;rgb:f/f/f{ST}{CSI}?1;2c"
        );
        let [fg, _, _, c231] = parse_four_colors(response.as_bytes()).expect("valid colors");
        assert_eq!(fg, (0xaa, 0xbb, 0xcc));
        assert_eq!(c231, (0xff, 0xff, 0xff));
    }

    #[test]
    fn detects_only_complete_da1_responses() {
        assert!(!has_complete_da1_response(b""));
        assert!(!has_complete_da1_response(b"\x1b[?62;22"));
        assert!(has_complete_da1_response(b"\x1b[?62;22;52c"));
        assert!(has_complete_da1_response(b"noise\x1b[?1;2cmore"));
    }

    #[test]
    fn waits_for_all_color_responses_before_settling() {
        let da1_only = b"\x1b[?62;22;52c";
        assert!(has_complete_da1_response(da1_only));
        assert!(!has_complete_probe_response(da1_only));

        let response = format!(
            "{OSC}10;rgb:dddd/dddd/dddd{ST}{OSC}11;rgb:1111/1111/1111{ST}{OSC}4;16;rgb:1111/1111/1111{ST}{OSC}4;231;rgb:dddd/dddd/dddd{ST}{CSI}?62;4c"
        );
        assert!(has_complete_probe_response(response.as_bytes()));
    }

    #[test]
    fn parses_contour_scheme_report_from_mixed_response() {
        let response = format!("{OSC}10;rgb:dddd/dddd/dddd{ST}{OSC}11;rgb:1111/1111/1111{ST}{CSI}?997;1n{CSI}?62;4c");
        assert_eq!(parse_scheme_report(response.as_bytes()), Some(ColorScheme::Dark));
        assert_eq!(parse_scheme_report(b"\x1b[?997;2n"), Some(ColorScheme::Light));
    }

    #[test]
    fn ignores_absent_and_malformed_scheme_reports() {
        assert_eq!(parse_scheme_report(b""), None);
        assert_eq!(parse_scheme_report(b"\x1b[?62;22;52c"), None);
        assert_eq!(parse_scheme_report(b"\x1b[?997;0n"), None);
        assert_eq!(parse_scheme_report(b"\x1b[?997;12n"), None);
        assert_eq!(parse_scheme_report(b"\x1b[?997;1"), None);
    }

    #[test]
    fn last_valid_scheme_report_wins() {
        let response = b"\x1b[?997;1nnoise\x1b[?997;2n";
        assert_eq!(parse_scheme_report(response), Some(ColorScheme::Light));
    }
}

#[cfg(not(unix))]
/// No-op on non-unix: the probe does not touch termios there.
pub fn note_crossterm_raw_mode() {}
