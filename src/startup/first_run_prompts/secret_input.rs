//! Echo-off secret line input for the first-run wizard.
//!
//! The wizard's API-key paste flow reads the key with terminal echo disabled
//! so a pasted secret does not appear in scrollback or terminal recordings.
//! Characters are echoed as `*`; Backspace deletes the last char, Ctrl-U
//! clears the line, Enter submits, Ctrl-D submits (or skips when empty), and
//! Ctrl-C aborts. Raw mode is restored unconditionally via an RAII guard, so
//! the terminal is never left in a broken state — even on panic or early
//! return.
//!
//! When stdin is not a TTY (piped input, CI, `--skip-confirmations`), raw
//! mode is unavailable, so we fall back to a plain echoed `read_line` and
//! warn once. This fallback is the only path automated tests can exercise.

use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use super::common::SetupInterrupted;

/// Read one line from stdin without echoing the raw characters.
///
/// Returns `Ok(None)` when the user submits an empty line (Enter or Ctrl-D
/// with no input) — callers treat this as "skip". Returns `Ok(Some(key))` on
/// a non-empty submit. Returns `Err(SetupInterrupted)` on Ctrl-C.
pub(super) fn read_secret_line(prompt: &str) -> Result<Option<String>> {
    if io::stdin().is_terminal() {
        read_secret_line_tty(prompt)
    } else {
        read_secret_line_fallback(prompt)
    }
}

/// Render a masked preview of a secret so the user can confirm they pasted
/// the right key without the full value being shown. Shows the first 4 and
/// last 4 characters with an ellipsis in between. Short keys (≤ 12 chars)
/// are fully masked so we don't leak most of the key.
pub(super) fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let len = chars.len();
    if len <= 12 {
        return "••••••••".to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[len - 4..].iter().collect();
    format!("{head}…{tail}")
}

fn read_secret_line_fallback(prompt: &str) -> Result<Option<String>> {
    vtcode_commons::diagnostics::stderr(format_args!(
        "Warning: stdin is not a terminal — the pasted key will be visible."
    ));
    print!("{prompt}");
    io::stdout().flush().context("Failed to flush secret prompt")?;
    let mut input = String::new();
    io::stdin().read_line(&mut input).context("Failed to read API key input")?;
    let trimmed = input.trim().to_string();
    Ok(if trimmed.is_empty() { None } else { Some(trimmed) })
}

fn read_secret_line_tty(prompt: &str) -> Result<Option<String>> {
    // RAII guard: Drop restores canonical mode + drains buffered events no
    // matter how we leave this function (return, `?`, panic). Never panic
    // from Drop so a restore failure cannot compound an in-flight error.
    let _guard = RawModeGuard::enable()?;

    {
        let mut stdout = io::stdout();
        write!(stdout, "{prompt}").ok();
        stdout.flush().ok();
    }

    let mut buffer = String::new();
    loop {
        let event = event::read().with_context(|| "Failed to read keypress while entering API key")?;
        match handle_key(event, &mut buffer)? {
            KeyAction::Continue => continue,
            KeyAction::Submit => {
                let mut stdout = io::stdout();
                write!(stdout, "\r\n").ok();
                stdout.flush().ok();
                let trimmed = buffer.trim().to_string();
                return Ok(if trimmed.is_empty() { None } else { Some(trimmed) });
            }
            KeyAction::Abort => return Err(SetupInterrupted.into()),
        }
    }
}

enum KeyAction {
    Continue,
    Submit,
    Abort,
}

fn handle_key(event: Event, buffer: &mut String) -> Result<KeyAction> {
    let Event::Key(key) = event else {
        return Ok(KeyAction::Continue);
    };
    if key.kind != KeyEventKind::Press {
        return Ok(KeyAction::Continue);
    }
    let mut stdout = io::stdout();
    match key.code {
        KeyCode::Enter => Ok(KeyAction::Submit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok(KeyAction::Abort),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => Ok(KeyAction::Submit),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Kill line: erase every buffered char from the screen, then clear.
            for _ in 0..buffer.chars().count() {
                write!(stdout, "\u{8} \u{8}").ok();
            }
            stdout.flush().ok();
            buffer.clear();
            Ok(KeyAction::Continue)
        }
        KeyCode::Backspace => {
            if !buffer.is_empty() {
                buffer.pop();
                write!(stdout, "\u{8} \u{8}").ok();
                stdout.flush().ok();
            }
            Ok(KeyAction::Continue)
        }
        KeyCode::Char(c) if !c.is_control() => {
            buffer.push(c);
            write!(stdout, "*").ok();
            stdout.flush().ok();
            Ok(KeyAction::Continue)
        }
        _ => Ok(KeyAction::Continue),
    }
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> Result<Self> {
        enable_raw_mode().context("Failed to enable raw mode for secret input")?;
        Ok(RawModeGuard)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        // Drain keystrokes buffered during input so they don't leak into the
        // next prompt, then restore canonical mode. Best-effort only — never
        // panic from Drop.
        while let Ok(true) = event::poll(std::time::Duration::from_millis(0)) {
            let _ = event::read();
        }
        let _ = disable_raw_mode();
        let _ = io::stdout().flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_key_short_keys_fully_masked() {
        assert_eq!(mask_key("short"), "••••••••");
        assert_eq!(mask_key("exactly12c"), "••••••••"); // 10 chars
        assert_eq!(mask_key("exactly12ch"), "••••••••"); // 11 chars
    }

    #[test]
    fn mask_key_boundary_12_chars_fully_masked() {
        // 12 chars is the boundary — still fully masked so we never leak 8 of
        // 12 characters (which would be most of the key).
        assert_eq!(mask_key("abcdefghijkl"), "••••••••"); // 12 chars
    }

    #[test]
    fn mask_key_long_keys_show_head_and_tail() {
        let masked = mask_key("sk-or-v1-abcdefgh1234567890");
        assert!(masked.starts_with("sk-o"), "head should be first 4 chars: {masked}");
        assert!(masked.ends_with("7890"), "tail should be last 4 chars: {masked}");
        assert!(masked.contains('…'), "middle should be ellipsis: {masked}");
    }

    #[test]
    fn mask_key_never_leaks_middle() {
        let key = "ABCDEFGHIJKLMNOP"; // 16 chars
        let masked = mask_key(key);
        assert!(!masked.contains("EFGHIJ"), "middle of the key must not appear: {masked}");
        assert!(!masked.contains("JKLM"), "middle of the key must not appear: {masked}");
    }

    #[test]
    fn mask_key_handles_unicode_without_panicking() {
        // Multi-byte chars must not panic on the index math.
        let masked = mask_key("🔑🔑🔑🔑🔑🔑🔑🔑🔑🔑🔑🔑🔑"); // 13 chars
        assert!(masked.contains('…'));
    }
}
