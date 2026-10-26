use std::path::PathBuf;

const MAX_NAVIGATION_ENTRIES: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NavigationLocation {
    pub(super) workspace: PathBuf,
    pub(super) session_id: Option<String>,
}

#[derive(Default)]
pub(super) struct NavigationHistory {
    entries: Vec<NavigationLocation>,
    current_index: Option<usize>,
}

impl NavigationHistory {
    pub(super) fn record(&mut self, location: NavigationLocation) {
        if self.current_index.and_then(|index| self.entries.get(index)) == Some(&location) {
            return;
        }

        if let Some(index) = self.current_index {
            self.entries.truncate(index + 1);
        }
        self.entries.push(location);
        self.current_index = Some(self.entries.len() - 1);

        if self.entries.len() > MAX_NAVIGATION_ENTRIES {
            let _evicted = self.entries.remove(0);
            self.current_index = self.current_index.map(|index| index.saturating_sub(1));
        }
    }

    pub(super) fn back_index(&self) -> Option<usize> {
        self.current_index?.checked_sub(1)
    }

    pub(super) fn forward_index(&self) -> Option<usize> {
        let next_index = self.current_index?.checked_add(1)?;
        (next_index < self.entries.len()).then_some(next_index)
    }

    pub(super) fn location(&self, index: usize) -> Option<&NavigationLocation> {
        self.entries.get(index)
    }

    pub(super) fn navigate_to(&mut self, index: usize) -> bool {
        if index >= self.entries.len() {
            return false;
        }
        self.current_index = Some(index);
        true
    }

    pub(super) fn replace_and_navigate_to(&mut self, index: usize, location: NavigationLocation) -> bool {
        let Some(entry) = self.entries.get_mut(index) else {
            return false;
        };
        *entry = location;
        self.current_index = Some(index);
        true
    }
}

#[derive(Clone, Copy)]
pub(super) struct PendingNavigation {
    pub(super) target_index: usize,
    pub(super) workspace_switch: bool,
}

#[cfg(test)]
mod tests {
    use super::{NavigationHistory, NavigationLocation};
    use std::path::PathBuf;

    fn location(workspace: &str, session_id: Option<&str>) -> NavigationLocation {
        NavigationLocation {
            workspace: PathBuf::from(workspace),
            session_id: session_id.map(str::to_owned),
        }
    }

    #[test]
    fn records_locations_and_truncates_forward_entries_after_new_navigation() {
        let mut history = NavigationHistory::default();
        history.record(location("/repo", Some("one")));
        history.record(location("/repo", Some("two")));
        history.record(location("/other", None));

        assert!(history.navigate_to(0));
        history.record(location("/repo", Some("three")));

        assert_eq!(history.back_index(), Some(0));
        assert_eq!(history.forward_index(), None);
        assert_eq!(history.location(1), Some(&location("/repo", Some("three"))));
    }

    #[test]
    fn bounds_history_and_rejects_invalid_cursor_targets() {
        let mut history = NavigationHistory::default();
        for index in 0..=100 {
            history.record(location("/repo", Some(&index.to_string())));
        }

        assert_eq!(history.location(0), Some(&location("/repo", Some("1"))));
        assert!(!history.navigate_to(100));
        assert_eq!(history.forward_index(), None);
    }
}
