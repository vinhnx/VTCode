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
    let list_files_name = canonical_tool_name("list_files");
    let list_files: &str = list_files_name.as_ref();
    assert_eq!(list_files, "list_files");

    let unknown_tool_name = canonical_tool_name("unknown_tool");
    let unknown_tool: &str = unknown_tool_name.as_ref();
    assert_eq!(unknown_tool, "unknown_tool");

    let container_exec_name = canonical_tool_name("container.exec");
    let container_exec: &str = container_exec_name.as_ref();
    assert_eq!(container_exec, "container.exec");
}
