//! Parity tests for the MCP allow-list wildcard matcher.
//!
//! The matcher is an authorization boundary, so a "simplification" that widens
//! or narrows its grammar is a regression even when it looks equivalent. These
//! tests keep the legacy regex implementation as an oracle.

use super::wildcard_match;
use regex::Regex;

/// The exact regex-based matcher that shipped before the two-pointer
/// implementation. Kept in tests only, as a parity oracle.
fn legacy_regex_wildcard_match(pattern: &str, candidate: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let mut regex_pattern = String::from("^");
    let mut literal_buffer = String::new();
    for ch in pattern.chars() {
        match ch {
            '*' | '?' => {
                if !literal_buffer.is_empty() {
                    regex_pattern.push_str(&regex::escape(&literal_buffer));
                    literal_buffer.clear();
                }
                regex_pattern.push_str(if ch == '*' { ".*" } else { "." });
            }
            _ => literal_buffer.push(ch),
        }
    }
    if !literal_buffer.is_empty() {
        regex_pattern.push_str(&regex::escape(&literal_buffer));
    }
    regex_pattern.push('$');
    Regex::new(&regex_pattern).is_ok_and(|regex| regex.is_match(candidate))
}

#[test]
fn wildcard_match_preserves_the_regex_translation_grammar() {
    // Anchored, case-sensitive matching with '*' spanning any character and
    // '?' matching exactly one. Includes cases where glob would diverge:
    // recursive `**`, character classes, braces/backslashes and newlines.
    for (pattern, candidate, expected) in [
        ("*", "anything.at/all:123", true),
        ("get_*", "get_current_time", true),
        ("get_*", "prefix_get_current_time", false),
        ("mcp__time__*", "mcp__time__get_current_time", true),
        ("mcp__time__*", "mcp__other__get_current_time", false),
        ("tool.v?", "tool.v1", true),
        ("tool.v?", "tool.v12", false),
        ("a*b*c", "aXYZbXYZc", true),
        ("a*b*c", "aXYZb", false),
        ("exact", "exact", true),
        ("exact", "exact-extra", false),
        ("", "", true),
        ("", "nonempty", false),
        // Discriminating cases: a glob-based matcher gets these wrong.
        ("docs/**/*", "docs/foo.md", false),
        ("docs/**/*", "docs/a/foo.md", true),
        ("get_[abc]", "get_a", false),
        ("get_[abc]", "get_[abc]", true),
        ("a**b", "axb", true),
        ("{a,b}", "{a,b}", true),
        ("a\\b", "a\\b", true),
        ("[", "[", true),
        ("a?b", "a\nb", false),
        ("a*b", "a\nb", false),
        ("*", "a\nb", true),
    ] {
        assert_eq!(
            legacy_regex_wildcard_match(pattern, candidate),
            expected,
            "legacy oracle disagrees with the pinned expectation for {pattern:?} vs {candidate:?}"
        );
        assert_eq!(wildcard_match(pattern, candidate), expected, "pattern {pattern:?} against candidate {candidate:?}");
    }
}

#[test]
fn wildcard_match_agrees_with_the_legacy_regex_over_a_generated_corpus() {
    // Both sides are derived independently from the documented grammar, so
    // this catches backtracking and edge-case drift rather than restating
    // one implementation's output as the expectation.
    let atoms = ["a", "b", "*", "?", "[", "]", "{", "}", "\\", ".", "/", "\n"];
    let mut patterns = vec![String::new(), "*".to_owned()];
    for first in atoms {
        patterns.push(first.to_owned());
        for second in atoms {
            patterns.push(format!("{first}{second}"));
        }
    }
    // Multi-wildcard shapes exercise the backtracking path, where a greedy
    // two-pointer matcher is most likely to diverge from the regex oracle.
    patterns.extend(
        [
            "*a*", "a*b*", "*a*b", "*?*", "?*?", "**a", "a**", "a*a*a", "*a*a*", "?a?b?",
        ]
        .iter()
        .map(|pattern| (*pattern).to_owned()),
    );
    let mut candidates = vec![String::new()];
    for first in atoms {
        candidates.push(first.to_owned());
        for second in atoms {
            candidates.push(format!("{first}{second}"));
        }
    }
    candidates.extend(
        ["aba", "aXbXa", "a\nb", "a?b", "*a*", "abc", "a/b", "a[b]c"]
            .iter()
            .map(|candidate| (*candidate).to_owned()),
    );

    for pattern in &patterns {
        for candidate in &candidates {
            assert_eq!(
                wildcard_match(pattern, candidate),
                legacy_regex_wildcard_match(pattern, candidate),
                "pattern {pattern:?} against candidate {candidate:?}"
            );
        }
    }
}
