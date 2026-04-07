//! Shared markdown rendering types.
//!
//! The actual rendering implementation lives in `vtcode-tui`.  These structs
//! define the output format so that `vtcode-core` can work with rendered
//! markdown lines regardless of whether the TUI crate is compiled in.

use anstyle::Style;

/// A styled segment inside a rendered markdown line.
#[derive(Clone, Debug)]
pub struct MarkdownSegment {
    pub style: Style,
    pub text: String,
    pub link_target: Option<String>,
}

/// A single rendered markdown line made up of styled segments.
#[derive(Clone, Debug, Default)]
pub struct MarkdownLine {
    pub segments: Vec<MarkdownSegment>,
}

impl MarkdownLine {
    pub fn is_empty(&self) -> bool {
        self.segments
            .iter()
            .all(|segment| segment.text.trim().is_empty())
    }
}

/// Options for the markdown rendering pipeline.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderMarkdownOptions {
    pub preserve_code_indentation: bool,
    pub disable_code_block_table_reparse: bool,
}

/// A syntax-highlighted text segment.
#[derive(Clone, Debug)]
pub struct HighlightedSegment {
    pub style: Style,
    pub text: String,
}
