//! Validate command implementation - environment and configuration validation

use crate::config::constants::tools;
use crate::config::types::AgentConfig;
use crate::prompts::read_system_prompt_from_md;
use crate::tools::ToolRegistry;
use crate::utils::colors::style;
use anyhow::Result;
use serde_json::json;

/// Handle the validate command - check environment and configuration
pub async fn handle_validate_command(
    config: AgentConfig,
    check_api: bool,
    check_filesystem: bool,
    _check_tools: bool,
    _check_config: bool,
    all: bool,
) -> Result<()> {
    println!(
        "{}",
        style(" Validating environment and configuration...")
            .cyan()
            .bold()
    );

    let mut all_checks = true;

    // Check API connectivity if requested
    if check_api || all {
        println!("{}", style("Checking API connectivity...").dim());
        match check_api_connectivity(&config).await {
            Ok(_) => println!("  {} API connectivity OK", style("✓").green()),
            Err(e) => {
                println!("  {} API connectivity failed: {}", style("✗").red(), e);
                all_checks = false;
            }
        }
    }

    // Check filesystem permissions if requested
    if check_filesystem || all {
        println!("{}", style("Checking filesystem permissions...").dim());
        match check_filesystem_permissions(&config).await {
            Ok(_) => println!("  {} Filesystem permissions OK", style("✓").green()),
            Err(e) => {
                println!("  {} Filesystem permissions issue: {}", style("✗").red(), e);
                all_checks = false;
            }
        }
    }

    // Summary
    if all_checks {
        println!("{}", style("All validation checks passed!").green().bold());
    } else {
        println!(
            "{}",
            style(" Some validation checks failed.").yellow().bold()
        );
        println!("{}", style("Please address the issues above.").dim());
    }

    Ok(())
}

/// Check API connectivity
async fn check_api_connectivity(config: &AgentConfig) -> Result<()> {
    use crate::gemini::models::SystemInstruction;
    use crate::gemini::{Client, Content, GenerateContentRequest};
    use crate::prompts::generate_lightweight_instruction;

    let mut client = Client::new(config.api_key.clone(), config.model.clone());
    let contents = vec![Content::user_text("Hello")];
    let lightweight_instruction = generate_lightweight_instruction();

    // Convert Content to SystemInstruction
    let system_instruction = if let Some(part) = lightweight_instruction.parts.first() {
        if let Some(text) = part.as_text() {
            SystemInstruction::new(text)
        } else {
            SystemInstruction::new(
                read_system_prompt_from_md()
                    .await
                    .unwrap_or_else(|_| "You are a helpful coding assistant.".to_string()),
            )
        }
    } else {
        SystemInstruction::new(
            read_system_prompt_from_md()
                .await
                .unwrap_or_else(|_| "You are a helpful coding assistant.".to_string()),
        )
    };

    let request = GenerateContentRequest {
        contents,
        tools: None,
        tool_config: None,
        generation_config: Some(json!({
            "maxOutputTokens": 10,
            "temperature": 0.1
        })),
        system_instruction: Some(system_instruction),
        reasoning_config: None,
    };

    client.generate(&request).await?;
    Ok(())
}

/// Check filesystem permissions
async fn check_filesystem_permissions(config: &AgentConfig) -> Result<()> {
    let mut registry = ToolRegistry::new(config.workspace.clone()).await;

    // Try to list files in the workspace
    registry
        .execute_tool(tools::LIST_FILES, json!({"path": ".", "max_items": 5}))
        .await?;

    // Try to create a test file
    registry
        .execute_tool(
            tools::WRITE_FILE,
            json!({
                "path": ".vtcode_test",
                "content": "test",
                "overwrite": true
            }),
        )
        .await?;

    // Clean up test file
    // Delete is supported via delete_file tool in ToolRegistry; we still validate permissions here

    Ok(())
}
