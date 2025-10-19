use std::ptr;

use ratatui::widgets::ListState;
use unicode_segmentation::UnicodeSegmentation;

use crate::ui::search::normalize_query;
use crate::ui::slash::{SlashCommandInfo, suggestions_for};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandRange {
    pub start: usize,
    pub end: usize,
}

pub fn command_range(input: &str, cursor: usize) -> Option<SlashCommandRange> {
    if !input.starts_with('/') {
        return None;
    }

    let mut last_range = None;
    let mut active_range = None;

    for (index, grapheme) in input.grapheme_indices(true) {
        if index > cursor {
            break;
        }

        if grapheme == "/" {
            active_range = Some(SlashCommandRange {
                start: index,
                end: input.len(),
            });
        } else if grapheme.chars().all(char::is_whitespace) {
            if let Some(mut range) = active_range.take() {
                range.end = index;
                last_range = Some(range);
            }
        } else if let Some(range) = &mut active_range {
            range.end = index + grapheme.len();
        }
    }

    if let Some(range) = active_range {
        if range.end > range.start {
            return Some(range);
        }
    }

    last_range.filter(|range| range.end > range.start)
}

pub fn command_prefix(input: &str, cursor: usize) -> Option<String> {
    let range = command_range(input, cursor)?;
    let end = cursor.min(range.end);
    let start = range.start + 1;
    if end < start {
        return Some(String::new());
    }
    Some(input[start..end].to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashPaletteHighlightSegment {
    pub content: String,
    pub highlighted: bool,
}

impl SlashPaletteHighlightSegment {
    pub fn highlighted(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            highlighted: true,
        }
    }

    pub fn plain(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            highlighted: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SlashPaletteItem<'a> {
    pub command: &'a SlashCommandInfo,
    pub name_segments: Vec<SlashPaletteHighlightSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashPaletteUpdate {
    NoChange,
    Cleared,
    Changed {
        suggestions_changed: bool,
        selection_changed: bool,
    },
}

#[derive(Debug, Default)]
pub struct SlashPalette {
    suggestions: Vec<&'static SlashCommandInfo>,
    list_state: ListState,
    visible_rows: usize,
    filter_query: Option<String>,
}

impl SlashPalette {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn suggestions(&self) -> &[&'static SlashCommandInfo] {
        &self.suggestions
    }

    pub fn is_empty(&self) -> bool {
        self.suggestions.is_empty()
    }

    pub fn selected_command(&self) -> Option<&'static SlashCommandInfo> {
        self.list_state
            .selected()
            .and_then(|index| self.suggestions.get(index).copied())
    }

    pub fn list_state_mut(&mut self) -> &mut ListState {
        &mut self.list_state
    }

    pub fn clear_visible_rows(&mut self) {
        self.visible_rows = 0;
    }

    pub fn set_visible_rows(&mut self, rows: usize) {
        self.visible_rows = rows;
        self.ensure_list_visible();
    }

    #[cfg(test)]
    pub fn visible_rows(&self) -> usize {
        self.visible_rows
    }

    pub fn update(&mut self, prefix: Option<&str>, limit: usize) -> SlashPaletteUpdate {
        if prefix.is_none() {
            if self.clear_internal() {
                return SlashPaletteUpdate::Cleared;
            }
            return SlashPaletteUpdate::NoChange;
        }

        let prefix = prefix.unwrap();
        let normalized = normalize_query(prefix);
        let mut new_suggestions = suggestions_for(prefix);
        if !prefix.is_empty() {
            new_suggestions.truncate(limit);
        }

        let filter_query = if normalized.is_empty() {
            None
        } else if new_suggestions
            .iter()
            .all(|info| info.name.starts_with(&normalized))
        {
            Some(normalized.clone())
        } else {
            None
        };

        let suggestions_changed = self.replace_suggestions(new_suggestions);
        self.filter_query = filter_query;
        let selection_changed = self.ensure_selection();

        if suggestions_changed || selection_changed {
            SlashPaletteUpdate::Changed {
                suggestions_changed,
                selection_changed,
            }
        } else {
            SlashPaletteUpdate::NoChange
        }
    }

    pub fn clear(&mut self) -> bool {
        self.clear_internal()
    }

    pub fn move_up(&mut self) -> bool {
        if self.suggestions.is_empty() {
            return false;
        }

        let visible_len = self.suggestions.len();
        let current = self.list_state.selected().unwrap_or(0);
        let new_index = if current > 0 {
            current - 1
        } else {
            visible_len - 1
        };

        self.apply_selection(Some(new_index))
    }

    pub fn move_down(&mut self) -> bool {
        if self.suggestions.is_empty() {
            return false;
        }

        let visible_len = self.suggestions.len();
        let current = self.list_state.selected().unwrap_or(visible_len - 1);
        let new_index = if current + 1 < visible_len {
            current + 1
        } else {
            0
        };

        self.apply_selection(Some(new_index))
    }

    pub fn select_first(&mut self) -> bool {
        if self.suggestions.is_empty() {
            return false;
        }

        self.apply_selection(Some(0))
    }

    pub fn select_last(&mut self) -> bool {
        if self.suggestions.is_empty() {
            return false;
        }

        let last = self.suggestions.len() - 1;
        self.apply_selection(Some(last))
    }

    pub fn page_up(&mut self) -> bool {
        if self.suggestions.is_empty() {
            return false;
        }

        let step = self.visible_rows.max(1);
        let current = self.list_state.selected().unwrap_or(0);
        let new_index = current.saturating_sub(step);

        self.apply_selection(Some(new_index))
    }

    pub fn page_down(&mut self) -> bool {
        if self.suggestions.is_empty() {
            return false;
        }

        let step = self.visible_rows.max(1);
        let visible_len = self.suggestions.len();
        let current = self.list_state.selected().unwrap_or(0);
        let mut new_index = current.saturating_add(step);
        if new_index >= visible_len {
            new_index = visible_len - 1;
        }

        self.apply_selection(Some(new_index))
    }

    pub fn items(&self) -> Vec<SlashPaletteItem<'static>> {
        self.suggestions
            .iter()
            .map(|command| SlashPaletteItem {
                command,
                name_segments: self.highlight_name_segments(command.name),
            })
            .collect()
    }

    fn clear_internal(&mut self) -> bool {
        if self.suggestions.is_empty()
            && self.list_state.selected().is_none()
            && self.visible_rows == 0
            && self.filter_query.is_none()
        {
            return false;
        }

        self.suggestions.clear();
        self.list_state.select(None);
        *self.list_state.offset_mut() = 0;
        self.visible_rows = 0;
        self.filter_query = None;
        true
    }

    fn replace_suggestions(&mut self, new_suggestions: Vec<&'static SlashCommandInfo>) -> bool {
        if self.suggestions.len() == new_suggestions.len()
            && self
                .suggestions
                .iter()
                .zip(&new_suggestions)
                .all(|(current, candidate)| ptr::eq(*current, *candidate))
        {
            return false;
        }

        self.suggestions = new_suggestions;
        true
    }

    fn ensure_selection(&mut self) -> bool {
        if self.suggestions.is_empty() {
            if self.list_state.selected().is_some() {
                self.list_state.select(None);
                *self.list_state.offset_mut() = 0;
                return true;
            }
            return false;
        }

        let visible_len = self.suggestions.len();
        let current = self.list_state.selected().unwrap_or(0);
        let bounded = current.min(visible_len - 1);

        if Some(bounded) == self.list_state.selected() {
            self.ensure_list_visible();
            false
        } else {
            self.apply_selection(Some(bounded))
        }
    }

    fn apply_selection(&mut self, index: Option<usize>) -> bool {
        if self.list_state.selected() == index {
            return false;
        }

        self.list_state.select(index);
        if index.is_none() {
            *self.list_state.offset_mut() = 0;
        }
        self.ensure_list_visible();
        true
    }

    fn ensure_list_visible(&mut self) {
        if self.visible_rows == 0 {
            return;
        }

        let Some(selected) = self.list_state.selected() else {
            *self.list_state.offset_mut() = 0;
            return;
        };

        let visible_rows = self.visible_rows;
        let offset_ref = self.list_state.offset_mut();
        let offset = *offset_ref;

        if selected < offset {
            *offset_ref = selected;
        } else if selected >= offset + visible_rows {
            *offset_ref = selected + 1 - visible_rows;
        }
    }

    fn highlight_name_segments(&self, name: &str) -> Vec<SlashPaletteHighlightSegment> {
        let Some(query) = self.filter_query.as_ref().filter(|query| !query.is_empty()) else {
            return vec![SlashPaletteHighlightSegment::plain(name.to_string())];
        };

        let lowercase = name.to_ascii_lowercase();
        if !lowercase.starts_with(query) {
            return vec![SlashPaletteHighlightSegment::plain(name.to_string())];
        }

        let query_len = query.chars().count();
        let mut highlighted = String::new();
        let mut remainder = String::new();

        for (index, ch) in name.chars().enumerate() {
            if index < query_len {
                highlighted.push(ch);
            } else {
                remainder.push(ch);
            }
        }

        let mut segments = Vec::new();
        if !highlighted.is_empty() {
            segments.push(SlashPaletteHighlightSegment::highlighted(highlighted));
        }
        if !remainder.is_empty() {
            segments.push(SlashPaletteHighlightSegment::plain(remainder));
        }
        if segments.is_empty() {
            segments.push(SlashPaletteHighlightSegment::plain(String::new()));
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette_with_commands() -> SlashPalette {
        let mut palette = SlashPalette::new();
        let _ = palette.update(Some(""), usize::MAX);
        palette
    }

    #[test]
    fn update_applies_prefix_and_highlights_matches() {
        let mut palette = SlashPalette::new();

        let update = palette.update(Some("co"), 10);
        assert!(matches!(
            update,
            SlashPaletteUpdate::Changed {
                suggestions_changed: true,
                selection_changed: true
            }
        ));

        let items = palette.items();
        assert!(!items.is_empty());
        let command = items
            .into_iter()
            .find(|item| item.command.name == "command")
            .expect("command suggestion available");

        assert_eq!(command.name_segments.len(), 2);
        assert!(command.name_segments[0].highlighted);
        assert_eq!(command.name_segments[0].content, "co");
        assert_eq!(command.name_segments[1].content, "mmand");
    }

    #[test]
    fn update_without_matches_resets_highlights() {
        let mut palette = SlashPalette::new();
        let _ = palette.update(Some("co"), 10);
        assert!(!palette.items().is_empty());

        let update = palette.update(Some("zzz"), 10);
        assert!(matches!(update, SlashPaletteUpdate::Changed { .. }));

        for item in palette.items() {
            assert!(
                item.name_segments
                    .iter()
                    .all(|segment| !segment.highlighted)
            );
        }
    }

    #[test]
    fn navigation_wraps_between_items() {
        let mut palette = palette_with_commands();

        assert!(palette.move_down());
        let first = palette.list_state.selected();
        assert_eq!(first, Some(1));

        let mut moved = false;
        for _ in 0..palette.suggestions.len() {
            moved = palette.move_down();
        }
        assert!(moved);
        assert_eq!(palette.list_state.selected(), Some(0));

        assert!(palette.move_up());
        assert_eq!(
            palette.list_state.selected(),
            Some(palette.suggestions.len() - 1)
        );
    }

    #[test]
    fn boundary_shortcuts_jump_to_expected_items() {
        let mut palette = palette_with_commands();

        assert!(palette.select_last());
        assert_eq!(
            palette.list_state.selected(),
            Some(palette.suggestions.len() - 1)
        );

        assert!(palette.select_first());
        assert_eq!(palette.list_state.selected(), Some(0));
    }

    #[test]
    fn page_navigation_advances_by_visible_rows() {
        let mut palette = palette_with_commands();
        palette.set_visible_rows(3);

        assert!(palette.page_down());
        assert_eq!(palette.list_state.selected(), Some(3));

        assert!(palette.page_down());
        assert_eq!(palette.list_state.selected(), Some(6));

        assert!(palette.page_up());
        assert_eq!(palette.list_state.selected(), Some(3));

        assert!(palette.page_up());
        assert_eq!(palette.list_state.selected(), Some(0));
    }

    #[test]
    fn clear_resets_state() {
        let mut palette = SlashPalette::new();
        let _ = palette.update(Some("co"), 10);
        palette.set_visible_rows(3);

        assert!(palette.clear());
        assert!(palette.suggestions().is_empty());
        assert_eq!(palette.list_state.selected(), None);
        assert_eq!(palette.visible_rows(), 0);
    }

    #[test]
    fn command_range_tracks_latest_slash_before_cursor() {
        let input = "/one two /three";
        let cursor = input.len();
        let range = command_range(input, cursor).expect("range available");
        assert_eq!(range.start, 9);
        assert_eq!(range.end, input.len());
    }

    #[test]
    fn command_range_stops_at_whitespace() {
        let input = "/cmd arg";
        let cursor = input.len();
        let range = command_range(input, cursor).expect("range available");
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 4);
    }

    #[test]
    fn command_prefix_includes_partial_match() {
        let input = "/hel";
        let prefix = command_prefix(input, input.len()).expect("prefix available");
        assert_eq!(prefix, "hel");
    }

    #[test]
    fn command_prefix_is_empty_when_cursor_immediately_after_slash() {
        let input = "/";
        let prefix = command_prefix(input, 1).expect("prefix available");
        assert!(prefix.is_empty());
    }

    #[test]
    fn command_prefix_returns_none_when_not_in_command() {
        let input = "say hello";
        assert!(command_prefix(input, input.len()).is_none());
    }
}
