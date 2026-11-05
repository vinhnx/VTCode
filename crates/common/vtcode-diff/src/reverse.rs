//! Strict per-file unified-diff splitting and exact reverse application.
//!
//! Rendering tolerates lossy previews (truncated bodies, omitted rows,
//! missing-newline markers). Reversal cannot: every byte of the pre-image
//! must be recoverable from the recorded hunks, so this module rejects
//! anything it cannot reproduce exactly. Callers that only display diffs
//! should keep using [`DiffDocument::from_unified`].

use super::compute::split_line_slices;
use super::parse::{parse_hunk_range, trim_line_ending};
use super::{DiffDocument, DiffHunk, DiffLineKind, ParseDiffError};

/// Refusal bound for reverse matching: inputs above this size are rejected
/// rather than matched slowly on a request path.
pub const MAX_REVERSE_FILE_BYTES: usize = 8 * 1024 * 1024;

/// One file's hunks from a multi-file unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedFile {
    /// Old-side path, or `None` for `/dev/null` (added file).
    pub old_path: Option<String>,
    /// New-side path, or `None` for `/dev/null` (deleted file).
    pub new_path: Option<String>,
    /// Hunks in file order; never empty.
    pub hunks: Vec<DiffHunk>,
}

/// Exact reversal failure. Every variant refuses the whole file: partial
/// reversal would silently corrupt content the evidence cannot reproduce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReverseApplyError {
    /// No hunks to reverse.
    EmptyDiff,
    /// Current content exceeds [`MAX_REVERSE_FILE_BYTES`].
    TooLarge,
    /// Hunk has no new-side lines, so no content anchors its position.
    EmptyNewSide {
        /// Zero-based hunk index.
        hunk: usize,
    },
    /// The hunk's new-side block does not occur in the current content:
    /// the file changed since the recorded edit.
    NoMatch {
        /// Zero-based hunk index.
        hunk: usize,
    },
    /// The hunk's new-side block occurs more than once: reversing would
    /// guess which occurrence the recorded edit produced.
    Ambiguous {
        /// Zero-based hunk index.
        hunk: usize,
    },
    /// Matched spans overlap or run out of order.
    OverlappingSpans,
}

impl std::fmt::Display for ReverseApplyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDiff => formatter.write_str("no hunks to reverse"),
            Self::TooLarge => formatter.write_str("file exceeds the reversal size bound"),
            Self::EmptyNewSide { hunk } => write!(formatter, "hunk {hunk} has no new-side lines"),
            Self::NoMatch { hunk } => {
                write!(formatter, "hunk {hunk} does not match the current content")
            }
            Self::Ambiguous { hunk } => {
                write!(formatter, "hunk {hunk} matches the current content more than once")
            }
            Self::OverlappingSpans => formatter.write_str("hunk matches overlap or run out of order"),
        }
    }
}

impl std::error::Error for ReverseApplyError {}

