//! Renderer-independent diff data types and their inherent constructors.
//!
//! This module owns the public vocabulary of the crate: the structured hunk
//! model, the semantic display-line model, layout request types, and the
//! responsive width constants shared by renderers.

use std::fmt;
use std::time::Duration;

/// A contiguous borrowed character-level diff chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chunk<'a> {
    /// Text present on both sides.
    Equal(&'a str),
    /// Text present only on the old side.
    Delete(&'a str),
    /// Text present only on the new side.
    Insert(&'a str),
}

/// Algorithms suitable for interactive diff previews.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum DiffAlgorithm {
    /// Practical, heuristic Myers diff.
    #[default]
    Myers,
    /// Patience diff, useful for source code with unique anchors.
    Patience,
    /// Histogram diff, useful for repeated source-code lines.
    Histogram,
}

impl DiffAlgorithm {
    pub(crate) fn similar(self) -> similar::Algorithm {
        match self {
            Self::Myers => similar::Algorithm::Myers,
            Self::Patience => similar::Algorithm::Patience,
            Self::Histogram => similar::Algorithm::Histogram,
        }
    }
}

/// Options controlling diff generation.
#[derive(Debug, Clone)]
pub struct DiffOptions<'a> {
    /// Number of unchanged lines retained around a change.
    pub context_lines: usize,
    /// Optional old-side label used by formatters.
    pub old_label: Option<&'a str>,
    /// Optional new-side label used by formatters.
    pub new_label: Option<&'a str>,
    /// Whether formatters should emit missing-final-newline markers.
    pub missing_newline_hint: bool,
    /// Line diff algorithm.
    pub algorithm: DiffAlgorithm,
    /// Maximum line-diff computation time.
    pub timeout: Duration,
    /// Maximum total time spent on intraline refinement.
    pub inline_timeout: Duration,
}

impl Default for DiffOptions<'_> {
    fn default() -> Self {
        Self {
            context_lines: 3,
            old_label: None,
            new_label: None,
            missing_newline_hint: true,
            algorithm: DiffAlgorithm::Myers,
            timeout: Duration::from_millis(200),
            inline_timeout: Duration::from_millis(40),
        }
    }
}

/// A diff hunk with old/new ranges and semantic lines.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiffHunk {
    /// One-based old-side start line.
    pub old_start: usize,
    /// Number of represented old-side lines.
    pub old_lines: usize,
    /// One-based new-side start line.
    pub new_start: usize,
    /// Number of represented new-side lines.
    pub new_lines: usize,
    /// Lines contained in the hunk.
    pub lines: Vec<DiffLine>,
}

/// The semantic role of a line inside a hunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum DiffLineKind {
    /// Unchanged context.
    Context,
    /// New-side insertion.
    Addition,
    /// Old-side deletion.
    Deletion,
}

/// A source line and its old/new positions.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiffLine {
    /// Semantic line role.
    pub kind: DiffLineKind,
    /// One-based old-side line number, when present.
    pub old_line: Option<u32>,
    /// One-based new-side line number, when present.
    pub new_line: Option<u32>,
    /// Source text, including its original line terminator when present.
    pub text: String,
}

/// Aggregate statistics for a complete document.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiffStats {
    /// Added lines.
    pub additions: usize,
    /// Deleted lines.
    pub deletions: usize,
    /// Context lines represented by hunks.
    pub context: usize,
    /// Number of hunks.
    pub hunks: usize,
    /// Semantic rows omitted by a bounded layout.
    pub omitted_rows: usize,
}

/// A complete renderer-independent diff.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiffDocument {
    /// Structured hunks.
    pub hunks: Vec<DiffHunk>,
    /// Whole-document statistics.
    pub stats: DiffStats,
    #[cfg_attr(feature = "serde", serde(default = "default_inline_timeout"))]
    pub(crate) inline_timeout: Duration,
}

impl Default for DiffDocument {
    fn default() -> Self {
        Self {
            hunks: Vec::new(),
            stats: DiffStats::default(),
            inline_timeout: default_inline_timeout(),
        }
    }
}

pub(crate) fn default_inline_timeout() -> Duration {
    Duration::from_millis(40)
}

