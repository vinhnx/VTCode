//! Pure string-parsing utilities for tool names.
//!
//! These functions parse and normalize tool name strings without depending
//! on any tool system types. They live in `utils` to break the circular
//! dependency between `tool_policy` and `tools::mcp`.
//!
//! # MCP Tool Name Format
//!
//! MCP tools use the canonical format `mcp::<provider>::<tool_name>`.
//! Legacy format is `mcp_<provider>_<tool_name>`.

use std::borrow::Cow;

use vtcode_commons::utils::calculate_sha256;

/// The MCP tool name prefix used in model-visible form.
pub const MCP_QUALIFIED_TOOL_PREFIX: &str = "mcp__";

/// Maximum length for MCP tool names before hashing.
const MCP_TOOL_NAME_MAX_LEN: usize = 64;

/// Length of the hash suffix used for truncated MCP tool names.
const MCP_HASH_SUFFIX_LEN: usize = 8;

/// Check if a tool name uses the legacy MCP format (`mcp_<provider>_<tool>`).
///
/// Legacy names start with `mcp_` but not `mcp__` (the qualified prefix).
pub fn is_legacy_mcp_tool_name(name: &str) -> bool {
    name.starts_with("mcp_") && !name.starts_with(MCP_QUALIFIED_TOOL_PREFIX)
}

/// Extract the tool name from a legacy MCP tool name.
///
/// For `mcp_<provider>_<tool>`, returns the part after `mcp_`.
pub fn legacy_mcp_tool_name(name: &str) -> Option<&str> {
    if is_legacy_mcp_tool_name(name) {
        name.strip_prefix("mcp_")
    } else {
        None
    }
}

/// Parse a canonical MCP tool name of the form `mcp::<provider>::<tool_name>`.
///
/// Returns `Some((provider, tool_name))` if the name matches the canonical format.
pub fn parse_canonical_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    let mut parts = name.splitn(3, "::");
    match (parts.next()?, parts.next(), parts.next()) {
        ("mcp", Some(provider), Some(tool)) if !provider.is_empty() && !tool.is_empty() => {
            Some((provider, tool))
        }
        _ => None,
    }
}

/// Build a model-visible MCP tool name from provider and tool name.
///
/// Uses the format `mcp__<provider>__<tool>`, truncated with a hash suffix
/// if the name exceeds the maximum length.
pub fn model_visible_mcp_tool_name(provider: &str, tool_name: &str) -> String {
    let provider = sanitize_tool_segment(provider);
    let tool = sanitize_tool_segment(tool_name);
    let qualified = format!("{MCP_QUALIFIED_TOOL_PREFIX}{provider}__{tool}");

    if qualified.len() <= MCP_TOOL_NAME_MAX_LEN {
        return qualified;
    }

    let hash = calculate_sha256(qualified.as_bytes());
    let hash = &hash[..MCP_HASH_SUFFIX_LEN];
    let keep = MCP_TOOL_NAME_MAX_LEN.saturating_sub(1 + MCP_HASH_SUFFIX_LEN);
    let prefix = &qualified[..keep];
    format!("{prefix}_{hash}")
}

fn sanitize_tool_segment(input: &str) -> Cow<'_, str> {
    if input.is_empty() {
        return Cow::Borrowed("tool");
    }

    let first_bad = input
        .bytes()
        .position(|b| !b.is_ascii_alphanumeric() && b != b'_' && b != b'-');

    match first_bad {
        None => Cow::Borrowed(input),
        Some(pos) => {
            let mut out = String::with_capacity(input.len());
            out.push_str(&input[..pos]);
            for ch in input[pos..].chars() {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                    out.push(ch);
                } else {
                    out.push('_');
                }
            }
            Cow::Owned(out)
        }
    }
}

/// Use direct tool name without alias resolution.
/// Alias resolution is now handled by the tool registry inventory
/// which maintains a mapping of aliases to canonical tool names.
pub const fn canonical_tool_name(name: &str) -> &str {
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical_mcp_tool_name_valid() {
        assert_eq!(
            parse_canonical_mcp_tool_name("mcp::github::list_issues"),
            Some(("github", "list_issues"))
        );
    }

    #[test]
    fn parse_canonical_mcp_tool_name_invalid() {
        assert_eq!(parse_canonical_mcp_tool_name("read_file"), None);
        assert_eq!(parse_canonical_mcp_tool_name("mcp::"), None);
        assert_eq!(parse_canonical_mcp_tool_name("mcp::provider"), None);
    }

    #[test]
    fn legacy_mcp_tool_name_basic() {
        assert_eq!(
            legacy_mcp_tool_name("mcp_github_list_issues"),
            Some("github_list_issues")
        );
        assert_eq!(legacy_mcp_tool_name("read_file"), None);
    }

    #[test]
    fn is_legacy_mcp_tool_name_detects_prefix() {
        assert!(is_legacy_mcp_tool_name("mcp_github_list"));
        assert!(!is_legacy_mcp_tool_name("mcp__github__list"));
        assert!(!is_legacy_mcp_tool_name("read_file"));
    }

    #[test]
    fn canonical_tool_name_is_identity() {
        assert_eq!(canonical_tool_name("list_files"), "list_files");
        assert_eq!(canonical_tool_name("unknown_tool"), "unknown_tool");
    }

    #[test]
    fn model_visible_mcp_tool_name_short() {
        let name = model_visible_mcp_tool_name("github", "list");
        assert_eq!(name, "mcp__github__list");
    }

    #[test]
    fn model_visible_mcp_tool_name_long_truncates() {
        let name = model_visible_mcp_tool_name(
            "very_long_provider_name",
            "very_long_tool_name_that_exceeds_limit",
        );
        assert!(name.len() <= MCP_TOOL_NAME_MAX_LEN);
        assert!(name.starts_with("mcp__"));
    }
}