/// Splits multi-file unified text into per-file hunk groups.
///
/// # Requires
///
/// - `input` is UTF-8 unified-diff text.
///
/// # Ensures
///
/// - `Ok(files)` holds one entry per file section, each with at least one
///   hunk validated by [`DiffDocument::from_unified`].
/// - Returns `Err` for lossy input a renderer would tolerate: `\ No newline`
///   markers, omitted-row markers, sections without hunks, and unparseable
///   quoted paths. Total (terminating): single pass plus one bounded parse
///   per section.
pub fn split_unified_files(input: &str) -> Result<Vec<UnifiedFile>, ParseDiffError> {
    for raw in split_line_slices(input) {
        let trimmed = trim_line_ending(raw);
        if trimmed == "\\ No newline at end of file" {
            return Err(ParseDiffError::new("missing-newline marker cannot be reversed exactly"));
        }
        // Body-prefixed lines are file content, not markers: a context
        // line may legitimately read `... 3 lines omitted ...`.
        let body_prefixed = matches!(trimmed.as_bytes().first(), Some(b' ' | b'+' | b'-'));
        if !body_prefixed && super::parse::parse_omitted_line_count(trimmed).is_some() {
            return Err(ParseDiffError::new("omitted rows cannot be reversed exactly"));
        }
    }

    // Hunk-body tracking mirrors `DiffDocument::from_unified`: a `--- ` line
    // is a file header only outside a hunk body, since a deleted line whose
    // content starts with `-- ` renders identically (likewise `+++ ` for an
    // added `++ ` line). Headers are recorded during the split so body
    // lines can never be misread as paths.
    let mut sections: Vec<Section> = Vec::new();
    let mut section_has_hunk = false;
    let mut current = Section::default();
    let mut expected_old = 0usize;
    let mut expected_new = 0usize;
    let mut seen_old = 0usize;
    let mut seen_new = 0usize;
    let mut in_hunk = false;

    for raw in split_line_slices(input) {
        let hunk_complete = !in_hunk || (seen_old >= expected_old && seen_new >= expected_new);
        let trimmed = trim_line_ending(raw);
        if trimmed.starts_with("@@") {
            let (_, old_count, _, new_count) =
                parse_hunk_range(trimmed).ok_or_else(|| ParseDiffError::new("invalid hunk header"))?;
            in_hunk = true;
            section_has_hunk = true;
            expected_old = old_count;
            expected_new = new_count;
            seen_old = 0;
            seen_new = 0;
            current.lines.push(raw);
            continue;
        }
        if trimmed.starts_with("diff --git ") {
            // Body lines always carry a ` `/`+`/`-` prefix, so a column-zero
            // `diff --git` header is always a real boundary.
            if !current.lines.is_empty() {
                sections.push(std::mem::take(&mut current));
                section_has_hunk = false;
            }
            in_hunk = false;
            if current.git.is_none() {
                current.git = trimmed.strip_prefix("diff --git ");
            }
            current.lines.push(raw);
            continue;
        }
        if hunk_complete && trimmed.starts_with("--- ") {
            // A `--- ` header opens a new file only when the current
            // section already describes one (its own header or hunks);
            // otherwise it is the current file's header following a
            // `diff --git` line.
            if !current.lines.is_empty() && (current.old.is_some() || section_has_hunk) {
                sections.push(std::mem::take(&mut current));
                section_has_hunk = false;
            }
            in_hunk = false;
            if current.old.is_none() {
                current.old = trimmed.strip_prefix("--- ");
            }
            current.lines.push(raw);
            continue;
        }
        if hunk_complete && trimmed.starts_with("+++ ") && current.new.is_none() {
            current.new = trimmed.strip_prefix("+++ ");
        }
        if in_hunk && !hunk_complete {
            match trimmed.as_bytes().first().copied() {
                Some(b'-') => seen_old += 1,
                Some(b'+') => seen_new += 1,
                Some(b' ') => {
                    seen_old += 1;
                    seen_new += 1;
                }
                _ => {}
            }
        }
        current.lines.push(raw);
    }
    if !current.lines.is_empty() {
        sections.push(current);
    }

    let mut files = Vec::with_capacity(sections.len());
    for section in &sections {
        let text: String = section.lines.concat();
        let document = DiffDocument::from_unified(&text)?;
        if document.stats.omitted_rows > 0 {
            return Err(ParseDiffError::new("omitted rows cannot be reversed exactly"));
        }
        if document.hunks.is_empty() {
            return Err(ParseDiffError::new("file section without hunks"));
        }
        let (old_path, new_path) = section_paths(section)?;
        files.push(UnifiedFile { old_path, new_path, hunks: document.hunks });
    }
    Ok(files)
}

#[derive(Default)]
struct Section<'a> {
    lines: Vec<&'a str>,
    old: Option<&'a str>,
    new: Option<&'a str>,
    git: Option<&'a str>,
}

