use anyhow::{Context, Result};
use vtcode_core::config::constants::ui;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
use vtcode_core::ui::tui::InlineHeaderContext;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::dot_config::WorkspaceTrustLevel;

use tracing::warn;

use super::welcome::SessionBootstrap;
use crate::workspace_trust;

#[derive(Clone, Debug)]
enum ToolStatusSummary {
    Available {
        allow: usize,
        prompt: usize,
        deny: usize,
    },
    Unavailable,
}

#[derive(Clone, Debug)]
enum McpStatusSummary {
    Enabled {
        active_providers: Vec<String>,
        configured: bool,
    },
    Disabled,
    Error(String),
    Unknown,
}

#[derive(Clone, Debug)]
struct InlineStatusDetails {
    workspace_trust: Option<WorkspaceTrustLevel>,
    tool_status: ToolStatusSummary,
    mcp_status: McpStatusSummary,
}

fn gather_inline_status_details(
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
) -> Result<InlineStatusDetails> {
    let workspace_trust = workspace_trust::workspace_trust_level(&config.workspace)
        .context("Failed to determine workspace trust level for banner")?;

    let tool_status = match ToolPolicyManager::new_with_workspace(&config.workspace) {
        Ok(manager) => {
            let summary = manager.get_policy_summary();
            let mut allow = 0usize;
            let mut prompt = 0usize;
            let mut deny = 0usize;
            for policy in summary.values() {
                match policy {
                    ToolPolicy::Allow => allow += 1,
                    ToolPolicy::Prompt => prompt += 1,
                    ToolPolicy::Deny => deny += 1,
                }
            }
            ToolStatusSummary::Available {
                allow,
                prompt,
                deny,
            }
        }
        Err(err) => {
            warn!("failed to load tool policy summary: {err:#}");
            ToolStatusSummary::Unavailable
        }
    };

    let mcp_status = if let Some(error) = session_bootstrap.mcp_error.clone() {
        McpStatusSummary::Error(error)
    } else if let Some(enabled) = session_bootstrap.mcp_enabled {
        if enabled {
            let configured = session_bootstrap.mcp_providers.is_some();
            let active_providers = session_bootstrap
                .mcp_providers
                .as_ref()
                .map(|providers| {
                    providers
                        .iter()
                        .filter(|provider| provider.enabled)
                        .map(|provider| provider.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            McpStatusSummary::Enabled {
                active_providers,
                configured,
            }
        } else {
            McpStatusSummary::Disabled
        }
    } else {
        McpStatusSummary::Unknown
    };

    Ok(InlineStatusDetails {
        workspace_trust,
        tool_status,
        mcp_status,
    })
}

pub(crate) fn build_inline_header_context(
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
    provider_label: String,
    model_label: String,
    mode_label: String,
    reasoning_label: String,
) -> Result<InlineHeaderContext> {
    let InlineStatusDetails {
        workspace_trust,
        tool_status,
        mcp_status,
    } = gather_inline_status_details(config, session_bootstrap)?;

    let version = env!("CARGO_PKG_VERSION").to_string();
    let provider_value = if provider_label.trim().is_empty() {
        format!(
            "{}{}",
            ui::HEADER_PROVIDER_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        )
    } else {
        format!("{}{}", ui::HEADER_PROVIDER_PREFIX, provider_label.trim())
    };
    let model_value = if model_label.trim().is_empty() {
        format!(
            "{}{}",
            ui::HEADER_MODEL_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        )
    } else {
        format!("{}{}", ui::HEADER_MODEL_PREFIX, model_label.trim())
    };
    let trimmed_mode = mode_label.trim();
    let mode = if trimmed_mode.is_empty() {
        ui::HEADER_MODE_INLINE.to_string()
    } else {
        trimmed_mode.to_string()
    };

    let reasoning = if reasoning_label.trim().is_empty() {
        format!(
            "{}{}",
            ui::HEADER_REASONING_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        )
    } else {
        format!("{}{}", ui::HEADER_REASONING_PREFIX, reasoning_label.trim())
    };

    let trust_value = match workspace_trust {
        Some(level) => format!("{}{}", ui::HEADER_TRUST_PREFIX, level),
        None => format!(
            "{}{}",
            ui::HEADER_TRUST_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
    };

    let tools_value = match tool_status {
        ToolStatusSummary::Available {
            allow,
            prompt,
            deny,
        } => format!(
            "{}allow {} · prompt {} · deny {}",
            ui::HEADER_TOOLS_PREFIX,
            allow,
            prompt,
            deny
        ),
        ToolStatusSummary::Unavailable => format!(
            "{}{}",
            ui::HEADER_TOOLS_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
    };

    let mcp_value = match mcp_status {
        McpStatusSummary::Error(message) => {
            format!("{}error - {}", ui::HEADER_MCP_PREFIX, message)
        }
        McpStatusSummary::Enabled {
            active_providers,
            configured,
        } => {
            if !active_providers.is_empty() {
                format!(
                    "{}enabled ({})",
                    ui::HEADER_MCP_PREFIX,
                    active_providers.join(", ")
                )
            } else if configured {
                format!("{}enabled (no providers)", ui::HEADER_MCP_PREFIX)
            } else {
                format!("{}enabled", ui::HEADER_MCP_PREFIX)
            }
        }
        McpStatusSummary::Disabled => format!("{}disabled", ui::HEADER_MCP_PREFIX),
        McpStatusSummary::Unknown => format!(
            "{}{}",
            ui::HEADER_MCP_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
    };

    Ok(InlineHeaderContext {
        provider: provider_value,
        model: model_value,
        version,
        mode,
        reasoning,
        workspace_trust: trust_value,
        tools: tools_value,
        mcp: mcp_value,
        highlights: session_bootstrap.header_highlights.clone(),
    })
}

pub(crate) fn render_session_banner(
    _renderer: &mut AnsiRenderer,
    _config: &CoreAgentConfig,
    _session_bootstrap: &SessionBootstrap,
    _model_label: &str,
    _reasoning_label: &str,
) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {}
