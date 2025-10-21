//! Headless integration example for the `vtcode-tools` crate.
//!
//! This sample shows how an external application can:
//! 1. Store tool policy data in its own configuration hierarchy.
//! 2. Construct a `ToolRegistry` with that custom policy manager.
//! 3. Register and invoke a lightweight headless tool implementation.
//!
//! Run with minimal features enabled:
//! ```sh
//! cargo run -p vtcode-tools --example headless_registry --no-default-features --features "policies"
//! ```

#![cfg_attr(not(feature = "policies"), allow(dead_code))]

#[cfg(not(feature = "policies"))]
fn main() {
    panic!("Enable the `policies` feature to build the `headless_registry` example.");
}

#[cfg(feature = "policies")]
use anyhow::{Context, Result, anyhow};
#[cfg(feature = "policies")]
use async_trait::async_trait;
#[cfg(feature = "policies")]
use serde_json::{Value, json};
#[cfg(feature = "policies")]
use std::path::PathBuf;
#[cfg(feature = "policies")]
use tempfile::tempdir;
#[cfg(feature = "policies")]
use vtcode_commons::{
    DisplayErrorFormatter, NoopErrorReporter, NoopTelemetry, StaticWorkspacePaths,
};
#[cfg(feature = "policies")]
use vtcode_core::config::types::CapabilityLevel;
#[cfg(feature = "policies")]
use vtcode_tools::policies::ToolPolicyManager;
#[cfg(feature = "policies")]
use vtcode_tools::{Tool, ToolRegistration, ToolRegistry};

#[cfg(feature = "policies")]
struct EchoTool;

#[cfg(feature = "policies")]
impl EchoTool {
    const NAME: &'static str = "echo_text";
}

#[cfg(feature = "policies")]
#[async_trait]
impl Tool for EchoTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let text = args
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("`text` argument must be a string"))?;

        Ok(json!({
            "echo": text,
        }))
    }

    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn description(&self) -> &'static str {
        "Echoes the provided text without modification."
    }

    fn validate_args(&self, args: &Value) -> Result<()> {
        args.get("text")
            .and_then(Value::as_str)
            .map(|_| ())
            .ok_or_else(|| anyhow!("`text` argument is required"))
    }
}

#[cfg(feature = "policies")]
#[tokio::main]
async fn main() -> Result<()> {
    // Applications can keep workspace data wherever they like; a temporary
    // directory keeps the example self-contained.
    let workspace = tempdir().context("failed to allocate workspace")?;
    let workspace_root = PathBuf::from(workspace.path());

    // Store the policy file inside an arbitrary configuration tree that the
    // host application controls (no writes to ~/.vtcode).
    let config_dir = workspace_root.join("config");
    let policy_path = config_dir.join("tool-policy.json");
    // Wire the shared adapters so telemetry, error handling, and path resolution
    // can be provided by the host application.
    let workspace_paths = StaticWorkspacePaths::new(workspace_root.clone(), config_dir);
    let telemetry = NoopTelemetry;
    let error_reporter = NoopErrorReporter;
    let formatter = DisplayErrorFormatter;

    // Build the registry with workspace-aware adapters and register a headless
    // echo tool that simply returns its input.
    let mut registry =
        RegistryBuilder::new(&workspace_paths, &telemetry, &error_reporter, &formatter)
            .with_policy_path(&policy_path)
            .build()?;
    registry.register_tool(
        ToolRegistration::from_tool_instance(EchoTool::NAME, CapabilityLevel::Basic, EchoTool)
            .with_llm_visibility(false),
    )?;

    // Opt the example into "allow all" so execution never prompts. Downstream
    // applications can persist whatever defaults they need via the policy file.
    registry.allow_all_tools()?;

    let output = registry
        .execute_tool(EchoTool::NAME, json!({ "text": "hello from vtcode-tools" }))
        .await?;

    println!("Echo tool response: {}", output);
    println!(
        "Policy file stored at {} (managed independently of ~/.vtcode)",
        policy_path.display()
    );

    Ok(())
}
