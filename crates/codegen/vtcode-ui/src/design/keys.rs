//! Shared keyboard and interaction copy for list modals and command menus.
//!
//! One source of truth so footer hints cannot drift across surfaces. Pair
//! these with `design::list` row kinds: nested views use [`BACK`], top-level
//! pickers use [`CANCEL`], filter-aware lists append [`FILTER`].

/// Move selection.
pub const NAVIGATE: &str = "↑↓ select";
/// Commit the highlighted row.
pub const APPLY: &str = "Enter apply";
/// Change a setting value without cycling the row.
pub const ADJUST: &str = "←→ value";
/// Type-to-filter affordance (when the list has a search field).
pub const FILTER: &str = "type to filter";
/// Nested view: leave this level (root Esc closes).
pub const BACK: &str = "Esc back";
/// Top-level picker: dismiss (after clearing a non-empty filter).
pub const CANCEL: &str = "Esc cancel";
/// Vim-style movement used by standalone ratatui pickers.
pub const MOVE_VIM: &str = "↑↓/jk move";
/// Jump to first/last row.
pub const ENDS: &str = "Home/End";
/// Confirm in standalone pickers.
pub const CONFIRM: &str = "Enter confirm";
/// Standalone picker Esc: clear filter, then cancel.
pub const CLEAR_OR_CANCEL: &str = "Esc clear/cancel";

const SEP: &str = " · ";

/// Footer for shared list modals with a search field (settings, model picker).
pub fn list_hint() -> String {
    [NAVIGATE, APPLY, ADJUST, FILTER, BACK].join(SEP)
}

/// Footer for standalone choice pickers (`run_interactive_selection`).
pub fn choice_hint() -> String {
    [MOVE_VIM, ENDS, CONFIRM, CLEAR_OR_CANCEL].join(SEP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_hint_includes_filter_and_back() {
        let hint = list_hint();
        assert!(hint.contains(FILTER));
        assert!(hint.contains(BACK));
        assert!(hint.contains(NAVIGATE));
    }

    #[test]
    fn choice_hint_matches_standalone_picker_verb_set() {
        let hint = choice_hint();
        assert!(hint.contains(MOVE_VIM));
        assert!(hint.contains(CLEAR_OR_CANCEL));
    }
}
