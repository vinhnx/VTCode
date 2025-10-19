//! Demonstrates running the tool registry in a headless context with a custom
//! policy store and planning disabled for a lightweight deployment.
//!
//! ```bash
//! cargo run -p vtcode-tools --example headless_custom_policy \
//!     --no-default-features --features "policies"
//! ```
//!
//! The example executes `list_files` and `read_file` using policies persisted in
//! a workspace-local directory instead of the global `~/.vtcode` config.

use std::fs;

use anyhow::{Context, Result};
use serde_json::{json, to_string_pretty};
use tempfile::tempdir;
use vtcode_core::config::{PtyConfig, constants::tools};
use vtcode_tools::policies::{ToolPolicy, ToolPolicyManager};
use vtcode_tools::registry::ToolRegistry;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let workspace = tempdir().context("failed to create temporary workspace")?;
    let workspace_root = workspace.path().to_path_buf();

    let sample_file = workspace_root.join("README.md");
    fs::write(
        &sample_file,
        "# vtcode-tools headless demo\n\nPolicies live next to the project.\n",
    )
    .context("failed to write sample file")?;

    let policy_path = workspace_root.join("policies").join("tool-policy.json");
    let mut policy_manager = ToolPolicyManager::new_with_config_path(&policy_path)
        .context("failed to initialize custom policy manager")?;

    policy_manager
        .set_policy(tools::LIST_FILES, ToolPolicy::Allow)
        .context("failed to allow list_files")?;
    policy_manager
        .set_policy(tools::READ_FILE, ToolPolicy::Allow)
        .context("failed to allow read_file")?;

    let mut registry = ToolRegistry::new_with_custom_policy_and_config(
        workspace_root.clone(),
        PtyConfig::default(),
        false, // Disable planning to demonstrate lightweight adoption.
        policy_manager,
    );

    let available_tools = registry.available_tools();
    println!(
        "Registered tools (planning disabled): {}",
        available_tools.join(", ")
    );
    println!("Policies stored at: {}", policy_path.display());
    println!(
        "Planning tool registered? {}",
        available_tools
            .iter()
            .any(|tool| tool == tools::UPDATE_PLAN)
    );

    let listing = registry
        .execute_tool(
            tools::LIST_FILES,
            json!({
                "path": ".",
                "response_format": "concise",
            }),
        )
        .await?;
    println!("list_files response:\n{}", to_string_pretty(&listing)?);

    let read_file = registry
        .execute_tool(
            tools::READ_FILE,
            json!({
                "path": "README.md",
                "response_format": "text",
            }),
        )
        .await?;
    println!("read_file response:\n{}", to_string_pretty(&read_file)?);

    Ok(())
}