/// Error returned for malformed unified diff input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiffError {
    message: &'static str,
}

impl ParseDiffError {
    pub(crate) fn new(message: &'static str) -> Self {
        Self { message }
    }
}

impl fmt::Display for ParseDiffError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for ParseDiffError {}

/// A diff rendered with both structured hunks and formatted text.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DiffBundle {
    /// Structured hunks.
    pub hunks: Vec<DiffHunk>,
    /// Caller-formatted representation.
    pub formatted: String,
    /// Whether both inputs were identical.
    pub is_empty: bool,
}
/// Intraline byte range, always aligned to UTF-8 boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IntralineRange {
    /// Inclusive byte start.
    pub start: usize,
    /// Exclusive byte end.
    pub end: usize,
}

/// Intra-line highlight ranges retained for compatibility.
pub type WordChangedRanges = Vec<(usize, usize)>;

/// Aggregate addition/deletion counts retained for compatibility.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiffChangeCounts {
    /// Added lines.
    pub additions: usize,
    /// Deleted lines.
    pub deletions: usize,
}

impl DiffChangeCounts {
    /// Total changed lines.
    #[must_use]
    pub const fn total(self) -> usize {
        self.additions + self.deletions
    }
}

/// Semantic role used by legacy preview consumers.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DiffDisplayKind {
    /// File or parser metadata.
    Metadata,
    /// Hunk range header.
    HunkHeader,
    /// Unchanged context.
    Context,
    /// Added content.
    Addition,
    /// Deleted content.
    Deletion,
}

impl DiffDisplayKind {
    /// Whether the kind represents source content.
    #[must_use]
    pub const fn is_diff(self) -> bool {
        matches!(self, Self::Context | Self::Addition | Self::Deletion)
    }
}

/// A semantic legacy display line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffDisplayLine {
    /// Semantic role.
    pub kind: DiffDisplayKind,
    /// Old-side number.
    pub old_line: Option<u32>,
    /// New-side number.
    pub new_line: Option<u32>,
    /// Content without the diff marker or line ending.
    pub text: String,
    /// Intraline changed byte ranges.
    pub changed: WordChangedRanges,
}

impl DiffDisplayLine {
    /// Constructs a source body line.
    #[must_use]
    pub fn body(kind: DiffDisplayKind, old_line: Option<u32>, new_line: Option<u32>, text: String) -> Self {
        Self {
            kind,
            old_line,
            new_line,
            text,
            changed: Vec::new(),
        }
    }

    /// Whether this line represents source content.
    #[must_use]
    pub const fn is_diff(&self) -> bool {
        self.kind.is_diff()
    }

    /// Formats a single numbered gutter.
    #[must_use]
    pub fn numbered_text(&self, line_number_width: usize) -> String {
        match self.kind {
            DiffDisplayKind::Metadata | DiffDisplayKind::HunkHeader => self.text.clone(),
            DiffDisplayKind::Deletion => {
                format!("-{:>width$} │ {}", self.old_line.unwrap_or_default(), self.text, width = line_number_width)
            }
            DiffDisplayKind::Addition => {
                format!("+{:>width$} │ {}", self.new_line.unwrap_or_default(), self.text, width = line_number_width)
            }
            DiffDisplayKind::Context => format!(
                " {:>width$} │ {}",
                self.new_line.or(self.old_line).unwrap_or_default(),
                self.text,
                width = line_number_width
            ),
        }
    }
}
/// Minimum content width for the paired old/new preview.
///
/// Below this width, two independently numbered panes leave too little room
/// for source text. Renderers should fall back to the unified presentation.
pub const DIFF_MIN_SIDE_BY_SIDE_WIDTH: usize = 60;

/// Whether a measured width can support the paired old/new preview.
///
/// An unknown width preserves the existing caller behavior; redirected
/// output and test sinks may not expose terminal sizing at all.
#[must_use]
pub const fn diff_side_by_side_fits(available_width: Option<usize>) -> bool {
    match available_width {
        Some(width) => width >= DIFF_MIN_SIDE_BY_SIDE_WIDTH,
        None => true,
    }
}

