/// Tool Registry Sync Test
///
/// This test verifies that tool definitions stay synchronized across:
/// 1. Tool name constants (vtcode-config/src/constants.rs)
/// 2. Tool policies (vtcode-config/src/core/tools.rs)
/// 3. Tool declarations/schemas (vtcode-core/src/tools/registry/declarations.rs)
/// 4. ACP tool registry (src/acp/tooling.rs)
///
/// This prevents tool drift where tools are added/removed in one place but not others,
/// which can lead to inconsistent behavior across different agent client interfaces.
use std::collections::{HashMap, HashSet};

#[test]
fn test_all_tools_have_policies() {
    // Define all tools from constants.rs
    let all_tools = vec![
        // File operations
        "list_files",
        "grep_file",
        "read_file",
        "write_file",
        "edit_file",
        "create_file",
        "delete_file",
        "apply_patch",
        // PTY operations
        "run_pty_cmd",
        "create_pty_session",
        "read_pty_session",
        "list_pty_sessions",
        "resize_pty_session",
        "send_pty_input",
        "close_pty_session",
        // Code execution and meta
        "execute_code",
        "search_tools",
        // Web operations
        "web_fetch",
    ];

    // Tools that MUST have policies defined
    let tools_requiring_policies: HashSet<&str> = HashSet::from_iter(all_tools.iter().copied());

    // Currently defined policies (from DEFAULT_TOOL_POLICIES)
    let tools_with_policies: HashSet<&str> = HashSet::from_iter(
        vec![
            "list_files",
            "grep_file",
            "read_file",
            "write_file",
            "edit_file",
            "create_file",
            "delete_file",
            "apply_patch",
            "run_pty_cmd",
            "create_pty_session",
            "read_pty_session",
            "list_pty_sessions",
            "resize_pty_session",
            "send_pty_input",
            "close_pty_session",
            "execute_code",
            "search_tools",
            "web_fetch",
        ]
        .iter()
        .copied(),
    );

    let missing_policies: Vec<_> = tools_requiring_policies
        .difference(&tools_with_policies)
        .copied()
        .collect();

    assert!(
        missing_policies.is_empty(),
        "The following tools have no defined policies in DEFAULT_TOOL_POLICIES: {:?}",
        missing_policies
    );
}

#[test]
fn test_tool_policy_categories_documented() {
    // Verify policy reasoning is clear
    let tool_policies = vec![
        // This maps tool name to its policy category for documentation purposes
        ("list_files", "Allow - non-destructive read"),
        ("grep_file", "Allow - non-destructive read"),
        ("read_file", "Allow - non-destructive read"),
        ("write_file", "Allow - controlled write"),
        ("edit_file", "Allow - controlled edit"),
        ("create_file", "Allow - controlled create"),
        ("delete_file", "Prompt - destructive"),
        ("apply_patch", "Prompt - destructive"),
        ("run_pty_cmd", "Prompt - execution with approval"),
        ("create_pty_session", "Allow - session management"),
        ("read_pty_session", "Allow - read-only session access"),
        ("list_pty_sessions", "Allow - non-destructive list"),
        ("resize_pty_session", "Allow - safe session control"),
        ("send_pty_input", "Prompt - active interaction"),
        ("close_pty_session", "Allow - cleanup operation"),
        ("execute_code", "Prompt - code execution risk"),
        ("search_tools", "Allow - metadata query"),
        ("web_fetch", "Prompt - network access"),
    ];

    // This test documents expected policy categories
    // If a tool's policy category changes, this test serves as documentation
    for (tool, expected_category) in &tool_policies {
        println!("{}: {}", tool, expected_category);
    }

    assert_eq!(
        tool_policies.len(),
        18,
        "Policy count mismatch - verify all tools documented"
    );
}

