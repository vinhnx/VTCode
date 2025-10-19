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

/// Returns true when every term in the query appears as an ordered subsequence
/// within the candidate text.
pub fn fuzzy_match(query: &str, candidate: &str) -> bool {
    if query.is_empty() {
        return true;
    }

    query
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
        .all(|segment| fuzzy_subsequence(segment, candidate))
}

/// Returns true when the characters from `needle` can be found in order within
/// `haystack`.
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
}
