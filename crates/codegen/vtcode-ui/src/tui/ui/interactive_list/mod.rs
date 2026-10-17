use std::io;

use crate::tui::utils::tty::TtyExt;
use anyhow::{Context, Result, anyhow};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event;
use ratatui::widgets::ListState;

mod input;
mod render;
mod terminal;

/// Minimum entry count that enables the search field automatically.
pub(crate) const SEARCH_MIN_ENTRIES: usize = 3;

#[derive(Debug, Clone)]
pub struct SelectionEntry {
    pub title: String,
    pub description: Option<String>,
    /// Extra search terms (ids, aliases, provider labels) matched by the filter.
    pub keywords: Vec<String>,
}

impl SelectionEntry {
    pub fn new(title: impl Into<String>, description: Option<String>) -> Self {
        Self {
            title: title.into(),
            description,
            keywords: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_keywords<I, S>(mut self, keywords: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }
}

/// Live filter + selection over the entry catalog.
pub(crate) struct SelectionListState {
    query: String,
    visible: Vec<usize>,
    /// Selection position within `visible` (not the original index).
    selected_pos: usize,
    search_enabled: bool,
}

impl SelectionListState {
    fn new(entries: &[SelectionEntry], default_index: usize) -> Self {
        let search_enabled = entries.len() >= SEARCH_MIN_ENTRIES;
        let mut state = Self {
            query: String::new(),
            visible: (0..entries.len()).collect(),
            selected_pos: 0,
            search_enabled,
        };
        let default_pos = state.visible.iter().position(|&index| index == default_index).unwrap_or(0);
        state.selected_pos = default_pos.min(state.visible.len().saturating_sub(1));
        state
    }

    pub(crate) fn search_enabled(&self) -> bool {
        self.search_enabled
    }

    pub(crate) fn is_filtering(&self) -> bool {
        self.search_enabled && !self.query.is_empty()
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn visible_indices(&self) -> &[usize] {
        &self.visible
    }

    pub(crate) fn selected_visible_index(&self) -> Option<usize> {
        if self.visible.is_empty() {
            None
        } else {
            Some(self.selected_pos.min(self.visible.len() - 1))
        }
    }

    pub(crate) fn selected_entry<'a>(&self, entries: &'a [SelectionEntry]) -> Option<&'a SelectionEntry> {
        let original = self.selected_original_index()?;
        entries.get(original)
    }

    fn selected_original_index(&self) -> Option<usize> {
        if self.visible.is_empty() {
            return None;
        }
        let pos = self.selected_pos.min(self.visible.len() - 1);
        self.visible.get(pos).copied()
    }

    fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            self.selected_pos = 0;
            return;
        }
        let len = self.visible.len() as isize;
        let next = self.selected_pos as isize + delta;
        self.selected_pos = next.rem_euclid(len) as usize;
    }

    /// Page jump clamps at the ends (does not wrap like `move_selection`).
    fn page_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            self.selected_pos = 0;
            return;
        }
        let len = self.visible.len() as isize;
        let next = (self.selected_pos as isize + delta).clamp(0, len - 1);
        self.selected_pos = next as usize;
    }

    fn set_selection_pos(&mut self, pos: usize) {
        if self.visible.is_empty() {
            self.selected_pos = 0;
            return;
        }
        self.selected_pos = pos.min(self.visible.len() - 1);
    }

    fn jump_to_original(&mut self, original: usize) {
        if let Some(pos) = self.visible.iter().position(|&index| index == original) {
            self.selected_pos = pos;
        }
    }

    fn recompute_visible(&mut self, entries: &[SelectionEntry], previous_original: Option<usize>) {
        self.visible = render::filter_entries(entries, &self.query, true);
        if self.visible.is_empty() {
            self.selected_pos = 0;
            return;
        }
        if let Some(previous) = previous_original {
            if let Some(pos) = self.visible.iter().position(|&index| index == previous) {
                self.selected_pos = pos;
                return;
            }
        }
        self.selected_pos = self.selected_pos.min(self.visible.len() - 1);
    }

    fn push_char(&mut self, ch: char, entries: &[SelectionEntry]) {
        let previous = self.selected_original_index();
        self.query.push(ch);
        self.recompute_visible(entries, previous);
    }

    fn backspace(&mut self, entries: &[SelectionEntry]) -> bool {
        if self.query.pop().is_none() {
            return false;
        }
        let previous = self.selected_original_index();
        self.recompute_visible(entries, previous);
        true
    }

    fn clear_query(&mut self, entries: &[SelectionEntry]) -> bool {
        if self.query.is_empty() {
            return false;
        }
        let previous = self.selected_original_index();
        self.query.clear();
        self.visible = (0..entries.len()).collect();
        if let Some(previous) = previous {
            self.jump_to_original(previous);
        } else if self.visible.is_empty() {
            self.selected_pos = 0;
        } else {
            self.selected_pos = self.selected_pos.min(self.visible.len() - 1);
        }
        true
    }
}

