//! Shared session poll/wait run identity for transcript coalescing.
//!
//! A long command is polled once per wait round-trip, so one build can
//! produce a dozen `Wait for command session` headers while it runs. Both
//! transcript surfaces (CLI renderer, desktop app) collapse consecutive
//! follow-ups for one session into a single header instead. This module owns
//! the run predicate so the surfaces can never disagree on what coalesces:
//! poll-like follow-ups (wait/poll/read) carrying a session id. Launches,
//! stdin sends, closes, and inspects always keep their own row.

use serde_json::Value;

use super::{READ_COMMAND_SESSION, WAIT_FOR_COMMAND_SESSION, lookup_string, session_followup_title};

/// Identity of one coalescible poll run: the session plus the action label.
///
/// The label comes from the same reader as the row title, so key and title
/// agree by construction; matching on the shared consts (never retyped
/// literals) keeps that agreement compile-checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionPollKey {
    pub session_id: String,
    pub label: &'static str,
}

/// Extract the coalescing key for a session poll/wait invocation.
///
/// `tool_name` is a registry name (`write_stdin`, `command_session_internal`,
/// `read_pty_session`, ...). Returns `None` for anything that is not a
/// session wait/poll/read or that carries no session id.
#[must_use]
pub fn session_poll_key(tool_name: &str, args: &Value) -> Option<SessionPollKey> {
    let title = session_followup_title(tool_name, args)?;
    let label = if title == WAIT_FOR_COMMAND_SESSION {
        WAIT_FOR_COMMAND_SESSION
    } else if title == READ_COMMAND_SESSION {
        READ_COMMAND_SESSION
    } else {
        return None;
    };
    let session_id = lookup_string(args, "session_id")?;
    Some(SessionPollKey { session_id, label })
}

/// Compact progress tick for a follow-up poll/wait (`×3 · waited 12s`).
///
/// Both surfaces phrase follow-ups identically: the desktop survivor title
/// suffixes `×N`, and the CLI prints one tick line per follow-up. Elapsed
/// time under a second is omitted (attempts alone carry fast runs); totals
/// render rounded to whole seconds.
#[must_use]
pub fn poll_run_tick(attempts: usize, waited_secs: f64) -> String {
    if waited_secs >= 1.0 {
        format!("×{attempts} · waited {waited_secs:.0}s")
    } else {
        format!("×{attempts}")
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn key(tool: &str, args: Value) -> Option<SessionPollKey> {
        session_poll_key(tool, &args)
    }

    #[test]
    fn poll_key_covers_session_waits_and_reads_only() {
        let wait = json!({"session_id": "run-1", "action": "wait", "wait_timeout_seconds": 8});
        assert_eq!(
            key("write_stdin", wait),
            Some(SessionPollKey {
                session_id: "run-1".into(),
                label: WAIT_FOR_COMMAND_SESSION
            })
        );
        // Case-insensitive actions and the other poll spellings join the run.
        for (tool, args, label) in [
            ("write_stdin", json!({"session_id": "run-1", "action": "WAIT"}), WAIT_FOR_COMMAND_SESSION),
            ("write_stdin", json!({"session_id": "run-1", "action": "poll"}), READ_COMMAND_SESSION),
            ("write_stdin", json!({"session_id": "run-1"}), READ_COMMAND_SESSION),
            ("write_stdin", json!({"session_id": "run-1", "action": "write", "input": ""}), READ_COMMAND_SESSION),
            ("command_session_internal", json!({"session_id": "s", "action": "wait"}), WAIT_FOR_COMMAND_SESSION),
            ("command_session_internal", json!({"session_id": "s", "action": "poll"}), READ_COMMAND_SESSION),
            ("read_pty_session", json!({"session_id": "s"}), READ_COMMAND_SESSION),
        ] {
            let got = key(tool, args.clone()).unwrap_or_else(|| panic!("{tool} {args} coalesces"));
            assert_eq!(got.label, label, "{tool} {args} coalesces under its row label");
        }
    }

    #[test]
    fn poll_tick_shows_attempts_and_whole_seconds() {
        assert_eq!(poll_run_tick(2, 0.0), "×2");
        // Sub-second totals omit the elapsed half (both sides of 1s).
        assert_eq!(poll_run_tick(2, 0.9), "×2");
        assert_eq!(poll_run_tick(2, 1.0), "×2 · waited 1s");
        // Totals render rounded from accumulated fractional waits.
        assert_eq!(poll_run_tick(3, 7.06), "×3 · waited 7s");
        assert_eq!(poll_run_tick(7, 61.4), "×7 · waited 61s");
    }

    #[test]
    fn poll_key_rejects_non_poll_calls() {
        // Sends, launches, lifecycle actions, and session-less calls keep rows.
        for (tool, args) in [
            ("write_stdin", json!({"session_id": "run-1", "action": "write", "input": "ls"})),
            ("write_stdin", json!({"action": "wait"})),
            ("write_stdin", json!({"session_id": "run-1", "action": "close"})),
            ("write_stdin", json!({"session_id": "run-1", "action": "inspect"})),
            ("write_stdin", json!({"session_id": "run-1", "action": "terminate"})),
            ("command_session_internal", json!({"session_id": "s", "action": "run", "cmd": "ls"})),
            ("command_session_internal", json!({"session_id": "s", "action": "close"})),
            ("send_pty_input", json!({"session_id": "s"})),
            ("exec_command", json!({"command": "ls"})),
            ("read_file", json!({"path": "a.rs"})),
        ] {
            assert!(key(tool, args.clone()).is_none(), "{tool} {args} keeps its row");
        }
    }
}
