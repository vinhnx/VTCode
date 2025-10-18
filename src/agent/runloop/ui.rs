use anyhow::{Context, Result};
use unicode_width::UnicodeWidthStr;
use vtcode_core::config::constants::ui;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
use vtcode_core::ui::tui::InlineHeaderContext;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
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
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    header_context: &InlineHeaderContext,
) -> Result<()> {
    let defaults = InlineHeaderContext::default();
    let version = {
        let trimmed = header_context.version.trim();
        if trimmed.is_empty() {
            defaults.version
        } else {
            trimmed.to_string()
        }
    };

    let header_line = format!(
        "{}{} {}{}{}",
        ui::HEADER_VERSION_PROMPT,
        ui::HEADER_VERSION_PREFIX,
        ui::HEADER_VERSION_LEFT_DELIMITER,
        version,
        ui::HEADER_VERSION_RIGHT_DELIMITER
    );

    let mut lines = Vec::new();
    lines.push(header_line);

    let mut details = Vec::new();

    let push_detail = |target: &mut Vec<String>, value: &str| {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            target.push(trimmed.to_string());
        }
    };

    push_detail(&mut details, &header_context.provider);
    push_detail(&mut details, &header_context.model);
    push_detail(&mut details, &header_context.reasoning);

    let mode_label = header_context.mode.trim();
    if !mode_label.is_empty() {
        details.push(format!("Mode: {}", mode_label));
    }

    let workspace = config.workspace.to_string_lossy();
    if !workspace.trim().is_empty() {
        details.push(format!("Workspace: {}", workspace));
    }

    push_detail(&mut details, &header_context.workspace_trust);
    push_detail(&mut details, &header_context.tools);
    push_detail(&mut details, &header_context.mcp);

    if !details.is_empty() {
        lines.push(String::new());
        lines.extend(details);
    }

    if !header_context.highlights.is_empty() {
        if !lines.is_empty() {
            lines.push(String::new());
        }

        for (index, highlight) in header_context.highlights.iter().enumerate() {
            let title = highlight.title.trim();
            let has_body = highlight.lines.iter().any(|line| !line.trim().is_empty());
            if !title.is_empty() {
                if has_body {
                    lines.push(format!("{title}:"));
                } else {
                    lines.push(title.to_string());
                }
            }

            for entry in &highlight.lines {
                let trimmed = entry.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                lines.push(trimmed.to_string());
            }

            if index + 1 != header_context.highlights.len() {
                lines.push(String::new());
            }
        }
    }

    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    let content_width = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()))
        .max()
        .unwrap_or(0);

    let left_padding = 2usize;
    let right_padding = 2usize;
    let interior_width = content_width + left_padding + right_padding;
    let top_border = format!("╭{}╮", "─".repeat(interior_width));
    let bottom_border = format!("╰{}╯", "─".repeat(interior_width));

    renderer.line(MessageStyle::Info, &top_border)?;

    for line in &lines {
        let line_width = UnicodeWidthStr::width(line.as_str());
        let mut padded = String::new();
        padded.push_str(&" ".repeat(left_padding));
        padded.push_str(line);
        let remaining = content_width.saturating_sub(line_width);
        padded.push_str(&" ".repeat(remaining + right_padding));
        let row = format!("│{}│", padded);
        renderer.line(MessageStyle::Info, &row)?;
    }

    renderer.line(MessageStyle::Info, &bottom_border)?;
    renderer.line_if_not_empty(MessageStyle::Info)?;

    Ok(())
}

#[cfg(test)]
mod tests {}
