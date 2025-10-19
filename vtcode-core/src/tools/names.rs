use std::borrow::Cow;

use crate::config::constants::tools;

const RP_SEARCH_ALIASES: &[&str] = &[tools::GREP_SEARCH_LEGACY];

/// Normalize tool identifiers to their canonical registry names.
pub fn canonical_tool_name<'a>(name: &'a str) -> Cow<'a, str> {
    match name {
        tools::GREP_SEARCH_LEGACY => Cow::Borrowed(tools::GREP_SEARCH),
        _ => Cow::Borrowed(name),
    }
}

/// Return known aliases for a canonical tool name.
pub fn tool_aliases(name: &str) -> &'static [&'static str] {
    match name {
        tools::GREP_SEARCH => RP_SEARCH_ALIASES,
        _ => &[],
    }
}
