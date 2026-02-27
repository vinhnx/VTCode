use ratatui::widgets::ListState;
use unicode_segmentation::UnicodeSegmentation;

use crate::ui::search::{fuzzy_match, normalize_query};
use crate::ui::tui::types::SlashCommandItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandRange {
    pub start: usize,
    pub end: usize,
}

pub fn command_range(input: &str, cursor: usize) -> Option<SlashCommandRange> {
    if !input.starts_with('/') {
        return None;
    }

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
            // Space terminates the current command token
            active_range = None;
        } else if let Some(range) = &mut active_range {
            range.end = index + grapheme.len();
        }
    }

    active_range.filter(|range| range.end > range.start)
}

pub fn command_prefix(input: &str, cursor: usize) -> Option<String> {
    let range = command_range(input, cursor)?;
    let end = cursor.min(range.end);
    let start = range.start + 1;
    if end < start {
        return Some(String::new());
    }
    Some(input[start..end].to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(test)]
pub struct SlashPaletteHighlightSegment {
    pub content: String,
    pub highlighted: bool,
}

#[cfg(test)]
impl SlashPaletteHighlightSegment {
    #[cfg(test)]
    pub fn highlighted(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            highlighted: true,
        }
    }

    #[cfg(test)]
    pub fn plain(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            highlighted: false,
        }
    }
}

#[derive(Debug, Clone)]
#[cfg(test)]
pub struct SlashPaletteItem {
    #[allow(dead_code)]
    pub command: Option<SlashCommandItem>,
    pub name_segments: Vec<SlashPaletteHighlightSegment>,
    #[allow(dead_code)]
    pub description: String,
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
    commands: Vec<SlashCommandItem>,
    suggestions: Vec<SlashPaletteSuggestion>,
    list_state: ListState,
    visible_rows: usize,
    filter_query: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlashPaletteSuggestion {
    Static(SlashCommandItem),
}

impl SlashPalette {
    pub fn new() -> Self {
        Self::with_commands(Vec::new())
    }

    pub fn with_commands(commands: Vec<SlashCommandItem>) -> Self {
        Self {
            commands,
            suggestions: Vec::new(),
            list_state: ListState::default(),
            visible_rows: 0,
            filter_query: None,
        }
    }

    pub fn suggestions(&self) -> &[SlashPaletteSuggestion] {
        &self.suggestions
    }

    pub fn is_empty(&self) -> bool {
        self.suggestions.is_empty()
    }

