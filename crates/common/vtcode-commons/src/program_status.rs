//! Bounded Program Status Protocol (OSC 7501) encoding. No terminal I/O.

use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};

/// Terminal record states, independent of execution authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramState {
    Idle,
    Working,
    Blocked,
    Done,
    Error,
    Clear,
}

impl ProgramState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Error => "error",
            Self::Clear => "clear",
        }
    }

    pub const fn is_finished(self) -> bool {
        matches!(self, Self::Done | Self::Error)
    }
}

/// The actual user intervention required by an interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionKind {
    Permission,
    Question,
    Auth,
}

impl InteractionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Auth => "auth",
        }
    }
}

/// Projection commands carry no input or execution authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramStatusUpdate {
    Configure {
        enabled: bool,
    },
    Wait {
        token: u64,
        kind: InteractionKind,
    },
    Resume {
        token: u64,
    },
    Outcome(ProgramState),
    /// Determinate task progress (0-100). `None` means indeterminate and
    /// omits `progress`, per OSC 7501. Only emitted with `working`/`blocked`.
    Progress {
        percent: Option<u8>,
    },
}

/// Generate an opaque ASCII segment without copying source identity onto the wire.
pub fn record_segment(kind: &str, source: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(kind.as_bytes());
    hash.update([0]);
    hash.update(source.as_bytes());
    let digest = hash.finalize();
    let mut segment = String::with_capacity(31);
    segment.push('t');
    for byte in digest.iter().take(15) {
        for nibble in [byte >> 4, byte & 0x0f] {
            segment.push(char::from(if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 }));
        }
    }
    segment
}

fn valid_segment(segment: &str) -> bool {
    (1..=32).contains(&segment.len())
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.+-".contains(&byte))
}

fn valid_id(id: &str) -> bool {
    id.len() <= 128 && id.split('/').count() <= 8 && id.split('/').all(valid_segment)
}

fn encode_text(text: &str, limit: usize) -> Result<String> {
    ensure!(text.len() <= limit, "program status text exceeds byte limit");
    ensure!(!text.chars().any(char::is_control), "program status text contains controls");
    // Disarm bidi overrides/isolates and invisible directional marks.
    let text: String = text
        .chars()
        .filter(
            |c| !matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'),
        )
        .collect();
    Ok(STANDARD.encode(text))
}

/// Minimum `VTE_VERSION` whose parser safely ignores unknown OSC sequences.
///
/// Older VTE-based terminals may mishandle unrecognized OSC bytes, so
/// default-on emission skips them. Gate adapted from Codex
/// (`codex-rs/tui/src/terminal_program_status.rs`, openai/codex#52725,
/// Apache-2.0): absent or unparsable versions are treated as safe.
pub const MIN_VTE_VERSION_WITH_SAFE_OSC: u32 = 5400;

/// Pure gate: whether a `VTE_VERSION` value permits unknown-OSC emission.
pub fn vte_version_allows_unknown_osc(raw: Option<&str>) -> bool {
    let Some(raw) = raw else {
        return true;
    };
    match raw.trim().parse::<u32>() {
        Ok(version) => version >= MIN_VTE_VERSION_WITH_SAFE_OSC,
        Err(_) => true,
    }
}

/// Live gate combining [`vte_version_allows_unknown_osc`] with the process environment.
pub fn vte_allows_unknown_osc() -> bool {
    vte_version_allows_unknown_osc(std::env::var("VTE_VERSION").ok().as_deref())
}

/// Maximum `detail` characters in an iTerm2 session-status report.
const MAX_ITERM21337_DETAIL_CHARS: usize = 200;

