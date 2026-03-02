use anyhow::Result;
use serde_json::json;
use vtcode_core::config::constants::tools;

mod support;
use support::TestHarness;

#[tokio::test]
async fn run_command_uses_pty_backend() -> Result<()> {
    let harness = TestHarness::new()?;
    harness.write_file("sample.txt", "hello")?;
    let registry = harness.registry().await;

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
    // Check for PTY-related fields (implementation may vary)
    let has_pty_indicators = response.get("session_id").is_some()
        || response.get("process_id").is_some()
        || response.get("pty_enabled").is_some()
        || response.get("mode").is_some();
    assert!(
        has_pty_indicators,
        "Response should have PTY-related fields. Response: {:?}",
        response
    );

    let output = response["output"].as_str().unwrap_or_default();
    let stdout = response["stdout"].as_str().unwrap_or_default();
    let combined_output = format!("{} {}", output, stdout);
    assert!(
        combined_output.contains("sample.txt"),
        "Output should contain sample.txt. output='{}', stdout='{}'",
        output,
        stdout
    );

    Ok(())
}

#[tokio::test]
async fn run_command_accepts_indexed_arguments_zero_based() -> Result<()> {
    let harness = TestHarness::new()?;
    harness.write_file("sample.txt", "hello")?;
    let registry = harness.registry().await;

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

    eprintln!("Response: {:?}", response);
    assert_eq!(response["success"], true);
    let output = response["output"].as_str().unwrap_or_default();
    let stdout = response["stdout"].as_str().unwrap_or_default();
    let combined_output = format!("{} {}", output, stdout);
    assert!(
        combined_output.contains("sample.txt"),
        "Output should contain sample.txt. output='{}', stdout='{}'",
        output,
        stdout
    );

    Ok(())
}

#[tokio::test]
async fn run_command_accepts_indexed_arguments_one_based() -> Result<()> {
    let harness = TestHarness::new()?;
    harness.write_file("sample2.txt", "hello2")?;
    let registry = harness.registry().await;

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
    let output = response["output"].as_str().unwrap_or_default();
    let stdout = response["stdout"].as_str().unwrap_or_default();
    let combined_output = format!("{} {}", output, stdout);
    assert!(
        combined_output.contains("sample2.txt"),
        "Output should contain sample2.txt. output='{}', stdout='{}'",
        output,
        stdout
    );

    Ok(())
}
