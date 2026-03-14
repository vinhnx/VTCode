use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::{Arc, RwLock as StdRwLock};
use tokio::sync::Mutex;

use vtcode_core::mcp::{
    ElicitationAction, McpElicitationHandler, McpElicitationRequest, McpElicitationResponse,
};
use vtcode_core::{
    NotificationEvent, exec_policy::AskForApproval, send_global_notification,
    utils::ansi_codes::notify_attention,
};

/// Interactive handler that prompts the user on the terminal when an MCP provider
/// requests additional input via the elicitation flow.
pub(crate) struct InteractiveMcpElicitationHandler {
    prompt_lock: Mutex<()>,
    hitl_notification_bell: bool,
    approval_policy: Arc<StdRwLock<AskForApproval>>,
}

impl InteractiveMcpElicitationHandler {
    pub(crate) fn new(
        hitl_notification_bell: bool,
        approval_policy: Arc<StdRwLock<AskForApproval>>,
    ) -> Self {
        Self {
            prompt_lock: Mutex::new(()),
            hitl_notification_bell,
            approval_policy,
        }
    }
}

fn elicitation_is_rejected_by_policy(approval_policy: AskForApproval) -> bool {
    approval_policy.rejects_mcp_elicitation()
}

#[async_trait]
impl McpElicitationHandler for InteractiveMcpElicitationHandler {
    async fn handle_elicitation(
        &self,
        provider: &str,
        request: McpElicitationRequest,
    ) -> Result<McpElicitationResponse> {
        use std::io::{self, Write};

        let approval_policy = match self.approval_policy.read() {
            Ok(policy) => *policy,
            Err(poisoned) => {
                tracing::warn!(
                    "MCP elicitation approval policy lock poisoned; continuing with recovered value"
                );
                *poisoned.into_inner()
            }
        };

        if elicitation_is_rejected_by_policy(approval_policy) {
            tracing::info!(
                "Auto-declining MCP elicitation from '{}' due to approval policy",
                provider
            );
            return Ok(McpElicitationResponse {
                action: ElicitationAction::Decline,
                content: None,
            });
        }

        let _guard = self.prompt_lock.lock().await;

        // Notify the user that attention is required
        if self.hitl_notification_bell
            && let Err(err) = send_global_notification(NotificationEvent::HumanInTheLoop {
                prompt: "MCP input required".to_string(),
                context: format!("Provider: {}", provider),
            })
            .await
        {
            tracing::debug!(error = %err, "Failed to emit MCP HITL notification");
            notify_attention(true, Some("MCP input required"));
        }

        tracing::info!("MCP elicitation request from '{}'", provider);
        tracing::info!("{}", request.message);

        if !request.requested_schema.is_null() {
            let schema_display = serde_json::to_string_pretty(&request.requested_schema)
                .context("Failed to format MCP elicitation schema")?;
            tracing::debug!("Expected response schema:\n{}", schema_display);
        }

        tracing::info!(
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
            tracing::info!("Cancelling elicitation request from '{}'.", provider);
            return Ok(McpElicitationResponse {
                action: ElicitationAction::Cancel,
                content: None,
            });
        }

        if trimmed.is_empty() {
            tracing::info!("Declining elicitation request from '{}'.", provider);
            return Ok(McpElicitationResponse {
                action: ElicitationAction::Decline,
                content: None,
            });
        }

        let content: Value =
            serde_json::from_str(trimmed).context("Elicitation response must be valid JSON")?;

        tracing::info!("Submitting elicitation response to '{}'.", provider);

        Ok(McpElicitationResponse {
            action: ElicitationAction::Accept,
            content: Some(content),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::elicitation_is_rejected_by_policy;
    use vtcode_core::exec_policy::{AskForApproval, RejectConfig};

    #[test]
    fn elicitation_reject_policy_defaults_to_prompting() {
        assert!(!elicitation_is_rejected_by_policy(
            AskForApproval::OnFailure
        ));
        assert!(!elicitation_is_rejected_by_policy(
            AskForApproval::OnRequest
        ));
        assert!(!elicitation_is_rejected_by_policy(
            AskForApproval::UnlessTrusted
        ));
        assert!(!elicitation_is_rejected_by_policy(AskForApproval::Reject(
            RejectConfig {
                sandbox_approval: false,
                rules: false,
                request_permissions: false,
                mcp_elicitations: false,
            }
        )));
    }

    #[test]
    fn elicitation_reject_policy_respects_never_and_reject_config() {
        assert!(elicitation_is_rejected_by_policy(AskForApproval::Never));
        assert!(elicitation_is_rejected_by_policy(AskForApproval::Reject(
            RejectConfig {
                sandbox_approval: false,
                rules: false,
                request_permissions: false,
                mcp_elicitations: true,
            }
        )));
    }
}