/// iTerm2 session-status (OSC 21337) label and indicator for a terminal state.
///
/// Finished states read as idle, as does `Clear` (defensive: callers clear via
/// the empty report instead); only the label/color pair travels. Mapping
/// adapted from Codex (`codex-rs/tui/src/terminal_program_status.rs`,
/// openai/codex#52725, Apache-2.0).
pub fn iterm21337_status(state: ProgramState) -> (&'static str, &'static str) {
    match state {
        ProgramState::Idle | ProgramState::Done | ProgramState::Error | ProgramState::Clear => ("Idle", "#00d75f"),
        ProgramState::Working => ("Working", "#ff9500"),
        ProgramState::Blocked => ("Waiting", "#5f87ff"),
    }
}

/// Encode an iTerm2 session-status report for a direct iTerm2 session.
///
/// `detail` is sanitized: controls are stripped, output is bounded, and `;`
/// / `\` are backslash-escaped so the payload cannot reshape the sequence.
pub fn encode_iterm21337(state: ProgramState, detail: &str) -> String {
    let (label, indicator) = iterm21337_status(state);
    format!(
        "\x1b]21337;status={label};indicator={indicator};status-color=;detail={}\x07",
        sanitize_iterm21337_detail(detail)
    )
}

/// Encode the empty iTerm2 session-status report that clears a previous one.
pub fn clear_iterm21337() -> String {
    "\x1b]21337;status=;indicator=;status-color=;detail=\x07".to_string()
}

fn sanitize_iterm21337_detail(detail: &str) -> String {
    let mut clean = String::with_capacity(detail.len());
    for ch in detail.chars().filter(|ch| !ch.is_control()).take(MAX_ITERM21337_DETAIL_CHARS) {
        if matches!(ch, ';' | '\\') {
            clean.push('\\');
        }
        clean.push(ch);
    }
    clean
}

/// Pure gate: 21337 is iTerm2-proprietary, so only direct iTerm2 sessions
/// (no multiplexer or GNU screen between the program and the emulator).
pub fn should_emit_iterm21337(iterm_session: bool, tmux_session: bool, screen_session: bool) -> bool {
    iterm_session && !tmux_session && !screen_session
}

/// Live gate combining [`should_emit_iterm21337`] with `ITERM_SESSION_ID` /
/// `TMUX` / `STY` detection.
pub fn should_emit_iterm21337_now() -> bool {
    should_emit_iterm21337(
        std::env::var("ITERM_SESSION_ID").is_ok(),
        std::env::var("TMUX").is_ok(),
        std::env::var("STY").is_ok(),
    )
}

/// ConEmu `OSC 9;4` tab-progress state, also rendered natively by Windows
/// Terminal and Ghostty 1.2+. Numeric mapping follows ConEmu and the
/// [Windows Terminal docs](https://learn.microsoft.com/en-us/windows/terminal/tutorials/progress-bar-sequences).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConEmuProgress {
    /// `0`: hide the progress indicator (also the completion state).
    Remove,
    /// `1`: determinate progress, clamped to `0..=100`.
    Normal(u8),
    /// `2`: error state.
    Error,
    /// `3`: indeterminate work (no percentage available).
    Indeterminate,
    /// `4`: paused/warning state (a wait, not a failure).
    Paused,
}

/// Encode a ConEmu `OSC 9;4` tab-progress report (`BEL`-terminated, the
/// canonical form from the Windows Terminal docs).
pub fn encode_conemu94(progress: ConEmuProgress) -> String {
    match progress {
        ConEmuProgress::Remove => "\x1b]9;4;0\x07".to_string(),
        ConEmuProgress::Normal(percent) => format!("\x1b]9;4;1;{}\x07", percent.min(100)),
        ConEmuProgress::Error => "\x1b]9;4;2\x07".to_string(),
        ConEmuProgress::Indeterminate => "\x1b]9;4;3\x07".to_string(),
        ConEmuProgress::Paused => "\x1b]9;4;4\x07".to_string(),
    }
}

/// Minimum Ghostty `(major, minor)` rendering `OSC 9;4` as tab progress.
///
/// Older Ghostty parses `OSC 9` as a notification, so unversioned or
/// unparsable versions fail closed (no emission).
pub const MIN_GHOSTTY_VERSION_WITH_CONEMU94: (u32, u32) = (1, 2);

