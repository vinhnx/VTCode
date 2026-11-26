//! Program Resume Protocol (OSC 7577) encoding. No terminal I/O.
//!
//! A program that knows how to resume itself reports its resume command to the
//! terminal; the terminal may run it after a restart. Reports are a hint, never
//! a guarantee. See <https://www.superlogical.com/rex/docs/build/program-resume>.

use anyhow::{Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

/// Environment variable carrying the per-terminal resume token.
///
/// Presence of this variable is the whole feature detection: a program that
/// does not find it has no token to compute a `mac` with, so it sends nothing.
pub const RESUME_TOKEN_ENV: &str = "OSC7577_TOKEN";

/// OSC number for the Program Resume Protocol.
pub const OSC_PROGRAM_RESUME: u16 = 7577;

/// Maximum report length we emit; terminals must accept at least this much.
pub const MAX_REPORT_LEN: usize = 2048;

/// Maximum `argv` elements we emit; terminals must accept at least this many.
pub const MAX_ARGV_ELEMENTS: usize = 64;

/// Maximum token bytes we hash. The spec fixes 22-64 characters; anything
/// beyond this bound is non-conformant and is ignored outright so a hostile
/// environment cannot force unbounded hashing on every flush.
const MAX_TOKEN_LEN: usize = 1024;

/// Projection commands for the session-owned resume adapter.
///
/// Like [`crate::program_status::ProgramStatusUpdate`], these carry no input or
/// execution authority: they only describe which resume record the terminal
/// should hold for this session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramResumeUpdate {
    Configure {
        enabled: bool,
    },
    /// Advertise `argv` as the command that resumes this session, in `cwd`.
    ///
    /// `cwd` must be absolute when present. The adapter re-reports only when
    /// the target changes.
    SetTarget {
        argv: Vec<String>,
        cwd: Option<String>,
    },
    /// Withdraw any outstanding resume record (user ended the session).
    Clear,
}

/// Read the resume token from the process environment.
///
/// Returns `None` when the terminal does not support the protocol (variable
/// absent) or exported an empty or absurdly long token. The token itself must
/// never be written to the terminal, logs, or child environments; only the
/// `mac` travels.
pub fn resume_token() -> Option<String> {
    std::env::var(RESUME_TOKEN_ENV)
        .ok()
        .filter(|token| !token.is_empty() && token.len() <= MAX_TOKEN_LEN)
}

/// Current process id for the `pid` key.
pub fn current_pid() -> u32 {
    std::process::id()
}

/// Current Unix time in seconds for the `time` key.
pub fn current_unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Compute the report `mac`: hex SHA-256 over `body:token`.
fn compute_mac(body: &str, token: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(body.as_bytes());
    hash.update(b":");
    hash.update(token.as_bytes());
    let digest = hash.finalize();
    let mut mac = String::with_capacity(digest.len() * 2);
    for byte in digest {
        for nibble in [byte >> 4, byte & 0x0f] {
            mac.push(char::from(if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 }));
        }
    }
    mac
}

fn ensure_reportable_text(text: &str, what: &str) -> Result<()> {
    ensure!(!text.chars().any(char::is_control), "{what} contains control characters");
    Ok(())
}

fn encode_element(text: &str, what: &str) -> Result<String> {
    ensure_reportable_text(text, what)?;
    Ok(STANDARD.encode(text.as_bytes()))
}

fn is_absolute_path(text: &str) -> bool {
    if text.starts_with('/') || text.starts_with("\\\\") {
        return true;
    }
    let mut chars = text.chars();
    matches!((chars.next(), chars.next(), chars.next()), (Some(drive), Some(':'), Some(sep))
        if drive.is_ascii_alphabetic() && matches!(sep, '\\' | '/'))
}

fn ensure_valid_argv(argv: &[String]) -> Result<()> {
    ensure!(argv.len() <= MAX_ARGV_ELEMENTS, "resume argv exceeds element limit");
    let Some(program) = argv.first() else {
        bail!("resume argv must have at least one element");
    };
    ensure!(!program.is_empty(), "resume program must not be empty");
    ensure_reportable_text(program, "resume program")?;
    let bare_name = !program.contains('/') && !program.contains('\\');
    if bare_name {
        ensure!(!program.starts_with('-'), "resume program must not start with '-'");
    } else {
        ensure!(is_absolute_path(program), "resume program must be a bare name or an absolute path");
    }
    for arg in argv.iter().skip(1) {
        ensure_reportable_text(arg, "resume argument")?;
    }
    Ok(())
}

fn ensure_valid_cwd(cwd: &str) -> Result<()> {
    ensure_reportable_text(cwd, "resume working directory")?;
    ensure!(is_absolute_path(cwd), "resume working directory must be absolute");
    Ok(())
}

fn wrap_report(body: &str) -> Result<String> {
    let report = format!("\x1b]{};{body}\x1b\\", OSC_PROGRAM_RESUME);
    ensure!(report.len() <= MAX_REPORT_LEN, "resume report exceeds byte limit");
    Ok(report)
}

/// Validate a resume target without encoding it.
///
/// Adapters call this at the boundary so an invalid target is rejected once
/// instead of failing (and warning) on every flush. Encoding still re-checks,
/// so this is a fast path, not a trust boundary.
pub fn validate_target(argv: &[String], cwd: Option<&str>) -> Result<()> {
    ensure_valid_argv(argv)?;
    if let Some(cwd) = cwd {
        ensure_valid_cwd(cwd)?;
    }
    Ok(())
}

/// Encode a resume report for `pid` made at `time`.
///
/// `argv` is the command the terminal runs to bring the program back; `cwd`
/// is the absolute directory to run it in. Fails closed on any malformed
/// input so callers never emit a report the terminal would reject.
pub fn encode_report(pid: u32, time: u64, argv: &[String], cwd: Option<&str>, token: &str) -> Result<String> {
    ensure!(pid > 0, "resume pid must be positive");
    ensure!(time > 0, "resume time must be positive");
    ensure!(!token.is_empty(), "resume token must not be empty");
    validate_target(argv, cwd)?;

    let mut elements = Vec::with_capacity(argv.len());
    for (index, arg) in argv.iter().enumerate() {
        elements.push(encode_element(
            arg,
            if index == 0 {
                "resume program"
            } else {
                "resume argument"
            },
        )?);
    }
    let mut body = format!("pid={pid}:time={time}:argv={}", elements.join(","));
    if let Some(cwd) = cwd {
        body.push_str(":cwd=");
        body.push_str(&encode_element(cwd, "resume working directory")?);
    }
    let mac = compute_mac(&body, token);
    body.push_str(":mac=");
    body.push_str(&mac);
    wrap_report(&body)
}

/// Encode a record removal for `pid` made at `time`.
///
/// A program sends this when the user ends it and it should no longer be
/// resumed. Never send it on SIGHUP, SIGTERM, or terminal loss: that is when
/// the record is needed.
pub fn encode_clear(pid: u32, time: u64, token: &str) -> Result<String> {
    ensure!(pid > 0, "resume pid must be positive");
    ensure!(time > 0, "resume time must be positive");
    ensure!(!token.is_empty(), "resume token must not be empty");
    let body = format!("pid={pid}:time={time}:clear=1");
    let mac = compute_mac(&body, token);
    wrap_report(&format!("{body}:mac={mac}"))
}

#[cfg(test)]
mod tests;