#[derive(Debug, thiserror::Error)]
#[error("selection interrupted by Ctrl+C")]
pub struct SelectionInterrupted;

pub fn run_interactive_selection(
    title: &str,
    instructions: &str,
    entries: &[SelectionEntry],
    default_index: usize,
) -> Result<Option<usize>> {
    if entries.is_empty() {
        return Err(anyhow!("No options available for selection"));
    }

    if !io::stderr().is_tty_ext() {
        return Err(anyhow!("Terminal UI is unavailable"));
    }

    let mut stderr = io::stderr();
    let mut terminal_guard = TerminalModeGuard::new(title);
    terminal_guard.save_cursor_position(&mut stderr);
    terminal_guard.enable_raw_mode()?;
    terminal_guard.enter_alternate_screen(&mut stderr)?;

    let backend = CrosstermBackend::new(stderr);
    let mut terminal = Terminal::new(backend)
        .with_context(|| format!("Failed to initialize Ratatui terminal for {title} selector"))?;
    terminal_guard.hide_cursor(&mut terminal)?;

    let selection_result = (|| -> Result<Option<usize>> {
        let mut state = SelectionListState::new(entries, default_index);
        let mut number_buffer = String::new();
        let mut list_state = ListState::default();

        loop {
            render::draw_selection_ui(&mut terminal, title, instructions, entries, &state, &mut list_state)?;

            let event = event::read().with_context(|| format!("Failed to read terminal input for {title} selector"))?;
            match input::handle_event(event, entries, &mut state, &mut number_buffer)? {
                input::SelectionAction::Continue => {}
                input::SelectionAction::Select => {
                    return Ok(state.selected_original_index());
                }
                input::SelectionAction::Cancel => return Ok(None),
            }
        }
    })();

    let cleanup_result = terminal_guard.restore_with_terminal(&mut terminal);
    cleanup_result?;
    selection_result
}