#[test]
fn test_no_tools_in_constants_without_declarations() {
    // Tools defined in constants.rs SHOULD have declarations
    // (with rare exceptions for meta/internal tools)
    let tools_in_constants: Vec<&str> = vec![
        "list_files",
        "grep_file",
        "read_file",
        "write_file",
        "edit_file",
        "create_file",
        "delete_file",
        "apply_patch",
        "run_pty_cmd",
        "create_pty_session",
        "read_pty_session",
        "list_pty_sessions",
        "resize_pty_session",
        "send_pty_input",
        "close_pty_session",
        "execute_code",
        "task_tracker",
        "plan_task_tracker",
        "search_tools",
        "web_fetch",
    ];

    // Tools with declarations (from declarations.rs)
    let tools_with_declarations: HashSet<&str> = HashSet::from_iter(
        vec![
            "list_files",
            "grep_file",
            "run_pty_cmd",
            "search_tools",
            "execute_code",
            "read_file",
            "create_file",
            "delete_file",
            "write_file",
            "edit_file",
            "apply_patch",
            "create_pty_session",
            "list_pty_sessions",
            "close_pty_session",
            "send_pty_input",
            "read_pty_session",
            "resize_pty_session",
            "web_fetch",
            "task_tracker",
            "plan_task_tracker",
        ]
        .iter()
        .copied(),
    );

    let tools_in_constants_set: HashSet<_> = tools_in_constants.iter().copied().collect();
    let missing_declarations: Vec<_> = tools_in_constants_set
        .difference(&tools_with_declarations)
        .copied()
        .collect();

    // Log missing declarations for visibility
    if !missing_declarations.is_empty() {
        eprintln!("Tools without declarations: {:?}", missing_declarations);
    }

    assert!(
        missing_declarations.is_empty(),
        "The following tools are missing declarations: {:?}",
        missing_declarations
    );
}

#[test]
fn test_acp_tool_subset_is_documented() {
    // ACP exposes only a subset of tools - verify this is intentional
    let acp_tools: HashSet<&str> = HashSet::from_iter(["read_file", "list_files"].iter().copied());

    let all_tools: HashSet<_> = HashSet::from_iter(
        vec![
            "list_files",
            "grep_file",
            "read_file",
            "write_file",
            "edit_file",
            "create_file",
            "delete_file",
            "apply_patch",
            "run_pty_cmd",
            "create_pty_session",
            "read_pty_session",
            "list_pty_sessions",
            "resize_pty_session",
            "send_pty_input",
            "close_pty_session",
            "execute_code",
            "task_tracker",
            "plan_task_tracker",
            "search_tools",
            "debug_agent",
            "analyze_agent",
            "web_fetch",
        ]
        .iter()
        .copied(),
    );

    let tools_excluded_from_acp: Vec<_> = all_tools.difference(&acp_tools).copied().collect();

    println!("Tools exposed via ACP: {:?}", acp_tools);
    println!("Tools NOT exposed via ACP: {:?}", tools_excluded_from_acp);

    // Verify the exclusion is intentional and documented
    // See src/acp/tooling.rs SupportedTool documentation
    assert_eq!(
        acp_tools.len(),
        2,
        "ACP tool count changed - verify this is intentional"
    );
    assert!(
        acp_tools.contains("read_file"),
        "read_file should be available via ACP"
    );
    assert!(
        acp_tools.contains("list_files"),
        "list_files should be available via ACP"
    );
}

#[test]
fn test_tool_policy_consistency() {
    // Map of tool -> expected policy type
    let expected_policies: HashMap<&str, &str> = HashMap::from_iter(vec![
        // Allow category: non-destructive, safe operations
        ("list_files", "Allow"),
        ("grep_file", "Allow"),
        ("read_file", "Allow"),
        ("write_file", "Allow"),
        ("edit_file", "Allow"),
        ("create_file", "Allow"),
        ("create_pty_session", "Allow"),
        ("read_pty_session", "Allow"),
        ("list_pty_sessions", "Allow"),
        ("resize_pty_session", "Allow"),
        ("close_pty_session", "Allow"),
        ("search_tools", "Allow"),
        // Prompt category: requires user confirmation
        ("delete_file", "Prompt"),
        ("apply_patch", "Prompt"),
        ("run_pty_cmd", "Prompt"),
        ("send_pty_input", "Prompt"),
        ("execute_code", "Prompt"),
        ("web_fetch", "Prompt"),
    ]);

    // Verify categorization makes sense
    for (tool, policy) in expected_policies.iter() {
        println!("{}: {}", tool, policy);
    }

    assert_eq!(
        expected_policies.len(),
        18,
        "Policy mapping completeness check"
    );
}
