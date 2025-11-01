//! CLI commands for managing tool policies

use crate::tool_policy::{ToolPolicy, ToolPolicyManager};
use crate::utils::colors::style;
use anyhow::Result;
use clap::Subcommand;

/// Tool policy management commands
#[derive(Debug, Clone, Subcommand)]
pub enum ToolPolicyCommands {
    /// Show current tool policy status
    Status,
    /// Allow a specific tool
    Allow {
        /// Tool name to allow
        tool: String,
    },
    /// Deny a specific tool
    Deny {
        /// Tool name to deny
        tool: String,
    },
    /// Set a tool to prompt for confirmation
    Prompt {
        /// Tool name to set to prompt
        tool: String,
    },
    /// Allow all tools
    AllowAll,
    /// Deny all tools
    DenyAll,
    /// Reset all tools to prompt
    ResetAll,
}

/// Handle tool policy commands
pub async fn handle_tool_policy_command(command: ToolPolicyCommands) -> Result<()> {
    let mut policy_manager = ToolPolicyManager::new().await?;

    match command {
        ToolPolicyCommands::Status => {
            policy_manager.print_status();
        }
        ToolPolicyCommands::Allow { tool } => {
            policy_manager.set_policy(&tool, ToolPolicy::Allow).await?;
            println!(
                "{}",
                style(format!("✓ Tool '{}' is now allowed", tool)).green()
            );
        }
        ToolPolicyCommands::Deny { tool } => {
            policy_manager.set_policy(&tool, ToolPolicy::Deny).await?;
            println!(
                "{}",
                style(format!("✗ Tool '{}' is now denied", tool)).red()
            );
        }
        ToolPolicyCommands::Prompt { tool } => {
            policy_manager.set_policy(&tool, ToolPolicy::Prompt).await?;
            println!(
                "{}",
                style(format!(
                    "? Tool '{}' will now prompt for confirmation",
                    tool
                ))
                .yellow()
            );
        }
        ToolPolicyCommands::AllowAll => {
            policy_manager.allow_all_tools().await?;
            println!("{}", style("✓ All tools are now allowed").green());
        }
        ToolPolicyCommands::DenyAll => {
            policy_manager.deny_all_tools().await?;
            println!("{}", style("✗ All tools are now denied").red());
        }
        ToolPolicyCommands::ResetAll => {
            policy_manager.reset_all_to_prompt().await?;
            println!(
                "{}",
                style("? All tools reset to prompt for confirmation").yellow()
            );
        }
    }

    Ok(())
}

/// Print tool policy help
pub fn print_tool_policy_help() {
    println!("{}", style("Tool Policy Management").cyan().bold());
    println!();
    println!("Tool policies control which tools the agent can use:");
    println!();
    println!(
        "  {} - Tool executes automatically without prompting",
        style("allow").green()
    );
    println!(
        "  {} - Tool prompts for user confirmation each time",
        style("prompt").yellow()
    );
    println!(
        "  {} - Tool is never allowed to execute",
        style("deny").red()
    );
    println!();
    println!("Policies are stored in ~/.vtcode/tool-policy.json");
    println!("Once you approve or deny a tool, your choice is remembered for future runs.");
    println!();
    println!("Examples:");
    println!("  vtcode tool-policy status           # Show current policies");
    println!("  vtcode tool-policy allow read_file  # Allow read_file tool");
    println!("  vtcode tool-policy deny rm          # Deny rm tool");
    println!("  vtcode tool-policy reset-all        # Reset all to prompt");
}
