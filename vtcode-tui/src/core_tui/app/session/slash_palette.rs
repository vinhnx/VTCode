use unicode_segmentation::UnicodeSegmentation;

use crate::core_tui::app::types::SlashCommandItem;
use crate::core_tui::session::list_navigator::ListNavigator;
use crate::ui::search::{fuzzy_score, normalize_query};

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
    navigator: ListNavigator,
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
            navigator: ListNavigator::new(),
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
        self.navigator
            .selected()
            .and_then(|index| self.suggestions.get(index))
            .map(|suggestion| match suggestion {
                SlashPaletteSuggestion::Static(info) => info,
            })
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.navigator.selected()
    }

    pub fn scroll_offset(&self) -> usize {
        self.navigator.scroll_offset()
    }

    pub(super) fn set_selected(&mut self, selected: Option<usize>) {
        self.navigator.set_selected(selected);
    }

    pub(super) fn set_scroll_offset(&mut self, offset: usize) {
        self.navigator.set_scroll_offset(offset);
    }

    pub fn clear_visible_rows(&mut self) {
        self.navigator.set_visible_rows(0);
    }

    pub fn set_visible_rows(&mut self, rows: usize) {
        self.navigator.set_visible_rows(rows);
    }

    #[cfg(test)]
    pub fn visible_rows(&self) -> usize {
        self.navigator.visible_rows()
    }

    pub fn update(&mut self, prefix: Option<&str>, limit: usize) -> SlashPaletteUpdate {
        let Some(prefix) = prefix else {
            if self.clear_internal() {
                return SlashPaletteUpdate::Cleared;
            }
            return SlashPaletteUpdate::NoChange;
        };
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
        self.navigator.move_up()
    }

    pub fn move_down(&mut self) -> bool {
        self.navigator.move_down()
    }

    pub fn select_first(&mut self) -> bool {
        self.navigator.select_first()
    }

    pub fn select_last(&mut self) -> bool {
        self.navigator.select_last()
    }

    pub fn page_up(&mut self) -> bool {
        let step = self.navigator.visible_rows().max(1);
        self.navigator.page_up(step)
    }

    pub fn page_down(&mut self) -> bool {
        let step = self.navigator.visible_rows().max(1);
        self.navigator.page_down(step)
    }

    pub fn select_index(&mut self, index: usize) -> bool {
        self.navigator.select_index(index)
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
            && self.navigator.selected().is_none()
            && self.navigator.visible_rows() == 0
            && self.filter_query.is_none()
        {
            return false;
        }

        self.suggestions.clear();
        self.navigator.set_item_count(0);
        self.navigator.set_visible_rows(0);
        self.filter_query = None;
        true
    }

    fn replace_suggestions(&mut self, new_suggestions: Vec<SlashPaletteSuggestion>) -> bool {
        if self.suggestions == new_suggestions {
            return false;
        }

        self.suggestions = new_suggestions;
        self.navigator.set_item_count(self.suggestions.len());
        true
    }

    fn suggestions_for(&self, prefix: &str) -> Vec<SlashCommandItem> {
        struct ScoredCommand<'a> {
            command: &'a SlashCommandItem,
            name_match: bool,
            name_prefix: bool,
            name_pos: usize,
            description_pos: usize,
            name_score: u32,
            description_score: u32,
        }

        let normalized_query = normalize_query(prefix);
        if normalized_query.is_empty() {
            return self.commands.clone();
        }

        let mut prefix_matches: Vec<&SlashCommandItem> = self
            .commands
            .iter()
            .filter(|info| info.name.starts_with(normalized_query.as_str()))
            .collect();
        if !prefix_matches.is_empty() {
            prefix_matches.sort_by(|a, b| a.name.cmp(&b.name));
            return prefix_matches.into_iter().cloned().collect();
        }

        let mut scored: Vec<ScoredCommand<'_>> = self
            .commands
            .iter()
            .filter_map(|info| {
                let name_score = fuzzy_score(&normalized_query, info.name.as_str());
                let description_score = fuzzy_score(&normalized_query, info.description.as_str());
                if name_score.is_none() && description_score.is_none() {
                    return None;
                }

                let name_lower = info.name.to_ascii_lowercase();
                let description_lower = info.description.to_ascii_lowercase();

                Some(ScoredCommand {
                    command: info,
                    name_match: name_score.is_some(),
                    name_prefix: name_lower.starts_with(normalized_query.as_str()),
                    name_pos: name_lower
                        .find(normalized_query.as_str())
                        .unwrap_or(usize::MAX),
                    description_pos: description_lower
                        .find(normalized_query.as_str())
                        .unwrap_or(usize::MAX),
                    name_score: name_score.unwrap_or(0),
                    description_score: description_score.unwrap_or(0),
                })
            })
            .collect();

        if scored.is_empty() {
            return Vec::new();
        }

        scored.sort_by(|left, right| {
            (
                !left.name_match,
                !left.name_prefix,
                left.name_pos == usize::MAX,
                std::cmp::Reverse(left.name_score),
                left.name_pos,
                left.description_pos == usize::MAX,
                std::cmp::Reverse(left.description_score),
                left.description_pos,
                left.command.name.len(),
                left.command.name.as_str(),
            )
                .cmp(&(
                    !right.name_match,
                    !right.name_prefix,
                    right.name_pos == usize::MAX,
                    std::cmp::Reverse(right.name_score),
                    right.name_pos,
                    right.description_pos == usize::MAX,
                    std::cmp::Reverse(right.description_score),
                    right.description_pos,
                    right.command.name.len(),
                    right.command.name.as_str(),
                ))
        });

        scored
            .into_iter()
            .map(|info| info.command.clone())
            .collect()
    }

    fn ensure_selection(&mut self) -> bool {
        if self.suggestions.is_empty() {
            if self.navigator.selected().is_some() {
                self.navigator.set_item_count(0);
                return true;
            }
            return false;
        }

        let previous = self.navigator.selected();
        self.navigator.set_item_count(self.suggestions.len());

        if self.navigator.selected().is_none() {
            return self.navigator.select_first();
        }

        self.navigator.selected() != previous
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
                    .is_some_and(|cmd| cmd.name == "command")
            })
            .expect("command suggestion available");

        assert_eq!(command.name_segments.len(), 2);
        assert!(command.name_segments[0].highlighted);
        assert_eq!(command.name_segments[0].content, "co");
        assert_eq!(command.name_segments[1].content, "mmand");
    }

    #[test]
    fn update_matches_fuzzy_command_name() {
        let mut palette = SlashPalette::with_commands(test_commands());

        let update = palette.update(Some("sts"), 10);
        assert!(matches!(update, SlashPaletteUpdate::Changed { .. }));

        let names: Vec<String> = palette
            .items()
            .into_iter()
            .filter_map(|item| item.command.map(|command| command.name))
            .collect();

        assert_eq!(names.first().map(String::as_str), Some("status"));
    }

    #[test]
    fn update_matches_command_description() {
        let mut palette = SlashPalette::with_commands(test_commands());

        let update = palette.update(Some("terminal"), 10);
        assert!(matches!(update, SlashPaletteUpdate::Changed { .. }));

        let names: Vec<String> = palette
            .items()
            .into_iter()
            .filter_map(|item| item.command.map(|command| command.name))
            .collect();

        assert_eq!(names.first().map(String::as_str), Some("command"));
    }

    #[test]
    fn update_without_matches_resets_highlights() {
        let mut palette = SlashPalette::with_commands(test_commands());
        let _ = palette.update(Some("co"), 10);
        assert!(!palette.items().is_empty());

        let update = palette.update(Some("zzz"), 10);
        assert!(matches!(update, SlashPaletteUpdate::Changed { .. }));
        assert!(palette.items().is_empty());

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
        let first = palette.selected_index();
        assert_eq!(first, Some(1));

        let steps = palette.suggestions.len().saturating_sub(1);
        for _ in 0..steps {
            assert!(palette.move_down());
        }
        assert_eq!(palette.selected_index(), Some(0));

        assert!(palette.move_up());
        assert_eq!(
            palette.selected_index(),
            Some(palette.suggestions.len() - 1)
        );
    }

    #[test]
    fn boundary_shortcuts_jump_to_expected_items() {
        let mut palette = palette_with_commands();

        assert!(palette.select_last());
        assert_eq!(
            palette.selected_index(),
            Some(palette.suggestions.len() - 1)
        );

        assert!(palette.select_first());
        assert_eq!(palette.selected_index(), Some(0));
    }

    #[test]
    fn page_navigation_advances_by_visible_rows() {
        let mut palette = palette_with_commands();
        palette.set_visible_rows(3);

        assert!(palette.page_down());
        assert_eq!(palette.selected_index(), Some(3));

        assert!(palette.page_down());
        assert_eq!(palette.selected_index(), Some(6));

        assert!(palette.page_up());
        assert_eq!(palette.selected_index(), Some(3));

        assert!(palette.page_up());
        assert_eq!(palette.selected_index(), Some(0));
    }

    #[test]
    fn clear_resets_state() {
        let mut palette = SlashPalette::with_commands(test_commands());
        let _ = palette.update(Some("co"), 10);
        palette.set_visible_rows(3);

        assert!(palette.clear());
        assert!(palette.suggestions().is_empty());
        assert_eq!(palette.selected_index(), None);
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
