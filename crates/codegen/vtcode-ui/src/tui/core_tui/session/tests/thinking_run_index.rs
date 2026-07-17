#![allow(missing_docs)]
use super::super::*;

/// Unit tests for the isolated `ThinkingRunIndex` state, exercised without a
/// full `Session` to demonstrate the run bookkeeping is independently testable.
#[cfg(test)]
mod thinking_run_index_tests {
    use super::*;

    #[test]
    fn resolves_config_default_until_overridden() {
        let mut index = ThinkingRunIndex::default();
        // No override: falls back to the supplied default.
        assert!(index.is_collapsed(0, true));
        assert!(!index.is_collapsed(0, false));

        // Explicit override wins over the default.
        index.set_collapsed(0, false);
        assert!(!index.is_collapsed(0, true));

        index.set_collapsed(0, true);
        assert!(index.is_collapsed(0, false));
    }

    #[test]
    fn tracks_active_streaming_run() {
        let mut index = ThinkingRunIndex::default();
        assert_eq!(index.active_start(), None);

        index.begin_run(7);
        assert_eq!(index.active_start(), Some(7));

        index.end_run();
        assert_eq!(index.active_start(), None);
    }

    #[test]
    fn clear_resets_all_state() {
        let mut index = ThinkingRunIndex::default();
        index.set_collapsed(3, true);
        index.begin_run(3);
        index.clear();

        assert_eq!(index.active_start(), None);
        assert!(!index.is_collapsed(3, false));
    }
}
