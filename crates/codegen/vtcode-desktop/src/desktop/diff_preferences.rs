//! Persisted diff viewer presentation preferences.
//!
//! Every field is optional so a missing value keeps existing installs on the
//! current rendering: the readers below default to what the Workspace Review
//! and expanded Inspector Changes surfaces render today. One deliberate
//! exception: the compact Inspector Changes pane never wrapped before, and
//! now follows the global Soft wrap toggle (on by default).
//!
//! Kit enums stay out of the serialized form (several are
//! `#[non_exhaustive]`); the file stores stable id strings and the readers
//! map them back with default fallbacks.

use gpui_kit::component::diff::{
    Diff, DiffChangeIndicator, DiffFile, DiffHoverHighlight, DiffHunkSeparator, DiffInlineUnit, DiffMode, DiffState,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct DiffPreferences {
    /// Diff layout id (`unified`/`split`). Unknown values fall back to Unified.
    pub mode: Option<String>,
    /// Wrap long diff lines instead of scrolling sideways.
    pub soft_wrap: Option<bool>,
    /// Show source line numbers beside diff rows.
    pub line_numbers: Option<bool>,
    /// Color diff code by language.
    pub syntax_highlight: Option<bool>,
    /// Show the file path and counts above each file.
    pub file_headers: Option<bool>,
    /// Tint added and removed rows.
    pub change_background: Option<bool>,
    /// Change marker id (`signs`/`bars`/`none`).
    pub change_indicator: Option<String>,
    /// Hunk separator id (`metadata`/`lineInfo`/`simple`).
    pub hunk_separator: Option<String>,
    /// Hover highlight id (`none`/`line`/`lineNumber`/`both`).
    pub hover_highlight: Option<String>,
    /// Context depth id (`3`/`5`/`10`/`unfolded`).
    pub context_lines: Option<String>,
    /// Inline emphasis id (`words`/`characters`/`lines`).
    pub inline_unit: Option<String>,
}

/// Case-insensitive id comparison for hand-edited preference files.
fn matches_str(value: Option<&str>, id: &str) -> bool {
    value.map(str::trim).is_some_and(|saved| saved.eq_ignore_ascii_case(id))
}

fn matches(saved: &Option<String>, id: &str) -> bool {
    matches_str(saved.as_deref(), id)
}

/// Parse a saved layout id. Missing or unknown values fall back to Unified
/// so a corrupt preference never breaks the diff viewers.
pub(super) fn mode_from_saved(value: Option<&str>) -> DiffMode {
    if matches_str(value, "split") {
        DiffMode::Split
    } else {
        DiffMode::Unified
    }
}

impl DiffPreferences {
    pub(super) fn mode(&self) -> DiffMode {
        mode_from_saved(self.mode.as_deref())
    }

    pub(super) fn set_mode(&mut self, mode: DiffMode) {
        // `DiffMode` is non-exhaustive: compare instead of matching so a
        // future kit variant degrades to the default layout.
        let id = if mode == DiffMode::Split { "split" } else { "unified" };
        self.mode = Some(id.to_owned());
    }

    pub(super) fn soft_wrap(&self) -> bool {
        self.soft_wrap.unwrap_or(true)
    }

    pub(super) fn set_soft_wrap(&mut self, enabled: bool) {
        self.soft_wrap = Some(enabled);
    }

    pub(super) fn line_numbers(&self) -> bool {
        self.line_numbers.unwrap_or(true)
    }

    pub(super) fn set_line_numbers(&mut self, enabled: bool) {
        self.line_numbers = Some(enabled);
    }

    pub(super) fn syntax_highlight(&self) -> bool {
        self.syntax_highlight.unwrap_or(true)
    }

    pub(super) fn set_syntax_highlight(&mut self, enabled: bool) {
        self.syntax_highlight = Some(enabled);
    }

    pub(super) fn file_headers(&self) -> bool {
        self.file_headers.unwrap_or(true)
    }

    pub(super) fn set_file_headers(&mut self, enabled: bool) {
        self.file_headers = Some(enabled);
    }

    pub(super) fn change_background(&self) -> bool {
        self.change_background.unwrap_or(true)
    }

    pub(super) fn set_change_background(&mut self, enabled: bool) {
        self.change_background = Some(enabled);
    }

    pub(super) fn change_indicator(&self) -> DiffChangeIndicator {
        if matches(&self.change_indicator, "bars") {
            DiffChangeIndicator::Bars
        } else if matches(&self.change_indicator, "none") {
            DiffChangeIndicator::None
        } else {
            DiffChangeIndicator::Signs
        }
    }

    pub(super) fn set_change_indicator(&mut self, indicator: DiffChangeIndicator) {
        // `DiffChangeIndicator` is non-exhaustive: compare instead of matching
        // so a future kit variant degrades to the default marker.
        let id = if indicator == DiffChangeIndicator::Bars {
            "bars"
        } else if indicator == DiffChangeIndicator::None {
            "none"
        } else {
            "signs"
        };
        self.change_indicator = Some(id.to_owned());
    }

    pub(super) fn hunk_separator(&self) -> DiffHunkSeparator {
        if matches(&self.hunk_separator, "lineinfo") {
            DiffHunkSeparator::LineInfo
        } else if matches(&self.hunk_separator, "simple") {
            DiffHunkSeparator::Simple
        } else {
            DiffHunkSeparator::Metadata
        }
    }

    pub(super) fn set_hunk_separator(&mut self, separator: DiffHunkSeparator) {
        let id = if separator == DiffHunkSeparator::LineInfo {
            "lineInfo"
        } else if separator == DiffHunkSeparator::Simple {
            "simple"
        } else {
            "metadata"
        };
        self.hunk_separator = Some(id.to_owned());
    }

    pub(super) fn hover_highlight(&self) -> DiffHoverHighlight {
        if matches(&self.hover_highlight, "line") {
            DiffHoverHighlight::Line
        } else if matches(&self.hover_highlight, "linenumber") {
            DiffHoverHighlight::LineNumber
        } else if matches(&self.hover_highlight, "both") {
            DiffHoverHighlight::Both
        } else {
            DiffHoverHighlight::None
        }
    }

    pub(super) fn set_hover_highlight(&mut self, highlight: DiffHoverHighlight) {
        // `DiffHoverHighlight` is non-exhaustive: compare instead of matching
        // so a future kit variant degrades to the default highlight.
        let id = if highlight == DiffHoverHighlight::Line {
            "line"
        } else if highlight == DiffHoverHighlight::LineNumber {
            "lineNumber"
        } else if highlight == DiffHoverHighlight::Both {
            "both"
        } else {
            "none"
        };
        self.hover_highlight = Some(id.to_owned());
    }

    /// Surrounding context lines, or `None` to show every supplied patch line.
    pub(super) fn context_lines(&self) -> Option<usize> {
        if matches(&self.context_lines, "5") {
            Some(5)
        } else if matches(&self.context_lines, "10") {
            Some(10)
        } else if matches(&self.context_lines, "unfolded") {
            None
        } else {
            Some(3)
        }
    }

    pub(super) fn set_context_lines(&mut self, lines: Option<usize>) {
        let id = match lines {
            Some(5) => "5",
            Some(10) => "10",
            None => "unfolded",
            _ => "3",
        };
        self.context_lines = Some(id.to_owned());
    }

    /// Inline comparison unit, or `None` to emphasize whole lines only.
    pub(super) fn inline_unit(&self) -> Option<DiffInlineUnit> {
        if matches(&self.inline_unit, "characters") {
            Some(DiffInlineUnit::Character)
        } else if matches(&self.inline_unit, "lines") {
            None
        } else {
            Some(DiffInlineUnit::Word)
        }
    }

    pub(super) fn set_inline_unit(&mut self, unit: Option<DiffInlineUnit>) {
        let id = match unit {
            Some(DiffInlineUnit::Character) => "characters",
            None => "lines",
            _ => "words",
        };
        self.inline_unit = Some(id.to_owned());
    }
}

/// Build a diff viewer state honoring the persisted presentation choices.
/// The layout stays caller-owned: each surface keeps its own mode while the
/// remaining options are global.
pub(super) fn new_diff_state(
    files: Vec<DiffFile>,
    mode: DiffMode,
    prefs: &DiffPreferences,
    cx: &mut gpui_kit::Context<DiffState>,
) -> DiffState {
    DiffState::new(files, cx)
        .with_mode(mode)
        .with_context_lines(prefs.context_lines())
        .with_inline_unit(prefs.inline_unit())
}

/// Apply the persisted appearance choices to a diff element under
/// construction. Line wrapping stays caller-owned: the Inspector forces it
/// while expanded, so the caller passes the effective value.
pub(super) fn apply_diff_appearance(diff: Diff, prefs: &DiffPreferences, soft_wrap: bool) -> Diff {
    diff.line_number(prefs.line_numbers())
        .syntax_highlight(prefs.syntax_highlight())
        .header_visible(prefs.file_headers())
        .hunk_separator(prefs.hunk_separator())
        .change_indicator(prefs.change_indicator())
        .change_background(prefs.change_background())
        .hover_highlight(prefs.hover_highlight())
        .soft_wrap(soft_wrap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_reproduce_the_current_rendering() {
        let prefs = DiffPreferences::default();
        assert_eq!(prefs.mode(), DiffMode::Unified);
        assert!(prefs.soft_wrap());
        assert!(prefs.line_numbers());
        assert!(prefs.syntax_highlight());
        assert!(prefs.file_headers());
        assert!(prefs.change_background());
        assert_eq!(prefs.change_indicator(), DiffChangeIndicator::Signs);
        assert_eq!(prefs.hunk_separator(), DiffHunkSeparator::Metadata);
        assert_eq!(prefs.hover_highlight(), DiffHoverHighlight::None);
        assert_eq!(prefs.context_lines(), Some(3));
        assert_eq!(prefs.inline_unit(), Some(DiffInlineUnit::Word));
    }

    #[test]
    fn every_option_round_trips_through_json() {
        let mut prefs = DiffPreferences::default();
        prefs.set_mode(DiffMode::Split);
        prefs.set_soft_wrap(false);
        prefs.set_line_numbers(false);
        prefs.set_syntax_highlight(false);
        prefs.set_file_headers(false);
        prefs.set_change_background(false);
        prefs.set_change_indicator(DiffChangeIndicator::Bars);
        prefs.set_hunk_separator(DiffHunkSeparator::Simple);
        prefs.set_hover_highlight(DiffHoverHighlight::Both);
        prefs.set_context_lines(None);
        prefs.set_inline_unit(Some(DiffInlineUnit::Character));
        let serialized = serde_json::to_string(&prefs).unwrap();
        let loaded: DiffPreferences = serde_json::from_str(&serialized).unwrap();
        assert_eq!(loaded.mode(), DiffMode::Split);
        assert!(!loaded.soft_wrap());
        assert!(!loaded.line_numbers());
        assert!(!loaded.syntax_highlight());
        assert!(!loaded.file_headers());
        assert!(!loaded.change_background());
        assert_eq!(loaded.change_indicator(), DiffChangeIndicator::Bars);
        assert_eq!(loaded.hunk_separator(), DiffHunkSeparator::Simple);
        assert_eq!(loaded.hover_highlight(), DiffHoverHighlight::Both);
        assert_eq!(loaded.context_lines(), None);
        assert_eq!(loaded.inline_unit(), Some(DiffInlineUnit::Character));

        let raw: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(raw.get("mode").and_then(serde_json::Value::as_str), Some("split"));
        assert_eq!(raw.get("softWrap").and_then(serde_json::Value::as_bool), Some(false));
        assert_eq!(raw.get("changeIndicator").and_then(serde_json::Value::as_str), Some("bars"));
        assert_eq!(raw.get("contextLines").and_then(serde_json::Value::as_str), Some("unfolded"));
    }

    #[test]
    fn unknown_saved_ids_fall_back_to_defaults() {
        for saved in ["", "  ", "sideways"] {
            let prefs = DiffPreferences {
                mode: Some(saved.to_owned()),
                change_indicator: Some(saved.to_owned()),
                hunk_separator: Some(saved.to_owned()),
                hover_highlight: Some(saved.to_owned()),
                context_lines: Some(saved.to_owned()),
                inline_unit: Some(saved.to_owned()),
                ..DiffPreferences::default()
            };
            assert_eq!(prefs.mode(), DiffMode::Unified, "{saved:?}");
            assert_eq!(prefs.change_indicator(), DiffChangeIndicator::Signs, "{saved:?}");
            assert_eq!(prefs.hunk_separator(), DiffHunkSeparator::Metadata, "{saved:?}");
            assert_eq!(prefs.hover_highlight(), DiffHoverHighlight::None, "{saved:?}");
            assert_eq!(prefs.context_lines(), Some(3), "{saved:?}");
            assert_eq!(prefs.inline_unit(), Some(DiffInlineUnit::Word), "{saved:?}");
        }
    }

    #[test]
    fn saved_ids_ignore_case_and_surrounding_whitespace() {
        let prefs = DiffPreferences {
            mode: Some("  SPLIT ".to_owned()),
            change_indicator: Some("Bars".to_owned()),
            hunk_separator: Some("LINEINFO".to_owned()),
            hover_highlight: Some(" LineNumber ".to_owned()),
            context_lines: Some("10".to_owned()),
            inline_unit: Some("CHARACTERS".to_owned()),
            ..DiffPreferences::default()
        };
        assert_eq!(prefs.mode(), DiffMode::Split);
        assert_eq!(prefs.change_indicator(), DiffChangeIndicator::Bars);
        assert_eq!(prefs.hunk_separator(), DiffHunkSeparator::LineInfo);
        assert_eq!(prefs.hover_highlight(), DiffHoverHighlight::LineNumber);
        assert_eq!(prefs.context_lines(), Some(10));
        assert_eq!(prefs.inline_unit(), Some(DiffInlineUnit::Character));
    }

    #[test]
    fn setters_map_values_to_stable_ids() {
        let mut prefs = DiffPreferences::default();
        prefs.set_change_indicator(DiffChangeIndicator::None);
        prefs.set_hunk_separator(DiffHunkSeparator::LineInfo);
        prefs.set_hover_highlight(DiffHoverHighlight::Line);
        prefs.set_context_lines(Some(5));
        prefs.set_inline_unit(None);
        assert_eq!(prefs.change_indicator.as_deref(), Some("none"));
        assert_eq!(prefs.hunk_separator.as_deref(), Some("lineInfo"));
        assert_eq!(prefs.hover_highlight.as_deref(), Some("line"));
        assert_eq!(prefs.context_lines.as_deref(), Some("5"));
        assert_eq!(prefs.inline_unit.as_deref(), Some("lines"));

        // Values outside the offered presets degrade to the default id.
        prefs.set_context_lines(Some(7));
        assert_eq!(prefs.context_lines(), Some(3));
    }
}
