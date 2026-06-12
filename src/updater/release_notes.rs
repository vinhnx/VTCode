use std::sync::LazyLock;

use regex::Regex;
use semver::Version;

/// Parsed highlight items extracted from a GitHub release body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParsedReleaseHighlights {
    /// The version string, e.g. "0.125.3"
    pub version: String,
    /// Bullet items extracted from the `### Highlights` section.
    pub items: Vec<String>,
}

/// Extract highlight bullet items from a GitHub release body markdown.
///
/// Looks for a `### Highlights` section and collects all `- ` prefixed lines
/// (including those under `####` sub-headers) until the next `##` header or
/// end of input. Trailing commit hash and author annotations like
/// `(071c9e64) (@username)` are stripped.
pub(crate) fn parse_highlights(version: &Version, body: &str) -> ParsedReleaseHighlights {
    let items = extract_highlight_items(body);

    ParsedReleaseHighlights {
        version: version.to_string(),
        items,
    }
}

fn extract_highlight_items(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut in_highlights = false;

    for line in body.lines() {
        let trimmed = line.trim();

        // Detect the start of the Highlights section
        if trimmed.eq_ignore_ascii_case("### Highlights") {
            in_highlights = true;
            continue;
        }

        if !in_highlights {
            continue;
        }

        // Stop at the next major section (## or ### that isn't a sub-header)
        if trimmed.starts_with("## ") {
            break;
        }

        // Skip #### sub-headers (Features, Bug Fixes, etc.) but keep collecting
        if trimmed.starts_with("#### ") {
            continue;
        }

        // Collect bullet items
        if let Some(content) = trimmed.strip_prefix("- ") {
            let cleaned = strip_commit_metadata(content);
            let cleaned = cleaned.trim();
            if !cleaned.is_empty() {
                items.push(cleaned.to_string());
            }
        }
    }

    items
}

/// Compiled regex for stripping trailing `(commit_hash) (@author)` patterns.
static COMMIT_METADATA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*\([0-9a-fA-F]+\)\s*(\(@[^)]+\))?\s*$").unwrap());

/// Strip trailing `(commit_hash) (@author)` patterns from a highlight line.
fn strip_commit_metadata(text: &str) -> String {
    COMMIT_METADATA_RE.replace(text, "").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn empty_body_returns_empty_items() {
        let result = parse_highlights(&v("0.1.0"), "");
        assert!(result.items.is_empty());
        assert_eq!(result.version, "0.1.0");
    }

    #[test]
    fn no_highlights_section_returns_empty() {
        let body = "## 0.1.0\n### Features\n- Something\n";
        let result = parse_highlights(&v("0.1.0"), body);
        assert!(result.items.is_empty());
    }

    #[test]
    fn extracts_basic_highlights() {
        let body = "\
## 0.125.0 - 2026-06-10

### Highlights
#### Features

- Add vtcode-ui to publish sequence (bb7228ed)
- Implement new parser (a1b2c3d4) (@vinhnx)

### Other Changes
#### Other

- Some other change
";
        let result = parse_highlights(&v("0.125.0"), body);
        assert_eq!(result.items.len(), 2);
        assert_eq!(result.items[0], "Add vtcode-ui to publish sequence");
        assert_eq!(result.items[1], "Implement new parser");
    }

    #[test]
    fn handles_subsections() {
        let body = "\
### Highlights
#### Bug Fixes

- Fix parameter handling (dac7afa0)
- Apply PR review fixes (d530f6c7) (@kernitus)

#### Features

- Add new model support (d4ed7872)

### Other Changes
- Not a highlight
";
        let result = parse_highlights(&v("0.124.0"), body);
        assert_eq!(result.items.len(), 3);
        assert_eq!(result.items[0], "Fix parameter handling");
        assert_eq!(result.items[1], "Apply PR review fixes");
        assert_eq!(result.items[2], "Add new model support");
    }

    #[test]
    fn strips_commit_hash_only() {
        let body = "\
### Highlights
- Simple fix (abc1234)
";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["Simple fix"]);
    }

    #[test]
    fn strips_commit_hash_and_author() {
        let body = "\
### Highlights
- Feature name (deadbeef) (@someone)
";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["Feature name"]);
    }

    #[test]
    fn stops_at_next_major_section() {
        let body = "\
### Highlights
- Item one (abc1234)

### Other Changes
- Should not appear (def5678)

### More Stuff
- Also excluded
";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["Item one"]);
    }

    #[test]
    fn skips_empty_bullet_lines() {
        let body = "\
### Highlights
- First item (abc1234)

- Second item (def5678)
";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["First item", "Second item"]);
    }

    #[test]
    fn highlights_with_no_subsections() {
        let body = "\
### Highlights
- Direct item one (abc1234)
- Direct item two (def5678) (@author)
";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["Direct item one", "Direct item two"]);
    }

    #[test]
    fn case_insensitive_header_match() {
        let body = "### highlights\n- Item (abc1234)\n";
        let result = parse_highlights(&v("0.1.0"), body);
        assert_eq!(result.items, vec!["Item"]);
    }
}
