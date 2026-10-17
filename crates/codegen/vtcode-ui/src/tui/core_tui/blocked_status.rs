//! Shared needles for detecting blocked turns in streamed status text.
//!
//! The runloop flags blocked turns by embedding markers in the input status it
//! streams to the TUI (`[BLOCKED]` on the right status, "blocked" wording on
//! the left status); the dedicated `is_blocked` flag stays runloop-side and is
//! not part of the inline UI protocol. The header badge and the footer hint
//! must agree on the same needles, so they are centralized here and a reword
//! cannot silently break one of the indicators.

use crate::tui::config::constants::ui;

/// Whether text uses the existing labelled or compact Git status format.
pub(crate) fn is_git_status(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with(ui::HEADER_GIT_PREFIX)
        || (!trimmed.contains(char::is_whitespace)
            && (trimmed.ends_with(ui::HEADER_GIT_CLEAN_SUFFIX) || trimmed.ends_with(ui::HEADER_GIT_DIRTY_SUFFIX)))
}

/// Marker the runloop prepends to the right input status on blocked turns.
pub(crate) const BLOCKED_STATUS_NEEDLE: &str = "[BLOCKED]";

/// Case-insensitive substring matched against the left input status.
pub(crate) const BLOCKED_LEFT_NEEDLE: &str = "blocked";

/// Left-status marker of the tool-free recovery pass.
pub(crate) const TOOLS_DISABLED_NEEDLE: &str = "tools disabled";

/// Left-status marker for recovery wording.
pub(crate) const RECOVERY_NEEDLE: &str = "recovery";

/// Whether the right input status carries the blocked chip.
pub(crate) fn right_status_is_blocked(right_status: &str) -> bool {
    right_status.contains(BLOCKED_STATUS_NEEDLE)
}

/// Whether the left input status mentions the blocked state (case-insensitive).
pub(crate) fn left_status_is_blocked(left_status: &str) -> bool {
    !is_git_status(left_status) && left_status.to_ascii_lowercase().contains(BLOCKED_LEFT_NEEDLE)
}

/// Whether the left input status mentions the tool-free recovery pass.
pub(crate) fn left_status_mentions_tools_disabled(left_status: &str) -> bool {
    !is_git_status(left_status) && left_status.to_ascii_lowercase().contains(TOOLS_DISABLED_NEEDLE)
}

/// Whether the left input status mentions the recovery pass (case-insensitive).
pub(crate) fn left_status_is_recovery(left_status: &str) -> bool {
    if is_git_status(left_status) {
        return false;
    }
    let lowered = left_status.to_ascii_lowercase();
    lowered.contains(RECOVERY_NEEDLE) || lowered.contains(TOOLS_DISABLED_NEEDLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_branch_names_do_not_select_blocked_or_recovery_states() {
        for status in [
            "git: fix/blocked | *",
            "fix/blocked*",
            "fix/recovery✓",
            "git: fix/recovery | *",
        ] {
            assert!(is_git_status(status), "{status}");
            assert!(!left_status_is_blocked(status), "{status}");
            assert!(!left_status_is_recovery(status), "{status}");
            assert!(!left_status_mentions_tools_disabled(status), "{status}");
        }
        assert!(!is_git_status("Running command: grep blocked*"));
        assert!(left_status_is_blocked("Running command: grep blocked*"));
        assert!(left_status_is_recovery("Recovery: tools disabled*"));
        assert!(left_status_mentions_tools_disabled("Recovery: tools disabled*"));
    }

    #[test]
    fn right_status_needle_matches_blocked_chip_only() {
        assert!(right_status_is_blocked("[BLOCKED] 84% context left"));
        assert!(!right_status_is_blocked("84% context left"));
        assert!(!right_status_is_blocked("blocked 84% context left"));
    }

    #[test]
    fn left_status_needle_is_case_insensitive() {
        assert!(left_status_is_blocked("Blocked — Type 'continue' to retry..."));
        assert!(left_status_is_blocked("turn blocked by policy"));
        assert!(!left_status_is_blocked("Running tool: edit_file"));
        assert!(!left_status_is_blocked(""));
    }

    #[test]
    fn recovery_needles_match_recovery_wording() {
        assert!(left_status_is_recovery("Recovery: tools disabled..."));
        assert!(left_status_is_recovery("recovery pass running"));
        assert!(left_status_mentions_tools_disabled("Recovery: tools disabled..."));
        assert!(!left_status_is_recovery("Running command: ls"));
    }
}
