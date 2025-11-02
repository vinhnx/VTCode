use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Normalizes a user-provided query by trimming whitespace, collapsing internal
/// spaces, and converting everything to lowercase ASCII.
pub fn normalize_query(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut normalized = String::with_capacity(trimmed.len());
    let mut last_was_space = false;
    for ch in trimmed.chars() {
        if ch.is_whitespace() {
            if !last_was_space && !normalized.is_empty() {
                normalized.push(' ');
            }
            last_was_space = true;
        } else {
            normalized.extend(ch.to_lowercase());
            last_was_space = false;
        }
    }

    normalized.trim_end().to_string()
}

/// Returns true when every term in the query appears as a fuzzy match
/// within the candidate text using nucleo-matcher.
pub fn fuzzy_match(query: &str, candidate: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buffer = Vec::new();
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let utf32_candidate = Utf32Str::new(candidate, &mut buffer);
    pattern.score(utf32_candidate, &mut matcher).is_some()
}

/// Returns true when the characters from `needle` can be found in order within
/// `haystack` (kept for backward compatibility).
pub fn fuzzy_subsequence(needle: &str, haystack: &str) -> bool {
    if needle.is_empty() {
        return true;
    }

    let mut needle_chars = needle.chars();
    let mut current = match needle_chars.next() {
        Some(value) => value,
        None => return true,
    };

    for ch in haystack.chars() {
        if ch == current {
            match needle_chars.next() {
                Some(next) => current = next,
                None => return true,
            }
        }
    }

    false
}

/// Returns a score for the fuzzy match between query and candidate using nucleo-matcher.
/// Returns None if no match is found, Some(score) if a match exists.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0); // Default score for empty query
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let mut buffer = Vec::new();
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let utf32_candidate = Utf32Str::new(candidate, &mut buffer);
    pattern.score(utf32_candidate, &mut matcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_query_trims_and_lowercases() {
        let normalized = normalize_query("   Foo   Bar   BAZ  ");
        assert_eq!(normalized, "foo bar baz");
    }

    #[test]
    fn normalize_query_handles_whitespace_only() {
        assert!(normalize_query("   ").is_empty());
    }

    #[test]
    fn fuzzy_subsequence_requires_in_order_match() {
        assert!(fuzzy_subsequence("abc", "a_b_c"));
        assert!(!fuzzy_subsequence("abc", "acb"));
    }

    #[test]
    fn fuzzy_match_supports_multiple_terms() {
        assert!(fuzzy_match("run cmd", "run command"));
        assert!(!fuzzy_match("missing", "run command"));
    }

    #[test]
    fn fuzzy_match_with_nucleo_basic() {
        // Test that nucleo-based fuzzy matching works
        assert!(fuzzy_match("smr", "src/main.rs"));
        assert!(fuzzy_match("src main", "src/main.rs"));
        assert!(fuzzy_match("main", "src/main.rs"));
        assert!(!fuzzy_match("xyz", "src/main.rs"));
    }

    #[test]
    fn fuzzy_score_returns_some_for_matches() {
        // Test that fuzzy scoring returns Some for valid matches
        assert!(fuzzy_score("smr", "src/main.rs").is_some());
        assert!(fuzzy_score("main", "src/main.rs").is_some());
    }

    #[test]
    fn fuzzy_score_returns_none_for_non_matches() {
        // Test that fuzzy scoring returns None for non-matches
        assert!(fuzzy_score("xyz", "src/main.rs").is_none());
    }
}
