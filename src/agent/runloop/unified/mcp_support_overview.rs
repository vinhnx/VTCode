use anyhow::Result;
use chrono::{DateTime, Local};
use hashbrown::HashSet;

use vtcode_core::config::mcp::McpTransportConfig;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::async_mcp_manager::{AsyncMcpManager, McpInitStatus};
use crate::agent::runloop::welcome::SessionBootstrap;

fn group_mcp_tools_by_provider_preserving_order(
    tools: impl IntoIterator<Item = vtcode_core::mcp::McpToolInfo>,
) -> Vec<(String, Vec<vtcode_core::mcp::McpToolInfo>)> {
    let mut grouped: Vec<(String, Vec<vtcode_core::mcp::McpToolInfo>)> = Vec::new();

    for tool in tools {
        let provider = tool.provider.clone();
        if let Some((_, provider_tools)) = grouped
            .iter_mut()
            .find(|(existing_provider, _)| *existing_provider == provider)
        {
            provider_tools.push(tool);
        } else {
            grouped.push((provider, vec![tool]));
        }
    }

    grouped
}

pub(crate) async fn display_mcp_status(
    renderer: &mut AnsiRenderer,
    session_bootstrap: &SessionBootstrap,
    tool_registry: &mut ToolRegistry,
    async_mcp_manager: Option<&AsyncMcpManager>,
    mcp_panel_state: &mcp_events::McpPanelState,
) -> Result<()> {
    renderer.line(MessageStyle::Status, "MCP status overview:")?;

    match session_bootstrap.mcp_enabled {
        Some(true) => renderer.line(MessageStyle::Status, "  • Enabled in vtcode.toml")?,
        Some(false) => renderer.line(MessageStyle::Status, "  • Disabled in vtcode.toml")?,
        None => renderer.line(MessageStyle::Status, "  • No MCP configuration detected")?,
    }

    if let Some(providers) = &session_bootstrap.mcp_providers {
        if providers.is_empty() {
            renderer.line(MessageStyle::Status, "  • No MCP providers configured")?;
        } else {
            let enabled: Vec<String> = providers
                .iter()
                .filter(|provider| provider.enabled)
                .map(|provider| provider.name.clone())
                .collect();
            if enabled.is_empty() {
                renderer.line(
                    MessageStyle::Status,
                    "  • Providers present but all disabled",
                )?;
            } else {
                renderer.line(
                    MessageStyle::Status,
                    &format!("  • Enabled providers: {}", enabled.join(", ")),
                )?;
            }
        }
    }

    if let Some(manager) = async_mcp_manager {
        let status = manager.get_status().await;
        match &status {
            McpInitStatus::Disabled => {
                renderer.line(MessageStyle::Status, "  • MCP disabled and not initialized")?;
            }
            McpInitStatus::Initializing { progress } => {
                renderer.line(
                    MessageStyle::Status,
                    &format!("  • Initializing: {}", progress),
                )?;
            }
            McpInitStatus::Ready { client } => {
                let runtime_status = client.get_status();
                renderer.line(
                    MessageStyle::Status,
                    &format!(
                        "  • Runtime connections: {} active / {} configured",
                        runtime_status.active_connections, runtime_status.provider_count
                    ),
                )?;

                if !runtime_status.configured_providers.is_empty() {
                    renderer.line(
                        MessageStyle::Status,
                        &format!(
                            "  • Configured providers: {}",
                            runtime_status.configured_providers.join(", ")
                        ),
                    )?;
                }

                match tool_registry.list_mcp_tools().await {
                    Ok(tools) => {
                        if tools.is_empty() {
                            renderer.line(
                                MessageStyle::Status,
                                "  • No MCP tools exposed by providers",
                            )?;
                        } else {
                            let mut samples = Vec::new();
                            for info in tools.iter().take(5) {
                                samples.push(format!("{} ({})", info.name, info.provider));
                            }
                            let extra = tools.len().saturating_sub(samples.len());
                            let suffix = if extra > 0 {
                                format!(" and {} more", extra)
                            } else {
                                String::new()
                            };
                            renderer.line(
                                MessageStyle::Status,
                                &format!("  • Tools available: {}{}", samples.join(", "), suffix),
                            )?;
                        }
                    }
                    Err(err) => {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("  • Failed to list MCP tools: {}", err),
                        )?;
                    }
                }
            }
            McpInitStatus::Error { message } => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("  • MCP initialization failed: {}", message),
                )?;
            }
        }
    } else {
        renderer.line(
            MessageStyle::Status,
            "  • MCP client inactive in this session",
        )?;
    }

    if mcp_panel_state.is_enabled() {
        let recent = mcp_panel_state.recent_events_snapshot(5);
        if !recent.is_empty() {
            renderer.line(MessageStyle::McpStatus, "Recent MCP activity:")?;
            for event in recent {
                let timestamp: DateTime<Local> = DateTime::<Local>::from(event.timestamp);
                let mut detail = event.status.label().to_string();
                if let Some(args) = event.args_preview.as_ref()
                    && !args.trim().is_empty()
                {
                    let preview: String = args.chars().take(80).collect();
                    if preview.len() < args.len() {
                        detail.push_str(&format!(" · args {}…", preview));
                    } else {
                        detail.push_str(&format!(" · args {}", preview));
                    }
                }

                renderer.line(
                    MessageStyle::McpStatus,
                    &format!(
                        "    {} [{}] {}::{} — {}",
                        event.status.symbol(),
                        timestamp.format("%H:%M:%S"),
                        event.provider,
                        event.method,
                        detail
                    ),
                )?;
            }
        }
    }

    renderer.line(
        MessageStyle::Info,
        "Use `vtcode mcp list` to review and manage providers.",
    )?;
    Ok(())
}