/// Reverses structured hunks against current content, returning the pre-image.
///
/// Each hunk's new-side block must occur exactly once in `current`, and the
/// matches must run in hunk order without overlap. Edits outside the matched
/// spans are preserved; edits inside them refuse the whole file.
///
/// # Requires
///
/// - `hunks` comes from [`split_unified_files`] (strict fidelity).
///
/// # Ensures
///
/// - `Ok(pre)` replaces exactly the recorded new-side blocks with their
///   old-side counterparts; `Err` computes nothing partial (pure).
/// - Total (terminating): bounded scans over `current` and `hunks`.
pub fn reverse_apply_hunks(current: &str, hunks: &[DiffHunk]) -> Result<String, ReverseApplyError> {
    if hunks.is_empty() {
        return Err(ReverseApplyError::EmptyDiff);
    }
    if current.len() > MAX_REVERSE_FILE_BYTES {
        return Err(ReverseApplyError::TooLarge);
    }
    let segments = split_line_slices(current);
    // Loop invariant: `spans` holds one verified `(start, end, old_block)`
    // per processed hunk, in hunk order.
    let mut spans: Vec<(usize, usize, Vec<&str>)> = Vec::with_capacity(hunks.len());
    for (index, hunk) in hunks.iter().enumerate() {
        let new_block: Vec<&str> = hunk
            .lines
            .iter()
            .filter(|line| line.kind != DiffLineKind::Deletion)
            .map(|line| line.text.as_str())
            .collect();
        if new_block.is_empty() {
            return Err(ReverseApplyError::EmptyNewSide { hunk: index });
        }
        // Two matches are enough to tell unique from ambiguous: never
        // scan past the second.
        let matches: Vec<usize> = segments
            .windows(new_block.len())
            .enumerate()
            .filter_map(|(start, window)| (window == new_block.as_slice()).then_some(start))
            .take(2)
            .collect();
        let [start] = matches.as_slice() else {
            if matches.is_empty() {
                return Err(ReverseApplyError::NoMatch { hunk: index });
            }
            return Err(ReverseApplyError::Ambiguous { hunk: index });
        };
        let old_block: Vec<&str> = hunk
            .lines
            .iter()
            .filter(|line| line.kind != DiffLineKind::Addition)
            .map(|line| line.text.as_str())
            .collect();
        spans.push((*start, start + new_block.len(), old_block));
    }
    for pair in spans.windows(2) {
        let [(.., previous_end, _), (start, ..)] = pair else {
            continue;
        };
        if start < previous_end {
            return Err(ReverseApplyError::OverlappingSpans);
        }
    }

    let mut restored = String::with_capacity(current.len());
    let mut cursor = 0usize;
    for (start, end, old_block) in &spans {
        for segment in &segments[cursor..*start] {
            restored.push_str(segment);
        }
        for line in old_block {
            restored.push_str(line);
        }
        cursor = *end;
    }
    for segment in &segments[cursor..] {
        restored.push_str(segment);
    }
    Ok(restored)
}

fn section_paths(section: &Section<'_>) -> Result<(Option<String>, Option<String>), ParseDiffError> {
    if section.old.is_some() || section.new.is_some() {
        let old_path = section.old.map(normalize_diff_path).transpose()?.flatten();
        let new_path = section.new.map(normalize_diff_path).transpose()?.flatten();
        return Ok((old_path, new_path));
    }
    if let Some(git) = section.git {
        let (old, new) = split_git_header(git)?;
        return Ok((normalize_diff_path(old)?, normalize_diff_path(new)?));
    }
    Ok((None, None))
}

fn split_git_header(rest: &str) -> Result<(&str, &str), ParseDiffError> {
    let rest = rest.trim_end();
    let (old, new) = if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted
            .find('"')
            .ok_or_else(|| ParseDiffError::new("unterminated quoted diff path"))?;
        let new_start = quoted[end + 1..]
            .find('"')
            .ok_or_else(|| ParseDiffError::new("unterminated quoted diff path"))?;
        let new_quoted = &quoted[end + 1 + new_start + 1..];
        let new_end = new_quoted
            .find('"')
            .ok_or_else(|| ParseDiffError::new("unterminated quoted diff path"))?;
        (&quoted[..end], &new_quoted[..new_end])
    } else {
        let mut parts = rest.splitn(2, ' ');
        let old = parts.next().ok_or_else(|| ParseDiffError::new("malformed diff header"))?;
        let new = parts.next().ok_or_else(|| ParseDiffError::new("malformed diff header"))?;
        (old, new)
    };
    if old.is_empty() || new.is_empty() {
        return Err(ParseDiffError::new("malformed diff header"));
    }
    Ok((old, new))
}

fn normalize_diff_path(path: &str) -> Result<Option<String>, ParseDiffError> {
    let path = path.strip_prefix("a/").or_else(|| path.strip_prefix("b/")).unwrap_or(path);
    if path == "/dev/null" {
        return Ok(None);
    }
    if path.contains('\\') {
        // C-style quoted escapes are not decoded: refusing beats guessing.
        return Err(ParseDiffError::new("unsupported escaped diff path"));
    }
    Ok(Some(path.to_owned()))
}
