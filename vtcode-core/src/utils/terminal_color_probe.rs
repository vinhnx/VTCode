//! Startup OSC probe for terminal color-scheme and 256-color harmony.
//!
//! This follows the same interactive detection path as the reference script:
//! - Query OSC 10/11 for foreground/background
//! - Query OSC 4;16 and OSC 4;231 for palette endpoints
//! - Send DA1 (`ESC [ c`) as a flush sentinel
//! - Read until a DA1 response begins (`ESC [ ?`)
//! - Infer terminal light/dark scheme and palette harmony

#[cfg(unix)]
use std::fs::{File, OpenOptions};
#[cfg(unix)]
use std::io::{Read, Write};
#[cfg(unix)]
use std::time::{Duration, Instant};

#[cfg(unix)]
use anyhow::{Context, Result, anyhow};
#[cfg(unix)]
use crossterm::tty::IsTty;
#[cfg(unix)]
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
#[cfg(unix)]
use nix::sys::termios::{self, SetArg};
#[cfg(unix)]
use std::os::fd::AsFd;
#[cfg(unix)]
use vtcode_commons::ansi_capabilities::{ColorScheme, set_color_scheme_override};
#[cfg(unix)]
use vtcode_commons::ansi_codes::{DEVICE_ATTRIBUTES_REQUEST, ESC_BYTE, ESC_CHAR, OSC, ST};
#[cfg(unix)]
use vtcode_commons::color256_theme::set_harmonious_runtime_hint;

#[cfg(unix)]
const DA1_RESPONSE_PREFIX: [u8; 3] = [ESC_BYTE, b'[', b'?'];

/// Run OSC probe once at startup and cache results in shared runtime hints.
pub fn probe_and_cache_terminal_palette_harmony() {
    #[cfg(unix)]
    {
        if !std::io::stdin().is_tty() || !std::io::stdout().is_tty() {
            return;
        }

        let timeout = Duration::from_millis(200);
        match probe_terminal_colors(timeout) {
            Ok(result) => {
                let scheme = if result.is_term_light_theme {
                    ColorScheme::Light
                } else {
                    ColorScheme::Dark
                };
                set_color_scheme_override(Some(scheme));
                set_harmonious_runtime_hint(Some(result.is_harmonious));
                tracing::trace!(
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
    tty.write_all(DEVICE_ATTRIBUTES_REQUEST.as_bytes())
        .context("failed to write DA1 sentinel query")?;
    tty.flush().context("failed to flush OSC probe queries")?;

    let response = read_until_da1(&mut tty, timeout)?;
    let [fg, bg, c16, c231] = parse_four_colors(&response)?;

    let is_term_light_theme = lightness(bg) > lightness(fg);
    let is_palette_light_theme = lightness(c16) > lightness(c231);

    Ok(ProbeResult {
        is_term_light_theme,
        is_palette_light_theme,
        is_harmonious: is_term_light_theme == is_palette_light_theme,
        is_generated: bg == c16 && fg == c231,
    })
}

#[cfg(unix)]
fn read_until_da1(tty: &mut File, timeout: Duration) -> Result<Vec<u8>> {
    let poll_tty = tty
        .try_clone()
        .context("failed to duplicate tty for polling")?;
    let deadline = Instant::now() + timeout;
    let mut buffer = Vec::with_capacity(1024);

    loop {
        if buffer
            .windows(DA1_RESPONSE_PREFIX.len())
            .any(|window| window == DA1_RESPONSE_PREFIX)
        {
            return Ok(buffer);
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(anyhow!("timed out waiting for DA1 sentinel response"));
        }

        let remaining_ms = (deadline - now).as_millis().min(i32::MAX as u128) as i32;
        let timeout = PollTimeout::try_from(remaining_ms).unwrap_or(PollTimeout::MAX);
        let mut pollfd = [PollFd::new(poll_tty.as_fd(), PollFlags::POLLIN)];
        let poll_result = poll(&mut pollfd, timeout).context("poll failed during OSC probe")?;
        let ready = pollfd[0]
            .revents()
            .unwrap_or(PollFlags::empty())
            .contains(PollFlags::POLLIN);
        if poll_result == 0 || !ready {
            continue;
        }

        let mut chunk = [0_u8; 4096];
        let bytes_read = tty
            .read(&mut chunk)
            .context("failed to read OSC probe response")?;
        if bytes_read == 0 {
            return Err(anyhow!(
                "terminal closed while waiting for OSC probe response"
            ));
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
    }
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
        return Err(anyhow!(
            "expected 4 colors from OSC probe, got {}",
            parsed.len()
        ));
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
}
