//! Per-tool headline builders for the transcript-display surfaces.
//!
//! These pure phrase builders turn a tool's argument payload into a short
//! `(headline, consumed keys)` pair. Kept in their own module so the parent
//! `tool_headlines` module stays focused on the shared lookup/formatting and
//! command-preview helpers. Every item here is re-exported from the parent,
//! so the public paths are unchanged.

use super::*;

/// Headline for `list_files`: location first, then the name and content
/// pattern fallbacks. Reports exactly the keys its headline consumed, so
/// detail collection hides if and only if the title phrased them.
#[must_use]
pub fn describe_list_files(args: &Value, workspace_root: Option<&Path>) -> Option<(String, &'static [&'static str])> {
    if let Some(path) = lookup_string(args, "path") {
        let location = if path == "." {
            "workspace root".to_string()
        } else {
            let rel = relativize_to_workspace(&path, workspace_root);
            truncate_path_middle(&rel, 60)
        };
        return Some((format!("List files in {location}"), &["path"]));
    }
    if let Some(pattern) = lookup_string(args, "name_pattern") {
        return Some((format!("Find files named {}", truncate_middle(&pattern, 40)), &["name_pattern"]));
    }
    if let Some(pattern) = lookup_string(args, "content_pattern") {
        return Some((format!("Search files for {}", truncate_middle(&pattern, 40)), &["content_pattern"]));
    }
    None
}

/// Headline for `grep_file`: the pattern is required, the scope joins it
/// when present. Reports exactly the keys its headline consumed.
#[must_use]
pub fn describe_grep_file(args: &Value, workspace_root: Option<&Path>) -> Option<(String, &'static [&'static str])> {
    let pattern = lookup_string(args, "pattern");
    let path = lookup_string(args, "path");
    match (pattern, path) {
        (Some(pat), Some(path)) => Some((
            format!(
                "Grep {} in {}",
                truncate_middle(&pat, 40),
                truncate_path_middle(&relativize_to_workspace(&path, workspace_root), 40)
            ),
            &["pattern", "path"],
        )),
        (Some(pat), None) => Some((format!("Grep {}", truncate_middle(&pat, 40)), &["pattern"])),
        _ => None,
    }
}

/// Headline for `code_search` over the query and its alias keys.
///
/// The schema requires `query`, but common aliases are accepted defensively
/// so a valid search never degrades to a generic header. Records the actual
/// matched key so detail collection and headline highlights stay in sync (a
/// hardcoded "query" would leak e.g. `Pattern: …` as a duplicate line).
#[must_use]
pub fn describe_code_search(args: &Value) -> Option<(String, &'static [&'static str])> {
    for key in ["query", "pattern", "q", "text"] {
        if let Some(query) = lookup_string(args, key) {
            let keys: &'static [&'static str] = match key {
                "query" => &["query"],
                "pattern" => &["pattern"],
                "q" => &["q"],
                _ => &["text"],
            };
            return Some((format!("Search code for {}", truncate_middle(&query, 40)), keys));
        }
    }
    None
}

/// Headline for the fetch tools over the full target URL.
///
/// Approval surfaces must show the full URL (not just the domain) so the
/// reader reviews the exact target. Falls back to a bare verb when no URL
/// is present, consuming no keys.
#[must_use]
pub fn describe_fetch_action(args: &Value) -> (String, &'static [&'static str]) {
    if let Some(url) = lookup_string(args, "url") {
        return (format!("Fetch {url}"), &["url"]);
    }
    ("Use Fetch".to_string(), &[])
}