/// Minimum source width retained when the unified line gutter is visible.
///
/// This keeps the marker, line number, separator, and a useful amount of
/// source text together. Narrower layouts should hide the gutter and give its
/// columns back to the source body.
pub const DIFF_MIN_BODY_WIDTH_WITH_GUTTER: usize = 20;

/// Width consumed by a unified diff gutter after the line-number field.
///
/// The rendered shape is `+123 │ `: one marker plus the three-cell separator
/// around `│`. The line-number field is supplied by the caller because it is
/// derived from the visible diff excerpt.
#[must_use]
pub const fn diff_gutter_width(line_number_width: usize) -> usize {
    line_number_width.saturating_add(4)
}

/// Whether a unified diff can keep its marker, line number, and separator
/// without starving the source body.
#[must_use]
pub const fn diff_gutter_fits(available_width: usize, line_number_width: usize) -> bool {
    available_width >= diff_gutter_width(line_number_width).saturating_add(DIFF_MIN_BODY_WIDTH_WITH_GUTTER)
}

/// Width to pass to semantic unified layout when the renderer hides its
/// visible gutter.
///
/// `layout_display_lines` subtracts the normal gutter before wrapping source
/// text. Giving that width back keeps wrapping aligned with compact rendering
/// without adding another public layout option.
#[must_use]
pub const fn diff_layout_width(available_width: usize, line_number_width: usize, show_gutter: bool) -> usize {
    if show_gutter {
        available_width
    } else {
        available_width.saturating_add(diff_gutter_width(line_number_width))
    }
}
/// One paired row in the compatibility side-by-side model.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SideBySideRow {
    /// Old-side cell.
    pub left: Option<DiffDisplayLine>,
    /// New-side cell.
    pub right: Option<DiffDisplayLine>,
}

impl SideBySideRow {
    /// Whether the left cell is a header spanning both panes.
    #[must_use]
    pub fn is_full_width(&self) -> bool {
        self.right.is_none()
            && self
                .left
                .as_ref()
                .is_some_and(|line| matches!(line.kind, DiffDisplayKind::HunkHeader | DiffDisplayKind::Metadata))
    }
}
/// Requested preview layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum DiffLayout {
    /// One stacked old/new column.
    #[default]
    Unified,
    /// Paired old/new panes.
    SideBySide,
}

/// Width and bounded-excerpt options for semantic layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutOptions {
    /// Requested layout.
    pub layout: DiffLayout,
    /// Available terminal columns.
    pub width: usize,
    /// Maximum rows, including an omission marker.
    pub max_rows: usize,
    /// Whether source bodies hard-wrap at display width.
    pub wrap: bool,
    /// Side-by-side fallback threshold.
    pub min_side_by_side_width: usize,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            layout: DiffLayout::Unified,
            width: 80,
            max_rows: 2_000,
            wrap: true,
            min_side_by_side_width: DIFF_MIN_SIDE_BY_SIDE_WIDTH,
        }
    }
}

/// Semantic role for a renderer-neutral row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffRowKind {
    /// File metadata outside a hunk.
    Metadata,
    /// Hunk header.
    HunkHeader,
    /// Unchanged content.
    Context,
    /// Added content.
    Addition,
    /// Deleted content.
    Deletion,
    /// A bounded middle omission.
    Omission,
}

/// One independently styled content segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSegment {
    /// Segment text.
    pub text: String,
    /// Whether the segment receives intraline emphasis.
    pub emphasized: bool,
}

/// One side of a semantic preview row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffCell {
    /// Old-side line number.
    pub old_line: Option<u32>,
    /// New-side line number.
    pub new_line: Option<u32>,
    /// Diff marker.
    pub marker: char,
    /// Styled content segments.
    pub segments: Vec<DiffSegment>,
}

/// A renderer-neutral row, optionally containing paired side-by-side cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRow {
    /// Semantic role.
    pub kind: DiffRowKind,
    /// Marker for unified renderers and simple inspection.
    pub marker: char,
    /// Stable zero-based hunk identity.
    pub hunk_index: Option<usize>,
    /// Whether this row continues a hard-wrapped source line.
    pub continuation: bool,
    /// Unified or old-side cell.
    pub left: Option<DiffCell>,
    /// New-side cell in side-by-side mode.
    pub right: Option<DiffCell>,
}
