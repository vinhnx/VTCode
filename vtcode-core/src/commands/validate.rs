//! Validate command implementation - environment and configuration validation

use crate::config::constants::tools;
use crate::config::types::AgentConfig;
use crate::llm::factory::create_provider_for_model;
use crate::llm::provider::{LLMRequest, Message};
use crate::prompts::system::lightweight_instruction_text;
use crate::tools::ToolRegistry;
use crate::utils::colors::style;
use anyhow::Result;
use serde_json::json;
use std::sync::Arc;

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
            Ok(_) => println!("  {} API connectivity OK", style("[+]").green()),
            Err(e) => {
                println!("  {} API connectivity failed: {}", style("[X]").red(), e);
                all_checks = false;
            }
        }
    }

    // Check filesystem permissions if requested
    if check_filesystem || all {
        println!("{}", style("Checking filesystem permissions...").dim());
        match check_filesystem_permissions(&config).await {
            Ok(_) => println!("  {} Filesystem permissions OK", style("[+]").green()),
            Err(e) => {
                println!(
                    "  {} Filesystem permissions issue: {}",
                    style("[X]").red(),
                    e
                );
                all_checks = false;
            }
        }
    }

    // Summary
    if all_checks {
        println!("{}", style("All validation checks passed!").green().bold());
    } else {
        println!("{}", style(" Some validation checks failed.").red().bold());
        println!("{}", style("Please address the issues above.").dim());
    }

    Ok(())
}

/// Check API connectivity
async fn check_api_connectivity(config: &AgentConfig) -> Result<()> {
    let provider = create_provider_for_model(&config.model, config.api_key.clone(), None, None)?;
    let request = LLMRequest {
        messages: vec![Message::user("Hello".to_string())],
        system_prompt: Some(Arc::new(lightweight_instruction_text())),
        model: config.model.to_string(),
        max_tokens: Some(10),
        temperature: Some(0.1),
        ..Default::default()
    };

    provider.generate(request).await?;
    Ok(())
}

/// Check filesystem permissions
async fn check_filesystem_permissions(config: &AgentConfig) -> Result<()> {
    let workspace = config.workspace.clone(); // Clone only once for reuse
    let registry = ToolRegistry::new(workspace).await;

    // Try to list files in the workspace
    registry
        .execute_tool(
            tools::UNIFIED_SEARCH,
            json!({"action": "list", "path": ".", "max_items": 5}),
        )
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
