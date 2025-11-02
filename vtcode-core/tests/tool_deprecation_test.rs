use tempfile::TempDir;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::registry::ToolRegistry;

#[tokio::test]
async fn test_deprecated_tool_still_available() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    // Verify run_terminal_cmd is still registered and available
    assert!(
        registry.has_tool(tools::RUN_COMMAND).await,
        "run_terminal_cmd should still be available (deprecated but not removed)"
    );
}

#[tokio::test]
async fn test_pty_session_tools_available() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    // Verify PTY session tools are available as replacements
    let pty_tools = vec![
        tools::CREATE_PTY_SESSION,
        tools::SEND_PTY_INPUT,
        tools::READ_PTY_SESSION,
        tools::CLOSE_PTY_SESSION,
        tools::LIST_PTY_SESSIONS,
        tools::RESIZE_PTY_SESSION,
    ];

    for tool_name in pty_tools {
        assert!(
            registry.has_tool(tool_name).await,
            "PTY tool {} should be available",
            tool_name
        );
    }
}

#[tokio::test]
async fn test_function_declarations_include_deprecated() {
    use vtcode_core::tools::registry::build_function_declarations;

    let declarations = build_function_declarations();

    // Verify run_terminal_cmd is in declarations
    let run_command_decl = declarations.iter().find(|d| d.name == tools::RUN_COMMAND);

    assert!(
        run_command_decl.is_some(),
        "run_terminal_cmd should be in function declarations"
    );

    // Verify description indicates deprecation
    let description = &run_command_decl.unwrap().description;
    assert!(
        description.contains("DEPRECATED") || description.contains("deprecated"),
        "Description should indicate deprecation: {}",
        description
    );
}
