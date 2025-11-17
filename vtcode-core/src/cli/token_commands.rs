//! Token budget management commands

use crate::core::token_budget::TokenBudgetManager;
use std::sync::Arc;

// Import the TokenCommands enum
use super::args::TokenCommands;

// We need to get the actual token budget manager from the application context
// This is a simplified version - in the actual application this will come from the context
pub async fn handle_token_command_with_budget(
    command: &TokenCommands,
    token_budget: Arc<TokenBudgetManager>,
) -> Result<(), Box<dyn std::error::Error>> {
    match command {
        TokenCommands::Status => handle_status_command(token_budget).await?,
        TokenCommands::History => handle_history_command(token_budget).await?,
        TokenCommands::Summary => handle_summary_command(token_budget).await?,
    }

    Ok(())
}

// Default implementation that tries to get the global token budget manager, falling back to default
pub async fn handle_token_command(
    command: &TokenCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    // Try to get the global token budget manager that should be initialized by the agent
    let token_budget = crate::core::global_token_manager::get_global_token_budget()
        .unwrap_or_else(|| Arc::new(TokenBudgetManager::default()));

    handle_token_command_with_budget(command, token_budget).await
}

async fn handle_status_command(
    token_budget: Arc<TokenBudgetManager>,
) -> Result<(), Box<dyn std::error::Error>> {
    let report = token_budget.generate_report().await;
    println!("{}", report);
    Ok(())
}

async fn handle_history_command(
    token_budget: Arc<TokenBudgetManager>,
) -> Result<(), Box<dyn std::error::Error>> {
    let history = token_budget.get_recent_max_tokens_usage().await;
    if history.is_empty() {
        println!("No max_tokens usage recorded.");
    } else {
        println!("Recent max_tokens Usage History:");
        println!("================================");
        for (i, usage) in history.iter().enumerate().take(20) {
            let applied = usage
                .applied_max_tokens
                .map(|n| n.to_string())
                .unwrap_or_else(|| "None".to_string());
            let timestamp = chrono::DateTime::from_timestamp(usage.timestamp as i64, 0)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| usage.timestamp.to_string());
            println!(
                "{:2}. {} | Tool: {} | Applied: {} | Context: {}",
                i + 1,
                timestamp,
                usage.tool_name,
                applied,
                usage.context
            );
        }
    }
    Ok(())
}

async fn handle_summary_command(
    token_budget: Arc<TokenBudgetManager>,
) -> Result<(), Box<dyn std::error::Error>> {
    let summary = token_budget.get_max_tokens_usage_summary().await;
    println!("{}", summary);
    Ok(())
}
