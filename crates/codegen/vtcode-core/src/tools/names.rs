// Re-export from shared utils to break the tool_policy <-> tools cycle.
pub use crate::utils::tool_name_parsing::canonical_tool_name;

#[test]
fn test_canonical_tool_name_passes_through() {
    // With registration-based aliases, this function now just passes through
    // Alias resolution happens earlier in the inventory layer
    assert_eq!(canonical_tool_name("list_files"), "list_files");

    assert_eq!(canonical_tool_name("unknown_tool"), "unknown_tool");

    assert_eq!(canonical_tool_name("container.exec"), "container.exec");
}