use terminal::TerminalModeGuard;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entries() -> Vec<SelectionEntry> {
        vec![
            SelectionEntry::new("Claude 4 Sonnet", Some("reasoning tools".to_string()))
                .with_keywords(["anthropic", "claude-sonnet-4"]),
            SelectionEntry::new("GPT-5", Some("reasoning".to_string())).with_keywords(["openai", "gpt-5.4"]),
            SelectionEntry::new("GLM Flash", Some("fast chat".to_string())).with_keywords(["zhipu"]),
            SelectionEntry::new("Local Llama", Some("offline".to_string())).with_keywords(["ollama"]),
        ]
    }

    #[test]
    fn search_enabled_for_catalog_at_or_above_threshold() {
        let small = SelectionListState::new(&sample_entries()[..2], 0);
        assert!(!small.search_enabled(), "2 entries stay search-less");
        let large = SelectionListState::new(&sample_entries(), 0);
        assert!(large.search_enabled(), "4 entries enable search");
    }

    #[test]
    fn typing_filters_by_label_description_and_keywords() {
        let entries = sample_entries();
        let mut state = SelectionListState::new(&entries, 0);
        for ch in "zhipu".chars() {
            state.push_char(ch, &entries);
        }
        assert_eq!(state.visible_indices(), &[2], "keyword zhipu matches GLM");
        state.clear_query(&entries);
        for ch in "offline".chars() {
            state.push_char(ch, &entries);
        }
        assert_eq!(state.visible_indices(), &[3], "description offline matches");
        state.clear_query(&entries);
        for ch in "sonnet".chars() {
            state.push_char(ch, &entries);
        }
        assert!(state.visible_indices().contains(&0), "label Claude 4 Sonnet matches: {:?}", state.visible_indices());
    }

    #[test]
    fn enter_returns_original_index_after_filter() {
        let entries = sample_entries();
        let mut state = SelectionListState::new(&entries, 0);
        state.push_char('g', &entries);
        state.push_char('l', &entries);
        state.push_char('m', &entries);
        assert_eq!(state.visible_indices(), &[2]);
        assert_eq!(state.selected_original_index(), Some(2));
    }

    #[test]
    fn navigation_stays_on_visible_subset() {
        let entries = sample_entries();
        let mut state = SelectionListState::new(&entries, 0);
        state.push_char('e', &entries); // Claude(0), GLM?(2)? "e" in reasoning/fast/offline
        state.move_selection(1);
        let selected = state.selected_original_index();
        assert!(
            state.visible_indices().contains(&selected.unwrap_or(usize::MAX)),
            "selection must stay inside the filtered set"
        );
    }

    #[test]
    fn empty_filter_result_blocks_selection() {
        let entries = sample_entries();
        let mut state = SelectionListState::new(&entries, 0);
        for ch in "zzzzz".chars() {
            state.push_char(ch, &entries);
        }
        assert!(state.visible_indices().is_empty());
        assert!(state.selected_entry(&entries).is_none());
        assert_eq!(state.selected_original_index(), None);
    }

    #[test]
    fn digit_jump_uses_original_catalog_index() {
        let entries = sample_entries();
        let mut state = SelectionListState::new(&entries, 0);
        state.jump_to_original(2);
        assert_eq!(state.selected_original_index(), Some(2));
    }

    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    fn press(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn jk_navigate_when_query_empty_and_type_when_filtering() {
        let entries = sample_entries();
        let mut state = SelectionListState::new(&entries, 0);
        let mut buf = String::new();
        input::handle_event(press(KeyCode::Char('j')), &entries, &mut state, &mut buf).unwrap();
        assert_eq!(state.selected_original_index(), Some(1), "j moves down when not filtering");
        input::handle_event(press(KeyCode::Char('k')), &entries, &mut state, &mut buf).unwrap();
        assert_eq!(state.selected_original_index(), Some(0), "k moves up when not filtering");

        input::handle_event(press(KeyCode::Char('z')), &entries, &mut state, &mut buf).unwrap();
        assert!(state.is_filtering(), "other printables start a filter");
        input::handle_event(press(KeyCode::Char('j')), &entries, &mut state, &mut buf).unwrap();
        assert_eq!(state.query(), "zj", "j types while filtering");
    }

    #[test]
    fn digits_jump_when_empty_and_type_when_filtering() {
        let entries = sample_entries();
        let mut state = SelectionListState::new(&entries, 0);
        let mut buf = String::new();
        input::handle_event(press(KeyCode::Char('3')), &entries, &mut state, &mut buf).unwrap();
        assert_eq!(state.selected_original_index(), Some(2), "digit jump uses original index");
        input::handle_event(press(KeyCode::Char('z')), &entries, &mut state, &mut buf).unwrap();
        input::handle_event(press(KeyCode::Char('1')), &entries, &mut state, &mut buf).unwrap();
        assert_eq!(state.query(), "z1", "digits type once filtering");
    }

    #[test]
    fn esc_clears_query_then_cancels() {
        let entries = sample_entries();
        let mut state = SelectionListState::new(&entries, 0);
        let mut buf = String::new();
        input::handle_event(press(KeyCode::Char('z')), &entries, &mut state, &mut buf).unwrap();
        let action = input::handle_event(press(KeyCode::Esc), &entries, &mut state, &mut buf).unwrap();
        assert!(matches!(action, input::SelectionAction::Continue));
        assert!(!state.is_filtering());
        let action = input::handle_event(press(KeyCode::Esc), &entries, &mut state, &mut buf).unwrap();
        assert!(matches!(action, input::SelectionAction::Cancel));
    }

    #[test]
    fn enter_blocked_when_filter_empty_and_returns_original_index() {
        let entries = sample_entries();
        let mut state = SelectionListState::new(&entries, 0);
        let mut buf = String::new();
        input::handle_event(press(KeyCode::Char('z')), &entries, &mut state, &mut buf).unwrap();
        input::handle_event(press(KeyCode::Char('z')), &entries, &mut state, &mut buf).unwrap();
        input::handle_event(press(KeyCode::Char('z')), &entries, &mut state, &mut buf).unwrap();
        let action = input::handle_event(press(KeyCode::Enter), &entries, &mut state, &mut buf).unwrap();
        assert!(matches!(action, input::SelectionAction::Continue), "Enter is a no-op with zero matches");
        state.clear_query(&entries);
        for ch in "zhipu".chars() {
            input::handle_event(press(KeyCode::Char(ch)), &entries, &mut state, &mut buf).unwrap();
        }
        let action = input::handle_event(press(KeyCode::Enter), &entries, &mut state, &mut buf).unwrap();
        assert!(matches!(action, input::SelectionAction::Select));
        assert_eq!(state.selected_original_index(), Some(2));
    }

    #[test]
    fn page_down_clamps_instead_of_wrapping() {
        let entries = sample_entries();
        let mut state = SelectionListState::new(&entries, 0);
        let mut buf = String::new();
        input::handle_event(press(KeyCode::PageDown), &entries, &mut state, &mut buf).unwrap();
        assert_eq!(state.selected_original_index(), Some(3), "page down clamps at end");
        input::handle_event(press(KeyCode::PageDown), &entries, &mut state, &mut buf).unwrap();
        assert_eq!(state.selected_original_index(), Some(3), "still clamped");
        input::handle_event(press(KeyCode::PageUp), &entries, &mut state, &mut buf).unwrap();
        assert_eq!(state.selected_original_index(), Some(0), "page up clamps at start");
    }
}
