use vtcode_core::mcp::cli::{LoginArgs, LogoutArgs, McpCommands, handle_mcp_command};

#[tokio::test]
async fn mcp_login_reports_not_supported() {
    let err = handle_mcp_command(McpCommands::Login(LoginArgs {
        name: "mock".to_string(),
    }))
    .await
    .expect_err("login should currently be unsupported");

    assert!(err.to_string().contains("not yet supported"));
}

#[tokio::test]
async fn mcp_logout_reports_not_supported() {
    let err = handle_mcp_command(McpCommands::Logout(LogoutArgs {
        name: "mock".to_string(),
    }))
    .await
    .expect_err("logout should currently be unsupported");

    assert!(err.to_string().contains("not yet supported"));
}
