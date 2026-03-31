use assert_fs::TempDir;
use std::fs;
use vtcode_core::mcp::cli::{
    LoginArgs, LogoutArgs, McpCommands, handle_mcp_command,
    set_global_config_path_override_for_tests,
};

struct ConfigOverrideGuard;

impl ConfigOverrideGuard {
    fn set(path: std::path::PathBuf) -> Self {
        set_global_config_path_override_for_tests(Some(path)).expect("set config override");
        Self
    }
}

impl Drop for ConfigOverrideGuard {
    fn drop(&mut self) {
        set_global_config_path_override_for_tests(None).expect("clear config override");
    }
}

#[tokio::test]
async fn mcp_login_reports_missing_provider() {
    let temp_dir = TempDir::new().expect("temp dir");
    let config_path = temp_dir.path().join("vtcode.toml");
    fs::write(&config_path, "").expect("write config");
    let _guard = ConfigOverrideGuard::set(config_path);

    let err = handle_mcp_command(McpCommands::Login(LoginArgs {
        name: "mock".to_string(),
    }))
    .await
    .expect_err("login should require a configured provider");

    assert!(
        err.to_string()
            .contains("No MCP provider named 'mock' found.")
    );
}

#[tokio::test]
async fn mcp_logout_reports_missing_provider() {
    let temp_dir = TempDir::new().expect("temp dir");
    let config_path = temp_dir.path().join("vtcode.toml");
    fs::write(&config_path, "").expect("write config");
    let _guard = ConfigOverrideGuard::set(config_path);

    let err = handle_mcp_command(McpCommands::Logout(LogoutArgs {
        name: "mock".to_string(),
    }))
    .await
    .expect_err("logout should require a configured provider");

    assert!(
        err.to_string()
            .contains("No MCP provider named 'mock' found.")
    );
}
