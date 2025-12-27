use std::borrow::Cow;

/// Use direct tool name without alias resolution.
/// Alias resolution is now handled by the tool registry inventory
/// which maintains a mapping of aliases to canonical tool names.
pub fn canonical_tool_name<'a>(name: &'a str) -> Cow<'a, str> {
    Cow::Borrowed(name)
}

#[test]
fn test_canonical_tool_name_passes_through() {
    // With registration-based aliases, this function now just passes through
    // Alias resolution happens earlier in the inventory layer
    assert_eq!(canonical_tool_name("list_files").as_ref(), "list_files");
    assert_eq!(canonical_tool_name("unknown_tool").as_ref(), "unknown_tool");
    assert_eq!(canonical_tool_name("container.exec").as_ref(), "container.exec");
}
