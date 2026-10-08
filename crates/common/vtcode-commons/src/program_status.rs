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
    Configure { enabled: bool },
    Wait { token: u64, kind: InteractionKind },
    Resume { token: u64 },
    Outcome(ProgramState),
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

/// Encode a complete replacement record. An explicit owned ID is mandatory;
/// callers cannot accidentally clear the terminal's root or unrelated records.
pub fn encode_report(
    id: &str,
    state: ProgramState,
    kind: Option<InteractionKind>,
    title: &str,
    message: &str,
) -> Result<String> {
    ensure!(
        id.len() <= 128 && id.split('/').count() <= 8 && id.split('/').all(valid_segment),
        "invalid program status id"
    );
    let title = encode_text(title, 192)?;
    let message = encode_text(message, 2048)?;
    let mut report = format!("\x1b]7501;state={}:id={id}:app=vtcode", state.as_str());
    if state == ProgramState::Blocked
        && let Some(kind) = kind
    {
        report.push_str(":kind=");
        report.push_str(kind.as_str());
    }
    report.push_str(":title=");
    report.push_str(&title);
    report.push_str(":msg=");
    report.push_str(&message);
    report.push_str("\x1b\\");
    ensure!(report.len() <= 4096, "program status report exceeds byte limit");
    Ok(report)
}

#[cfg(test)]
mod tests;
