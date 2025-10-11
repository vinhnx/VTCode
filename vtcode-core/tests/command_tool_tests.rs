use anyhow::Result;
use serde_json::json;
use vtcode_core::config::core::commands::CommandsConfig;
use vtcode_core::tools::command::CommandTool;

#[tokio::test]
async fn command_tool_splits_single_string_command() -> Result<()> {
    let tool = CommandTool::new(std::env::current_dir()?, CommandsConfig::default());
    let result = tool
        .execute(json!({
            "command": ["printf ready"],
        }))
        .await?;

    assert!(
        result
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    );
    assert_eq!(
        result.get("stdout").and_then(|v| v.as_str()).unwrap_or(""),
        "ready"
    );

    Ok(())
}
