//! Single live-session ownership: message + detection shared by service and app.
//!
//! The canonical event log is single-writer (exclusive flock on `session.lock`).
//! Live sessions (CLI interactive, desktop service) open it with
//! `open_exclusive` and fail with `SessionBusy` when another process holds it.
//! Background readers (retention, query, inspector history) intentionally use
//! shared `open` and stay available. This module is the one place that names
//! the busy text so the service message and the desktop UI detection cannot drift.

/// Stable substring present in every session-busy message.
pub const SESSION_BUSY_MARKER: &str = "already open in another process";

/// Full actionable busy message for `session_id`.
#[must_use]
pub fn session_busy_message(session_id: &str) -> String {
    format!(
        "session {session_id} is already open in another process; close it there and reload, or open a copy with vtcode --fork-session {session_id}"
    )
}

/// Whether a backend error message reports session-busy contention.
#[must_use]
pub fn is_session_busy_message(message: &str) -> bool {
    message.contains(SESSION_BUSY_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_message_names_contention_session_and_fork_remedy() {
        let message = session_busy_message("thread-9");
        assert!(message.contains(SESSION_BUSY_MARKER));
        assert!(message.contains("thread-9"));
        assert!(message.contains("close it there and reload"));
        assert!(message.contains("vtcode --fork-session thread-9"));
    }

    #[test]
    fn busy_detection_matches_any_message_carrying_the_marker() {
        assert!(is_session_busy_message(&session_busy_message("a")));
        // Substring semantics, not a copy of another crate's wording: any
        // present or future busy text carrying the marker must match.
        assert!(is_session_busy_message(&format!("prefix {} suffix", SESSION_BUSY_MARKER)));
        assert!(!is_session_busy_message("saved session was not found"));
        assert!(!is_session_busy_message(""));
    }
}
