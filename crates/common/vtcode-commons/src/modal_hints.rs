//! Shared copy for inline list modals: footer key hints and recurring sentences.
//!
//! Modal headers stay descriptive (full sentences, no keybindings); all
//! keybinding guidance lives in `footer_hint` / summary rows using these
//! constants so approval, limit, and picker modals read identically.

/// Navigate, select, or deny: tool approval, hook approval, session-limit, and
/// policy-denied prompts.
pub const APPROVAL_NAVIGATE_DENY: &str = "Use ↑↓ or Tab to navigate • Enter to select • Esc to deny";
/// Navigate, select, or cancel: MCP approval variant.
pub const APPROVAL_NAVIGATE_CANCEL: &str = "Use ↑↓ or Tab to navigate • Enter to select • Esc to cancel";
/// Navigate, select, or stop: tool-loop-limit variant.
pub const APPROVAL_NAVIGATE_STOP: &str = "Use ↑↓ or Tab to navigate • Enter to select • Esc to stop";

/// Model picker: navigate a long provider/model list with live filtering.
pub const MODEL_PICKER_NAVIGATE_FILTER: &str =
    "Use ↑↓ or Tab to navigate • Enter to select • Type to filter • Esc to cancel";
/// Model picker follow-ups (reasoning, service tier): short single-choice list.
/// Esc cancels the whole picker; `← Back to model list` returns to step 1.
///
/// Only pass this to lists without `InlineListSelection::ConfigAction` items:
/// `ConfigAction` lists are `FixedComfortable` and render the shared
/// `CONFIG_LIST_NAVIGATION_HINT`, dropping any explicit footer.
pub const MODEL_PICKER_FOLLOW_UP_HINT: &str =
    "Use ↑↓ or Tab to navigate • Enter to select • ← Back to model list • Esc cancels the picker";

/// Trailing sentence before approval options, e.g. `choose_handling_line("this tool")`.
pub fn choose_handling_line(object: &str) -> String {
    format!("Choose how to handle {object}:")
}

/// Truncate modal body text to `max_chars` characters, appending `…` only when
/// truncation actually happened so shortened text never reads as complete.
pub fn truncate_modal_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut truncated = text.chars().take(max_chars.saturating_sub(1)).collect::<String>();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_hints_share_navigation_prefix_but_differ_by_exit_verb() {
        for hint in [APPROVAL_NAVIGATE_DENY, APPROVAL_NAVIGATE_CANCEL, APPROVAL_NAVIGATE_STOP] {
            assert!(hint.starts_with("Use ↑↓ or Tab to navigate • Enter to select • Esc to "));
        }
        assert!(APPROVAL_NAVIGATE_DENY.ends_with("deny"));
        assert!(APPROVAL_NAVIGATE_CANCEL.ends_with("cancel"));
        assert!(APPROVAL_NAVIGATE_STOP.ends_with("stop"));
    }

    #[test]
    fn model_picker_hints_share_navigation_prefix() {
        for hint in [MODEL_PICKER_NAVIGATE_FILTER, MODEL_PICKER_FOLLOW_UP_HINT] {
            assert!(hint.starts_with("Use ↑↓ or Tab to navigate • Enter to "));
        }
        assert!(MODEL_PICKER_NAVIGATE_FILTER.contains("Type to filter"));
        assert!(MODEL_PICKER_FOLLOW_UP_HINT.contains("Back to model list"));
        assert!(MODEL_PICKER_FOLLOW_UP_HINT.contains("Esc"));
    }

    #[test]
    fn choose_handling_line_interpolates_object_as_sentence() {
        assert_eq!(choose_handling_line("this tool"), "Choose how to handle this tool:");
        assert_eq!(
            choose_handling_line("workspace lifecycle hooks"),
            "Choose how to handle workspace lifecycle hooks:"
        );
    }

    #[test]
    fn truncate_modal_text_marks_truncation_with_ellipsis_only_when_shortened() {
        assert_eq!(truncate_modal_text("short", 10), "short");
        assert_eq!(truncate_modal_text("exactly-ten", 11), "exactly-ten");
        assert_eq!(truncate_modal_text("over the limit text", 10), "over the …");
    }
}
