//! Utilities for parsing @ symbol patterns in user input

use regex::Regex;
use std::sync::LazyLock;

/// Regex to match @ followed by a potential file path or URL
/// Handles both quoted paths (with spaces) and unquoted paths
pub static AT_PATTERN_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"@(?:\"([^\"]+)\"|'([^']+)'|([^\s"'\[\](){}<>|\\^`]+))"#)
        .expect("Failed to compile @ pattern regex")
});

/// A parsed match of an @ pattern
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtPatternMatch<'a> {
    /// The full text of the match (e.g., "@file.txt")
    pub full_match: &'a str,
    /// The extracted path or URL part (e.g., "file.txt")
    pub path: &'a str,
    /// Start position in the original string
    pub start: usize,
    /// End position in the original string
    pub end: usize,
}

/// Find all @ patterns in the given text
pub fn find_at_patterns(text: &str) -> Vec<AtPatternMatch<'_>> {
    AT_PATTERN_REGEX
        .captures_iter(text)
        .filter_map(|cap| {
            let full_match = cap.get(0)?;
            let path_part = cap.get(1).or_else(|| cap.get(2)).or_else(|| cap.get(3))?;

            Some(AtPatternMatch {
                full_match: full_match.as_str(),
                path: path_part.as_str(),
                start: full_match.start(),
                end: full_match.end(),
            })
        })
        .collect()
}