    pub fn selected_command(&self) -> Option<&SlashCommandItem> {
        self.list_state
            .selected()
            .and_then(|index| self.suggestions.get(index))
            .map(|suggestion| match suggestion {
                SlashPaletteSuggestion::Static(info) => info,
            })
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
        let mut new_suggestions = Vec::new();

        // Handle regular slash commands
        let static_suggestions = self.suggestions_for(prefix);
        new_suggestions.extend(
            static_suggestions
                .into_iter()
                .map(SlashPaletteSuggestion::Static),
        );

        // Apply limit if prefix is not empty
        if !prefix.is_empty() {
            new_suggestions.truncate(limit);
        }

        let filter_query = {
            let normalized = normalize_query(prefix);
            if normalized.is_empty() {
                None
            } else if new_suggestions
                .iter()
                .map(|suggestion| match suggestion {
                    SlashPaletteSuggestion::Static(info) => info,
                })
                .all(|info| info.name.starts_with(normalized.as_str()))
            {
                Some(normalized.clone())
            } else {
                None
            }
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

    #[cfg(test)]
    pub fn items(&self) -> Vec<SlashPaletteItem> {
        self.suggestions
            .iter()
            .map(|suggestion| match suggestion {
                SlashPaletteSuggestion::Static(command) => SlashPaletteItem {
                    command: Some(command.clone()),
                    name_segments: self.highlight_name_segments_static(command.name.as_str()),
                    description: command.description.to_owned(),
                },
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

    fn replace_suggestions(&mut self, new_suggestions: Vec<SlashPaletteSuggestion>) -> bool {
        if self.suggestions == new_suggestions {
            return false;
        }

        self.suggestions = new_suggestions;
        true
    }

    fn suggestions_for(&self, prefix: &str) -> Vec<SlashCommandItem> {
        let trimmed = prefix.trim();
        if trimmed.is_empty() {
            return self.commands.clone();
        }

        let query = trimmed.to_ascii_lowercase();

        let mut prefix_matches: Vec<&SlashCommandItem> = self
            .commands
            .iter()
            .filter(|info| info.name.starts_with(query.as_str()))
            .collect();

        if !prefix_matches.is_empty() {
            prefix_matches.sort_by(|a, b| a.name.cmp(&b.name));
            return prefix_matches.into_iter().cloned().collect();
        }

        let mut substring_matches: Vec<(&SlashCommandItem, usize)> = self
            .commands
            .iter()
            .filter_map(|info| {
                info.name
                    .find(query.as_str())
                    .map(|position| (info, position))
            })
            .collect();

        if !substring_matches.is_empty() {
            substring_matches.sort_by(|(a, pos_a), (b, pos_b)| {
                (*pos_a, a.name.len(), a.name.as_str()).cmp(&(
                    *pos_b,
                    b.name.len(),
                    b.name.as_str(),
                ))
            });
            return substring_matches
                .into_iter()
                .map(|(info, _)| info.clone())
                .collect();
        }

        let normalized_query = normalize_query(&query);
        if normalized_query.is_empty() {
            return self.commands.clone();
        }

        let mut scored: Vec<(&SlashCommandItem, usize, usize)> = self
            .commands
            .iter()
            .filter_map(|info| {
                let mut candidate = info.name.to_ascii_lowercase();
                if !info.description.is_empty() {
                    candidate.push(' ');
                    candidate.push_str(info.description.to_ascii_lowercase().as_str());
                }

                if !fuzzy_match(&normalized_query, &candidate) {
                    return None;
                }

                let name_pos = info
                    .name
                    .to_ascii_lowercase()
                    .find(query.as_str())
                    .unwrap_or(usize::MAX);
                let desc_pos = info
                    .description
                    .to_ascii_lowercase()
                    .find(query.as_str())
                    .unwrap_or(usize::MAX);

                Some((info, name_pos, desc_pos))
            })
            .collect();

        if scored.is_empty() {
            return self.commands.clone();
        }

        scored.sort_by(|(a, name_pos_a, desc_pos_a), (b, name_pos_b, desc_pos_b)| {
            let score_a = (
                *name_pos_a == usize::MAX,
                *name_pos_a,
                *desc_pos_a,
                a.name.as_str(),
            );
            let score_b = (
                *name_pos_b == usize::MAX,
                *name_pos_b,
                *desc_pos_b,
                b.name.as_str(),
            );
            score_a.cmp(&score_b)
        });

        scored
            .into_iter()
            .map(|(info, _, _)| info.clone())
            .collect()
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

    #[cfg(test)]
    fn highlight_name_segments_static(&self, name: &str) -> Vec<SlashPaletteHighlightSegment> {
        let Some(query) = self.filter_query.as_ref().filter(|query| !query.is_empty()) else {
            return vec![SlashPaletteHighlightSegment::plain(name.to_owned())];
        };

        // For static commands, only use the part after the prompt invocation prefix if applicable
        let lowercase = name.to_ascii_lowercase();
        if !lowercase.starts_with(query) {
            return vec![SlashPaletteHighlightSegment::plain(name.to_owned())];
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

    fn test_commands() -> Vec<SlashCommandItem> {
        vec![
            SlashCommandItem::new("command", "Run a terminal command"),
            SlashCommandItem::new("config", "Show effective configuration"),
            SlashCommandItem::new("clear", "Clear screen"),
            SlashCommandItem::new("new", "Start new session"),
            SlashCommandItem::new("status", "Show status"),
            SlashCommandItem::new("help", "Show help"),
            SlashCommandItem::new("theme", "Switch theme"),
            SlashCommandItem::new("mode", "Switch mode"),
        ]
    }

    fn palette_with_commands() -> SlashPalette {
        let mut palette = SlashPalette::with_commands(test_commands());
        let _ = palette.update(Some(""), usize::MAX);
        palette
    }

    #[test]
    fn update_applies_prefix_and_highlights_matches() {
        let mut palette = SlashPalette::with_commands(test_commands());

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
            .find(|item| {
                item.command
                    .as_ref()
                    .map_or(false, |cmd| cmd.name == "command")
            })
            .expect("command suggestion available");

        assert_eq!(command.name_segments.len(), 2);
        assert!(command.name_segments[0].highlighted);
        assert_eq!(command.name_segments[0].content, "co");
        assert_eq!(command.name_segments[1].content, "mmand");
    }

    #[test]
    fn update_without_matches_resets_highlights() {
        let mut palette = SlashPalette::with_commands(test_commands());
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

        let steps = palette.suggestions.len().saturating_sub(1);
        for _ in 0..steps {
            assert!(palette.move_down());
        }
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
        let mut palette = SlashPalette::with_commands(test_commands());
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
        // Previous behavior: returned Some(0..4) (last range)
        // New behavior: returns None (active range interrupted by space)
        assert!(command_range(input, cursor).is_none());
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