pub(crate) async fn display_mcp_providers(
    renderer: &mut AnsiRenderer,
    session_bootstrap: &SessionBootstrap,
    async_mcp_manager: Option<&AsyncMcpManager>,
) -> Result<()> {
    renderer.line(
        MessageStyle::Status,
        "MCP providers configured in vtcode.toml:",
    )?;

    let Some(configured) = &session_bootstrap.mcp_providers else {
        renderer.line(
            MessageStyle::Info,
            "No vtcode.toml configuration detected for MCP providers.",
        )?;
        renderer.line(
            MessageStyle::Info,
            "Use `vtcode mcp add <name> --command <...>` to register providers.",
        )?;
        return Ok(());
    };

    if configured.is_empty() {
        renderer.line(MessageStyle::Info, "No providers defined in vtcode.toml.")?;
        renderer.line(
            MessageStyle::Info,
            "Run `vtcode mcp add` to connect VT Code to external tools.",
        )?;
        return Ok(());
    }

    // Get active providers from the async manager
    let active = if let Some(manager) = async_mcp_manager {
        match manager.get_status().await {
            McpInitStatus::Ready { client } => client
                .get_status()
                .configured_providers
                .into_iter()
                .collect::<HashSet<_>>(),
            _ => HashSet::new(),
        }
    } else {
        HashSet::new()
    };

    for provider in configured {
        let runtime_state = if active.contains(&provider.name) {
            "connected"
        } else if provider.enabled {
            "inactive"
        } else {
            "disabled"
        };
        let status_label = if provider.enabled {
            "enabled"
        } else {
            "disabled"
        };
        renderer.line(
            MessageStyle::Info,
            &format!("- {} ({}, {})", provider.name, status_label, runtime_state),
        )?;

        match &provider.transport {
            McpTransportConfig::Stdio(stdio) => {
                let mut command_desc = stdio.command.clone();
                if !stdio.args.is_empty() {
                    command_desc.push(' ');
                    command_desc.push_str(&stdio.args.join(" "));
                }
                renderer.line(
                    MessageStyle::Info,
                    &format!("    transport: stdio · {}", command_desc),
                )?;
                if let Some(dir) = &stdio.working_directory {
                    renderer.line(MessageStyle::Info, &format!("    working_dir: {}", dir))?;
                }
            }
            McpTransportConfig::Http(http) => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("    transport: http · {}", http.endpoint),
                )?;
                if let Some(env) = &http.api_key_env {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("    bearer token env: {}", env),
                    )?;
                }
                if !http.http_headers.is_empty() {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "    headers: {}",
                            http.http_headers
                                .keys()
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )?;
                }
                if !http.env_http_headers.is_empty() {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "    env headers: {}",
                            http.env_http_headers
                                .keys()
                                .cloned()
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    )?;
                }
            }
        }

        renderer.line(
            MessageStyle::Info,
            &format!(
                "    max concurrent requests: {}",
                provider.max_concurrent_requests
            ),
        )?;

        if !provider.env.is_empty() {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "    env vars: {}",
                    provider.env.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
            )?;
        }
    }

    renderer.line(
        MessageStyle::Info,
        "Use `vtcode mcp get <name>` for full JSON output or `vtcode mcp remove <name>` to delete.",
    )?;
    Ok(())
}

