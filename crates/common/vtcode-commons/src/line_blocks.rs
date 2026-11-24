//! Line-block containment for deduplicating tool output streams.
//!
//! Multiple renderers need to know whether one stream's text already appears
//! inside another (merged `output` vs named `stdout`/`stderr`) so each line is
//! shown once. This module is the single home; the runloop pipe-stream
//! canonicalizer and the lifecycle summary aggregator used to carry verbatim
//! copies and now delegate here.

/// Ranges (line offsets) where `candidate` occurs in `container_lines` as a
/// contiguous block, in ascending order. Empty when `candidate` is empty or
/// longer than the container.
fn block_ranges_in(container_lines: &[&str], candidate: &str) -> Vec<(usize, usize)> {
    let candidate_lines = candidate.lines().collect::<Vec<_>>();
    if candidate_lines.is_empty() || candidate_lines.len() > container_lines.len() {
        return Vec::new();
    }

    container_lines
        .windows(candidate_lines.len())
        .enumerate()
        .filter_map(|(start, window)| {
            (window == candidate_lines.as_slice()).then_some((start, start + candidate_lines.len()))
        })
        .collect()
}

/// True when `candidate` occurs in `container` as a contiguous line block.
///
/// Short-circuits on the first match: unlike collecting ranges and testing
/// emptiness, this builds no ranges vector and stops scanning early. An empty
/// candidate matches nothing.
#[must_use]
pub fn contains_line_block(container: &str, candidate: &str) -> bool {
    let container_lines = container.lines().collect::<Vec<_>>();
    let candidate_lines = candidate.lines().collect::<Vec<_>>();
    if candidate_lines.is_empty() || candidate_lines.len() > container_lines.len() {
        return false;
    }

    container_lines
        .windows(candidate_lines.len())
        .any(|window| window == candidate_lines.as_slice())
}

/// True when `first` and `second` both occur in `container` in non-overlapping
/// ranges. Requiring distinct blocks matters when the streams are identical or
/// one is a prefix of the other: a single occurrence cannot prove both are
/// copies. The container lines are collected once for both scans.
#[must_use]
pub fn contains_distinct_line_blocks(container: &str, first: &str, second: &str) -> bool {
    let container_lines = container.lines().collect::<Vec<_>>();
    let first_ranges = block_ranges_in(&container_lines, first);
    let second_ranges = block_ranges_in(&container_lines, second);
    first_ranges.iter().any(|&(first_start, first_end)| {
        second_ranges
            .iter()
            .any(|&(second_start, second_end)| first_end <= second_start || second_end <= first_start)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_finds_middle_block_but_not_absent_text() {
        let container = "alpha\nbeta\ngamma\ndelta";
        assert!(contains_line_block(container, "beta\ngamma"));
        assert!(!contains_line_block(container, "beta\ndelta"));
    }

    #[test]
    fn contains_is_directional_for_prefix_pairs() {
        // Asymmetric oracle: a prefix is contained in the whole, never the reverse.
        assert!(contains_line_block("one\ntwo\nthree", "one\ntwo"));
        assert!(!contains_line_block("one\ntwo", "one\ntwo\nthree"));
    }

    #[test]
    fn contains_rejects_empty_and_oversized_candidates() {
        assert!(!contains_line_block("one\ntwo", ""));
        assert!(!contains_line_block("", "one"));
        assert!(!contains_line_block("", ""));
        assert!(!contains_line_block("one", "one\ntwo"));
    }

    #[test]
    fn contains_matches_exact_and_single_line_candidates() {
        assert!(contains_line_block("one\ntwo", "one\ntwo"));
        assert!(contains_line_block("one\ntwo\nthree", "two"));
        assert!(!contains_line_block("one\ntwo\nthree", "four"));
    }

    #[test]
    fn contains_ignores_trailing_newline_differences() {
        // `str::lines` strips terminators on both sides, so trailing newlines
        // never affect the comparison either way.
        assert!(contains_line_block("one\ntwo\n", "one\ntwo"));
        assert!(contains_line_block("one\ntwo", "one\ntwo\n"));
    }

    #[test]
    fn distinct_blocks_reject_a_single_shared_occurrence() {
        // Identical streams with one occurrence: one block cannot prove both.
        let container = "out\nerr";
        assert!(!contains_distinct_line_blocks(container, "out\nerr", "out\nerr"));
        // Same when one candidate is a prefix of the other over one occurrence.
        assert!(!contains_distinct_line_blocks(container, "out\nerr", "out"));
    }

    #[test]
    fn distinct_blocks_accept_separate_occurrences() {
        let container = "out-1\nerr-1\nout-2\nerr-2";
        assert!(contains_distinct_line_blocks(container, "out-1\nerr-1", "out-2\nerr-2"));
        // Order of the two candidates does not matter.
        assert!(contains_distinct_line_blocks(container, "out-2\nerr-2", "out-1\nerr-1"));
    }

    #[test]
    fn distinct_blocks_reject_missing_candidates() {
        let container = "out\nerr";
        assert!(!contains_distinct_line_blocks(container, "out", "absent"));
        assert!(!contains_distinct_line_blocks(container, "absent", "err"));
        assert!(!contains_distinct_line_blocks(container, "", "err"));
    }

    #[test]
    fn block_ranges_report_every_occurrence_in_order() {
        let container_lines = ["a", "b", "a", "b", "c"];
        assert_eq!(block_ranges_in(&container_lines, "a\nb"), vec![(0, 2), (2, 4)]);
        assert_eq!(block_ranges_in(&container_lines, "b\nc"), vec![(3, 5)]);
        assert!(block_ranges_in(&container_lines, "c\na").is_empty());
        assert!(block_ranges_in(&container_lines, "").is_empty());
        assert!(block_ranges_in(&[], "a").is_empty());
    }
}
