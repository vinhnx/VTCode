use std::borrow::Cow;

/// Normalize tool identifiers to their canonical registry names.
pub fn canonical_tool_name<'a>(name: &'a str) -> Cow<'a, str> {
    Cow::Borrowed(name)
}

/// Return known aliases for a canonical tool name.
pub fn tool_aliases(_name: &str) -> &'static [&'static str] {
    &[]
}
