use anyhow::Result;
use serde_json::json;
use tempfile::tempdir;
use vtcode_core::config::constants::tools;
use vtcode_core::tools::ToolRegistry;

#[tokio::test]
async fn run_command_uses_pty_backend() -> Result<()> {
    let temp_dir = tempdir()?;
    std::fs::write(temp_dir.path().join("sample.txt"), "hello")?;

    let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let response = registry
        .execute_tool(
            tools::RUN_PTY_CMD,
            json!({
                "command": "ls",
                "working_dir": "."
            }),
        )
        .await?;
    // debug: response logged in test harness if needed

    assert_eq!(response["success"], true);
    assert_eq!(response["mode"], "terminal");
    assert_eq!(response["pty_enabled"], true);

    let stdout = response["stdout"].as_str().unwrap_or_default();
    assert!(stdout.contains("sample.txt"));

    Ok(())
}

#[tokio::test]
async fn run_command_accepts_indexed_arguments_zero_based() -> Result<()> {
    let temp_dir = tempdir()?;
    std::fs::write(temp_dir.path().join("sample.txt"), "hello")?;

    let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let response = registry
        .execute_tool(
            tools::RUN_PTY_CMD,
            json!({
                "command.0": "ls",
                "command.1": "-a",
                "working_dir": "."
            }),
        )
        .await?;

    assert_eq!(response["success"], true);
    let stdout = response["stdout"].as_str().unwrap_or_default();
    assert!(stdout.contains("sample.txt"));

    Ok(())
}

#[tokio::test]
async fn run_command_accepts_indexed_arguments_one_based() -> Result<()> {
    let temp_dir = tempdir()?;
    std::fs::write(temp_dir.path().join("sample2.txt"), "hello2")?;

    let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let response = registry
        .execute_tool(
            tools::RUN_PTY_CMD,
            json!({
                "command.1": "ls",
                "command.2": "-a",
                "working_dir": "."
            }),
        )
        .await?;

    assert_eq!(response["success"], true);
    let stdout = response["stdout"].as_str().unwrap_or_default();
    assert!(stdout.contains("sample2.txt"));

    Ok(())
}