pub(crate) async fn display_mcp_tools(
    renderer: &mut AnsiRenderer,
    tool_registry: &mut ToolRegistry,
) -> Result<()> {
    renderer.line(
        MessageStyle::Status,
        "Listing MCP tools exposed by connected providers:",
    )?;
    match tool_registry.list_mcp_tools().await {
        Ok(tools) => {
            if tools.is_empty() {
                renderer.line(
                    MessageStyle::Info,
                    "No MCP tools are currently available. Try /mcp refresh after connecting providers.",
                )?;
                return Ok(());
            }

            for (provider, entries) in group_mcp_tools_by_provider_preserving_order(tools) {
                renderer.line(
                    MessageStyle::Info,
                    &format!("- Provider: {} ({} tool(s))", provider, entries.len()),
                )?;
                for info in entries {
                    renderer.line(MessageStyle::Info, &format!("    • {}", info.name))?;
                }
            }
        }
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to list MCP tools: {}", err),
            )?;
        }
    }
    Ok(())
}

pub(crate) async fn refresh_mcp_tools(
    renderer: &mut AnsiRenderer,
    tool_registry: &mut ToolRegistry,
) -> Result<bool> {
    renderer.line(MessageStyle::Status, "Refreshing MCP tool index…")?;
    match tool_registry.refresh_mcp_tools().await {
        Ok(()) => {
            match tool_registry.list_mcp_tools().await {
                Ok(tools) => {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("Indexed {} MCP tool(s).", tools.len()),
                    )?;
                }
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Refreshed but failed to list tools: {}", err),
                    )?;
                }
            }
            renderer.line(
                MessageStyle::Info,
                "Use /mcp tools to inspect the refreshed catalog.",
            )?;
            Ok(true)
        }
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to refresh MCP tools: {}", err),
            )?;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::group_mcp_tools_by_provider_preserving_order;
    use serde_json::json;
    use vtcode_core::mcp::McpToolInfo;

    fn mock_tool(provider: &str, name: &str) -> McpToolInfo {
        McpToolInfo {
            name: name.to_string(),
            description: String::new(),
            provider: provider.to_string(),
            input_schema: json!({}),
        }
    }

    #[test]
    fn grouped_mcp_tools_preserve_provider_and_tool_order() {
        let grouped = group_mcp_tools_by_provider_preserving_order(vec![
            mock_tool("gmail", "send_email"),
            mock_tool("calendar", "create_event"),
            mock_tool("gmail", "read_email"),
            mock_tool("docs", "search"),
            mock_tool("calendar", "list_events"),
        ]);

        let providers = grouped
            .iter()
            .map(|(provider, _)| provider.as_str())
            .collect::<Vec<_>>();
        assert_eq!(providers, vec!["gmail", "calendar", "docs"]);

        let tool_names = grouped
            .into_iter()
            .map(|(_, tools)| tools.into_iter().map(|tool| tool.name).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        assert_eq!(
            tool_names,
            vec![
                vec!["send_email".to_string(), "read_email".to_string()],
                vec!["create_event".to_string(), "list_events".to_string()],
                vec!["search".to_string()],
            ]
        );
    }
}
