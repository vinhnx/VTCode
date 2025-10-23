use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::Mutex;

use vtcode_core::mcp_client::{
    ElicitationAction, McpElicitationHandler, McpElicitationRequest, McpElicitationResponse,
};

/// Interactive handler that prompts the user on the terminal when an MCP provider
/// requests additional input via the elicitation flow.
pub struct InteractiveMcpElicitationHandler {
    prompt_lock: Mutex<()>,
}

impl InteractiveMcpElicitationHandler {
    pub fn new() -> Self {
        Self {
            prompt_lock: Mutex::new(()),
        }
    }
}

#[async_trait]
impl McpElicitationHandler for InteractiveMcpElicitationHandler {
    async fn handle_elicitation(
        &self,
        provider: &str,
        request: McpElicitationRequest,
    ) -> Result<McpElicitationResponse> {
        use std::io::{self, Write};

        let _guard = self.prompt_lock.lock().await;

        println!();
        println!("=== MCP elicitation request from '{}' ===", provider);
        println!("{}", request.message);

        if !request.requested_schema.is_null() {
            let schema_display = serde_json::to_string_pretty(&request.requested_schema)
                .context("Failed to format MCP elicitation schema")?;
            println!("Expected response schema:\n{}", schema_display);
        }

        println!(
            "Enter JSON to accept, press Enter to decline, or type 'cancel' to cancel the operation."
        );
        print!("Response> ");
        io::stdout().flush().ok();

        let input = tokio::task::spawn_blocking(|| {
            let mut buffer = String::new();
            io::stdin().read_line(&mut buffer).map(|_| buffer)
        })
        .await
        .context("Failed to read elicitation response")??;

        let trimmed = input.trim();

        if trimmed.eq_ignore_ascii_case("cancel") {
            println!("Cancelling elicitation request from '{}'.", provider);
            return Ok(McpElicitationResponse {
                action: ElicitationAction::Cancel,
                content: None,
            });
        }

        if trimmed.is_empty() {
            println!("Declining elicitation request from '{}'.", provider);
            return Ok(McpElicitationResponse {
                action: ElicitationAction::Decline,
                content: None,
            });
        }

        let content: Value =
            serde_json::from_str(trimmed).context("Elicitation response must be valid JSON")?;

        println!("Submitting elicitation response to '{}'.", provider);

        Ok(McpElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(content),
        })
    }
}
