//! Shared markdown rendering types.
//!
//! The actual rendering implementation lives in `vtcode-ui`.  These structs
//! define the output format so that `vtcode-core` can work with rendered
//! markdown lines regardless of whether the TUI crate is compiled in.

use anstyle::{Color, Style};

/// A styled segment inside a rendered markdown line.
///
/// Keep field-compatible with the TUI variant `vtcode_ui::tui::ui::markdown::MarkdownSegment`;
/// `vtcode-core` picks one of the two by feature and streams both through the same shapes.
#[derive(Clone, Debug)]
pub struct MarkdownSegment {
    pub style: Style,
    pub text: String,
    pub link_target: Option<Box<str>>,
}

/// A single rendered markdown line made up of styled segments.
///
/// Keep field-compatible with the TUI variant `vtcode_ui::tui::ui::markdown::MarkdownLine`.
#[derive(Clone, Debug, Default)]
pub struct MarkdownLine {
    pub segments: Vec<MarkdownSegment>,
    pub line_background: Option<Color>,
}

impl MarkdownLine {
    pub fn is_empty(&self) -> bool {
        self.segments.iter().all(|segment| segment.text.trim().is_empty())
    }
}

/// Visual weight for fenced code blocks.
///
/// `Primary` keeps the theme foreground at full intensity. `Muted` recedes the
/// block toward the theme background so secondary surfaces (evidence bodies
/// behind a disclosure) stay readable without competing with the summary.
/// Muted is an explicit theme-derived color, never a faint/dim text attribute:
/// several terminals attenuate that attribute to near-invisible and ratatui
/// only ever *inserts* modifiers, so a dimmed area background would mute every
/// glyph painted on top of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodeBlockWeight {
    /// Theme foreground at full intensity.
    #[default]
    Primary,
    /// Receded toward the theme background.
    Muted,
}

/// Options for the markdown rendering pipeline.
///
/// Keep field-compatible with the TUI variant `vtcode_ui::tui::ui::markdown::RenderMarkdownOptions`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderMarkdownOptions {
    pub preserve_code_indentation: bool,
    pub disable_code_block_table_reparse: bool,
    /// Available content width for tables. Headered tables keep a padded,
    /// width-aware grid while cells remain readable, then fall back to aligned
    /// labeled records when the grid becomes too cramped. Headerless tables
    /// retain their grid layout and scale columns to fit. Only effective in
    /// TUI mode.
    pub table_max_width: Option<usize>,
    /// Visual weight applied to fenced code blocks. Only effective in TUI
    /// mode.
    pub code_block_weight: CodeBlockWeight,
}

/// A syntax-highlighted text segment.
///
/// Keep field-compatible with the TUI variant `vtcode_ui::tui::ui::markdown::HighlightedSegment`.
#[derive(Clone, Debug)]
pub struct HighlightedSegment {
    pub style: Style,
    pub text: String,
}
