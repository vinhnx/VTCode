use anyhow::Result;
use chrono::{DateTime, Local};
use std::collections::HashSet;

use vtcode_core::config::mcp::McpTransportConfig;
use vtcode_core::mcp_client::McpClient;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::welcome::SessionBootstrap;

pub(crate) async fn display_mcp_status(
    renderer: &mut AnsiRenderer,
    session_bootstrap: &SessionBootstrap,
    tool_registry: &mut ToolRegistry,
    mcp_client: Option<&std::sync::Arc<McpClient>>,
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

    if let Some(client) = mcp_client {
        let status = client.get_status();
        renderer.line(
            MessageStyle::Status,
            &format!(
                "  • Runtime connections: {} active / {} configured",
                status.active_connections, status.provider_count
            ),
        )?;

        if !status.configured_providers.is_empty() {
            renderer.line(
                MessageStyle::Status,
                &format!(
                    "  • Configured providers: {}",
                    status.configured_providers.join(", ")
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
                if let Some(args) = event.args_preview.as_ref() {
                    if !args.trim().is_empty() {
                        let preview: String = args.chars().take(80).collect();
                        if preview.len() < args.len() {
                            detail.push_str(&format!(" · args {}…", preview));
                        } else {
                            detail.push_str(&format!(" · args {}", preview));
                        }
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

pub(crate) fn display_mcp_providers(
    renderer: &mut AnsiRenderer,
    session_bootstrap: &SessionBootstrap,
    mcp_client: Option<&std::sync::Arc<McpClient>>,
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
            "Run `vtcode mcp add` to connect Claude Code to external tools.",
        )?;
        return Ok(());
    }

    let active = mcp_client
        .map(|client| {
            client
                .get_status()
                .configured_providers
                .into_iter()
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();

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
                if !http.headers.is_empty() {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "    headers: {}",
                            http.headers.keys().cloned().collect::<Vec<_>>().join(", ")
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

            let mut grouped: std::collections::BTreeMap<String, Vec<_>> =
                std::collections::BTreeMap::new();
            for tool in tools {
                grouped.entry(tool.provider.clone()).or_default().push(tool);
            }

            for (provider, mut entries) in grouped {
                entries.sort_by(|a, b| a.name.cmp(&b.name));
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
) -> Result<()> {
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
        }
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to refresh MCP tools: {}", err),
            )?;
        }
    }
    Ok(())
}

pub(crate) fn render_mcp_login_guidance(
    renderer: &mut AnsiRenderer,
    provider: String,
    is_login: bool,
) -> Result<()> {
    let trimmed = provider.trim();
    if trimmed.is_empty() {
        renderer.line(
            MessageStyle::Error,
            "Provider name required. Usage: /mcp login <name> or /mcp logout <name>.",
        )?;
        return Ok(());
    }

    let action = if is_login { "login" } else { "logout" };
    renderer.line(
        MessageStyle::Status,
        &format!(
            "OAuth {} flow requested for provider '{}'.",
            action, trimmed
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        "VTCode delegates OAuth to the CLI today.",
    )?;
    renderer.line(
        MessageStyle::Info,
        &format!("Run: vtcode mcp {} {}", action, trimmed),
    )?;
    renderer.line(
        MessageStyle::Info,
        "This command will walk you through the authentication flow in your shell.",
    )?;
    Ok(())
}
