//! MCP tool-name parsing and building.
//!
//! Pure string functions over tool name spellings, with no dependency on the
//! tool system. This is the single home for MCP name shapes so the agent
//! runtime, policy gates, and transcript display never fork the parsing.
//!
//! # MCP Tool Name Format
//!
//! Canonical form is `mcp::<provider>::<tool_name>`.
//! Model-visible form is `mcp__<provider>__<tool>`.
//! Legacy form is `mcp_<provider>_<tool_name>`.

use std::borrow::Cow;

use crate::utils::calculate_sha256;

/// The MCP tool name prefix used in model-visible form.
pub const MCP_QUALIFIED_TOOL_PREFIX: &str = "mcp__";

/// Maximum length for MCP tool names before hashing.
const MCP_TOOL_NAME_MAX_LEN: usize = 64;

/// Length of the hash suffix used for truncated MCP tool names.
const MCP_HASH_SUFFIX_LEN: usize = 8;

/// Check if a tool name uses the legacy MCP format (`mcp_<provider>_<tool>`).
///
/// Legacy names start with `mcp_` but not `mcp__` (the qualified prefix).
#[must_use]
pub fn is_legacy_mcp_tool_name(name: &str) -> bool {
    name.starts_with("mcp_") && !name.starts_with(MCP_QUALIFIED_TOOL_PREFIX)
}

/// Extract the tool name from a legacy MCP tool name.
///
/// For `mcp_<provider>_<tool>`, returns the part after `mcp_`.
#[must_use]
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
/// A tool segment containing further `::` separators is kept whole.
#[must_use]
pub fn parse_canonical_mcp_tool_name(name: &str) -> Option<(&str, &str)> {
    let mut parts = name.splitn(3, "::");
    match (parts.next()?, parts.next(), parts.next()) {
        ("mcp", Some(provider), Some(tool)) if !provider.is_empty() && !tool.is_empty() => Some((provider, tool)),
        _ => None,
    }
}

/// Build a model-visible MCP tool name from provider and tool name.
///
/// Uses the format `mcp__<provider>__<tool>`, truncated with a hash suffix
/// if the name exceeds the maximum length.
#[expect(
    clippy::string_slice,
    reason = "Slices land on proven ASCII: the hash is lowercase hex and the qualified name is built from sanitized ASCII-only segments."
)]
#[must_use]
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

#[expect(
    clippy::string_slice,
    reason = "`pos` is the first byte failing an ASCII-class test, so every preceding byte is a complete ASCII char and `pos` is a char boundary."
)]
fn sanitize_tool_segment(input: &str) -> Cow<'_, str> {
    if input.is_empty() {
        return Cow::Borrowed("tool");
    }

    let first_bad = input.bytes().position(|b| !b.is_ascii_alphanumeric() && b != b'_' && b != b'-');

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_canonical_mcp_tool_name_valid() {
        assert_eq!(parse_canonical_mcp_tool_name("mcp::github::list_issues"), Some(("github", "list_issues")));
    }

    #[test]
    fn parse_canonical_mcp_tool_name_invalid() {
        assert_eq!(parse_canonical_mcp_tool_name("read_file"), None);
        assert_eq!(parse_canonical_mcp_tool_name("mcp::"), None);
        assert_eq!(parse_canonical_mcp_tool_name("mcp::provider"), None);
    }

    #[test]
    fn parse_canonical_mcp_tool_name_keeps_remainder_in_tool() {
        assert_eq!(parse_canonical_mcp_tool_name("mcp::a::b::c"), Some(("a", "b::c")));
    }

    #[test]
    fn legacy_mcp_tool_name_basic() {
        assert_eq!(legacy_mcp_tool_name("mcp_github_list_issues"), Some("github_list_issues"));
        assert_eq!(legacy_mcp_tool_name("read_file"), None);
    }

    #[test]
    fn is_legacy_mcp_tool_name_detects_prefix() {
        assert!(is_legacy_mcp_tool_name("mcp_github_list"));
        assert!(!is_legacy_mcp_tool_name("mcp__github__list"));
        assert!(!is_legacy_mcp_tool_name("read_file"));
    }

    #[test]
    fn legacy_prefix_edges() {
        assert!(is_legacy_mcp_tool_name("mcp_"));
        assert_eq!(legacy_mcp_tool_name("mcp_"), Some(""));
        assert!(!is_legacy_mcp_tool_name("mcp"));
        assert!(!is_legacy_mcp_tool_name("mcp__"));
    }

    #[test]
    fn model_visible_mcp_tool_name_short() {
        let name = model_visible_mcp_tool_name("github", "list");
        assert_eq!(name, "mcp__github__list");
    }

    #[test]
    fn model_visible_mcp_tool_name_long_truncates() {
        let name = model_visible_mcp_tool_name("very_long_provider_name", "very_long_tool_name_that_exceeds_limit");
        assert!(name.len() <= MCP_TOOL_NAME_MAX_LEN);
        assert!(name.starts_with("mcp__"));
    }

    #[test]
    fn model_visible_mcp_tool_name_at_limit_is_unchanged() {
        let provider = "p".repeat(28);
        let tool = "t".repeat(29);
        let expected = format!("mcp__{provider}__{tool}");
        assert_eq!(expected.len(), MCP_TOOL_NAME_MAX_LEN);
        assert_eq!(model_visible_mcp_tool_name(&provider, &tool), expected);
    }

    #[test]
    fn model_visible_mcp_tool_name_empty_segments_default() {
        assert_eq!(model_visible_mcp_tool_name("", ""), "mcp__tool__tool");
    }

    #[test]
    fn model_visible_mcp_tool_name_sanitizes_unicode() {
        assert_eq!(model_visible_mcp_tool_name("githüb", "lïst"), "mcp__gith_b__l_st");
    }

    #[test]
    fn model_visible_mcp_tool_name_is_deterministic_and_order_sensitive() {
        let ab_c = model_visible_mcp_tool_name("ab", "c");
        let a_bc = model_visible_mcp_tool_name("a", "bc");
        assert_ne!(ab_c, a_bc);
        assert_eq!(ab_c, model_visible_mcp_tool_name("ab", "c"));
    }
}
