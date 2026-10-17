//! Exact-first matching inspired by Codex's seek_sequence.rs at
//! f87ba04ec3cd386a92e775405b252a310b40bb0d (Apache-2.0, copyright 2025 OpenAI).
//! <https://github.com/openai/codex/blob/f87ba04ec3cd386a92e775405b252a310b40bb0d/codex-rs/apply-patch/src/seek_sequence.rs>
//! Independently implemented: unique matches only, with no punctuation normalization.

use std::ops::Range;
use std::path::Path;

use super::super::error::{EditFailure, EditFailureKind, EditLocation};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum MatchKind {
    Exact,
    LineEdgeWhitespace,
}

#[derive(Debug)]
pub(super) struct ReplacementPlan {
    pub byte_range: Range<usize>,
    pub line_range: Range<usize>,
    pub replacement: String,
    pub match_kind: MatchKind,
}

struct ReplacementCandidate {
    byte_range: Range<usize>,
    line_ending: &'static str,
}

fn line_range(content: &str, bytes: &Range<usize>) -> Range<usize> {
    let start = content[..bytes.start].bytes().filter(|byte| *byte == b'\n').count();
    let end = content.as_bytes()[..bytes.end.saturating_sub(1)]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    start..end
}

impl ReplacementPlan {
    pub(super) fn build(path: &Path, content: &str, old: &str, new: &str) -> Result<Self, EditFailure> {
        // Include overlapping occurrences: replacing "aa" in "aaa" is ambiguous.
        let mut matches = content
            .char_indices()
            .filter_map(|(offset, _)| {
                (!old.is_empty() && content[offset..].starts_with(old)).then_some(ReplacementCandidate {
                    byte_range: offset..offset + old.len(),
                    line_ending: "",
                })
            })
            .take(6)
            .collect::<Vec<_>>();
        let mut match_kind = MatchKind::Exact;
        if matches.is_empty() && !old.is_empty() {
            match_kind = MatchKind::LineEdgeWhitespace;
            let pattern_lines = old.lines().collect::<Vec<_>>();
            let source_lines = content.split_inclusive('\n').collect::<Vec<_>>();
            let mut byte_offset = 0;
            for (index, line) in source_lines.iter().enumerate() {
                if let Some(window) = source_lines.get(index..index + pattern_lines.len())
                    && !pattern_lines.is_empty()
                    && window
                        .iter()
                        .zip(&pattern_lines)
                        .all(|(source, pattern)| source.trim() == pattern.trim())
                {
                    let mut end = byte_offset + window.iter().map(|line| line.len()).sum::<usize>();
                    if !old.ends_with('\n') && window.last().is_some_and(|line| line.ends_with('\n')) {
                        end -= if window.last().is_some_and(|line| line.ends_with("\r\n")) {
                            2
                        } else {
                            1
                        };
                    }
                    matches.push(ReplacementCandidate {
                        byte_range: byte_offset..end,
                        line_ending: if line.ends_with("\r\n") { "\r\n" } else { "\n" },
                    });
                    if matches.len() == 6 {
                        break;
                    }
                }
                byte_offset += line.len();
            }
        }
        if matches.len() != 1 {
            let locations = matches
                .iter()
                .take(5)
                .map(|candidate| {
                    let lines = line_range(content, &candidate.byte_range);
                    EditLocation { start_line: lines.start + 1, end_line: lines.end }
                })
                .collect();
            return Err(EditFailure::new(
                if matches.is_empty() {
                    EditFailureKind::NoMatch
                } else {
                    EditFailureKind::AmbiguousMatch
                },
                path,
                locations,
                old,
            ));
        }
        let candidate = matches.remove(0);
        let byte_range = candidate.byte_range;
        // On a whitespace fallback keep the target's CRLF convention inside the
        // replacement as well. Exact replacements retain the supplied bytes.
        let replacement = if match_kind == MatchKind::LineEdgeWhitespace && candidate.line_ending == "\r\n" {
            new.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            new.to_owned()
        };
        let mut affected_lines = line_range(content, &byte_range);
        let starts_at_boundary = byte_range.start == 0 || content.as_bytes()[byte_range.start - 1] == b'\n';
        let ends_at_boundary = content.as_bytes().get(byte_range.end.saturating_sub(1)) == Some(&b'\n');
        if ends_at_boundary
            && byte_range.end < content.len()
            && !replacement.ends_with('\n')
            && !(replacement.is_empty() && starts_at_boundary)
        {
            // Joining a surviving suffix changes its line too. Inspect and map
            // that line rather than crediting previously unseen suffix bytes.
            affected_lines.end += 1;
        }
        Ok(Self {
            line_range: affected_lines,
            byte_range,
            replacement,
            match_kind,
        })
    }

    pub(super) fn apply(&self, content: &str) -> String {
        let mut after = String::with_capacity(content.len() - self.byte_range.len() + self.replacement.len());
        after.push_str(&content[..self.byte_range.start]);
        after.push_str(&self.replacement);
        after.push_str(&content[self.byte_range.end..]);
        after
    }
}

#[cfg(test)]
mod tests;