/// Pure gate: whether a `TERM_PROGRAM_VERSION` value permits `OSC 9;4`.
/// Compares only the leading `major.minor` pair; anything else fails closed.
pub fn ghostty_version_allows_conemu94(raw: Option<&str>) -> bool {
    let Some(raw) = raw else {
        return false;
    };
    let mut parts = raw.trim().split('.');
    let (Some(major), Some(minor)) = (parts.next(), parts.next()) else {
        return false;
    };
    match (major.parse::<u32>(), minor.parse::<u32>()) {
        (Ok(major), Ok(minor)) => (major, minor) >= MIN_GHOSTTY_VERSION_WITH_CONEMU94,
        _ => false,
    }
}

/// Pure gate: `OSC 9;4` renders only on terminals documented to honor it.
///
/// Everywhere else `OSC 9` may mean notifications (iTerm2, older Ghostty),
/// so this is default-deny: unknown terminals never receive the sequence.
pub fn should_emit_conemu94(windows_terminal: bool, conemu: bool, ghostty_supported: bool) -> bool {
    windows_terminal || conemu || ghostty_supported
}

/// Live gate combining [`should_emit_conemu94`] with process-environment
/// detection: `WT_SESSION`/`WT_PROFILE_ID` (Windows Terminal), `ConEmuPID`
/// (ConEmu), or exactly `TERM_PROGRAM=ghostty` with
/// [`ghostty_version_allows_conemu94`].
pub fn should_emit_conemu94_now() -> bool {
    let windows_terminal = std::env::var("WT_SESSION").is_ok() || std::env::var("WT_PROFILE_ID").is_ok();
    let conemu = std::env::var("ConEmuPID").is_ok();
    let ghostty_supported = std::env::var("TERM_PROGRAM")
        .map(|program| program.eq_ignore_ascii_case("ghostty"))
        .unwrap_or(false)
        && ghostty_version_allows_conemu94(std::env::var("TERM_PROGRAM_VERSION").ok().as_deref());
    should_emit_conemu94(windows_terminal, conemu, ghostty_supported)
}

/// Encode a complete replacement record. An explicit owned ID is mandatory;
/// callers cannot accidentally clear the terminal's root or unrelated records.
///
/// `progress` follows OSC 7501: only emitted with `working`/`blocked` when
/// `Some(0..=100)`. Any other state, `None`, or out-of-range value omits the
/// key (absent means indeterminate).
pub fn encode_report(
    id: &str,
    state: ProgramState,
    kind: Option<InteractionKind>,
    progress: Option<u8>,
    title: &str,
    message: &str,
) -> Result<String> {
    ensure!(valid_id(id), "invalid program status id");
    let title = encode_text(title, 192)?;
    let message = encode_text(message, 2048)?;
    let mut report = format!("\x1b]7501;state={}:id={id}:app=vtcode", state.as_str());
    if state == ProgramState::Blocked
        && let Some(kind) = kind
    {
        report.push_str(":kind=");
        report.push_str(kind.as_str());
    }
    if matches!(state, ProgramState::Working | ProgramState::Blocked)
        && let Some(percent) = progress
        && percent <= 100
    {
        report.push_str(":progress=");
        report.push_str(&percent.to_string());
    }
    report.push_str(":title=");
    report.push_str(&title);
    report.push_str(":msg=");
    report.push_str(&message);
    report.push_str("\x1b\\");
    ensure!(report.len() <= 4096, "program status report exceeds byte limit");
    Ok(report)
}

/// Encode a record removal. `clear` carries no content keys: the terminal
/// drops the record and its subtree without reading anything else.
pub fn encode_clear(id: &str) -> Result<String> {
    ensure!(valid_id(id), "invalid program status id");
    let report = format!("\x1b]7501;state=clear:id={id}:app=vtcode\x1b\\");
    ensure!(report.len() <= 4096, "program status report exceeds byte limit");
    Ok(report)
}

#[cfg(test)]
mod tests;
