use regex::Regex;
use std::sync::LazyLock;

pub(crate) fn display_error(message: &str) {
    tracing::error!("{message}");
}

/// Shared token scanners for transcript and markdown link detectors.
///
/// Both detectors first match quoted paths, then fall back to whitespace
/// tokens while skipping ranges already claimed by a quoted match. The
/// patterns are byte-identical by construction; per-match building,
/// extension policy, and filesystem resolution stay caller-owned.
pub(crate) static NON_WHITESPACE_TOKEN_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\S+").expect("valid transcript token regex"));
pub(crate) static QUOTED_PATH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"`(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^`]+`|"(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^"]+"|'(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^']+'"#,
    )
    .expect("valid quoted transcript path regex")
});

/// Shared explicit path-syntax check for transcript and markdown link detectors.
///
/// Covers path prefixes (`./`, `../`, `/`, `~/`, `file://`), Windows drive
/// letters (`C:\`, `C:/`), and embedded separators. Extension policy, URL
/// rejection, and filesystem/workspace resolution stay caller-owned:
/// transcript favors recall + `is_file`, markdown favors precision via
/// allowlists with no fs access.
pub(crate) fn has_explicit_path_syntax(token: &str) -> bool {
    token.starts_with("./")
        || token.starts_with("../")
        || token.starts_with('/')
        || token.starts_with("~/")
        || token.starts_with("file://")
        // Windows drive letter (e.g. C:\ or C:/)
        || (token.len() >= 3
            && token.as_bytes()[0].is_ascii_alphabetic()
            && token.as_bytes()[1] == b':'
            && matches!(token.as_bytes()[2], b'\\' | b'/'))
        // Contains path separator
        || token.contains('/')
        || token.contains('\\')
}

/// Trim surrounding punctuation, retaining parenthesized source locations.
pub(crate) fn trim_transcript_token_bounds(token: &str) -> (usize, usize) {
    let mut start = 0usize;
    let mut end = token.len();

    while start < end {
        let Some(ch) = token[start..end].chars().next() else {
            break;
        };
        if matches!(ch, '(' | '[' | '{' | '<' | '"' | '\'' | '`') {
            start += ch.len_utf8();
        } else {
            break;
        }
    }

    while start < end {
        let Some(ch) = token[start..end].chars().next_back() else {
            break;
        };
        if matches!(ch, ')' | ']' | '}' | '>' | '"' | '\'' | '`' | ',' | ';' | '.' | '!' | '?') {
            // Preserve trailing ')' when it looks like a location suffix e.g. file.rs(10,5)
            if ch == ')' && location_paren_suffix_start(&token[start..end]).is_some() {
                break;
            }
            end -= ch.len_utf8();
        } else {
            break;
        }
    }

    (start, end)
}

fn location_paren_suffix_start(token: &str) -> Option<usize> {
    let paren_start = token.rfind('(')?;
    let inner = token[paren_start + 1..].strip_suffix(')')?;
    // Accept `(digits)` or `(digits,digits)` — reject empty or malformed like `(,)` `(10,,5)`
    let valid = !inner.is_empty()
        && !inner.starts_with(',')
        && !inner.ends_with(',')
        && !inner.contains(",,")
        && inner.chars().all(|c| c.is_ascii_digit() || c == ',');
    valid.then_some(paren_start)
}

#[cfg(test)]
mod tests {
    use super::{has_explicit_path_syntax, trim_transcript_token_bounds};

    #[test]
    fn explicit_path_syntax_covers_prefixes_drives_and_separators() {
        for token in [
            "./Cargo.toml",
            "../src/main.rs",
            "/etc/hosts",
            "~/notes.md",
            "file://src/main.rs",
            "C:\\Windows\\notepad.exe",
            "c:/src/main.rs",
            "src/main.rs",
            "src\\main.rs",
        ] {
            assert!(has_explicit_path_syntax(token), "explicit path: {token}");
        }
        for token in ["", "ordinary", "Cargo", "main.rs", ".rs", "3999:"] {
            assert!(!has_explicit_path_syntax(token), "not explicit syntax: {token:?}");
        }
    }

    #[test]
    fn token_bounds_preserve_unicode_and_valid_location_suffixes() {
        for (token, expected) in [
            ("[`src/日本.rs(10,5)`].", "src/日本.rs(10,5)"),
            ("(src/é.rs)", "src/é.rs"),
            ("\"src/é.rs(10)\",", "src/é.rs(10)"),
            ("src/é.rs(10,,5).", "src/é.rs(10,,5"),
            ("src/é.rs(,).", "src/é.rs("),
            ("src/é.rs().", "src/é.rs("),
            ("src/é.rs", "src/é.rs"),
            ("[``]", ""),
            ("", ""),
        ] {
            let (start, end) = trim_transcript_token_bounds(token);
            assert_eq!(&token[start..end], expected, "token: {token}");
        }
    }
}
