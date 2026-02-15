use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::{
    fs, task,
    time::{Duration, sleep},
};

use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::mcp::validate_mcp_config;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::async_mcp_manager::{AsyncMcpManager, McpInitStatus};
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::welcome::SessionBootstrap;
#[path = "mcp_support_overview.rs"]
mod mcp_support_overview;
pub(crate) use mcp_support_overview::{
    display_mcp_providers, display_mcp_status, display_mcp_tools, refresh_mcp_tools,
};

pub(crate) async fn display_mcp_config_summary(
    renderer: &mut AnsiRenderer,
    vt_cfg: Option<&VTCodeConfig>,
    session_bootstrap: &SessionBootstrap,
    async_mcp_manager: Option<&AsyncMcpManager>,
) -> Result<()> {
    renderer.line(MessageStyle::Status, "MCP configuration summary:")?;

    if let Some(cfg) = vt_cfg {
        let mcp_cfg = &cfg.mcp;
        renderer.line(
            MessageStyle::Info,
            &format!("  enabled: {}", if mcp_cfg.enabled { "yes" } else { "no" }),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!("  providers configured: {}", mcp_cfg.providers.len()),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!(
                "  max concurrent connections: {}",
                mcp_cfg.max_concurrent_connections
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!("  request timeout: {}s", mcp_cfg.request_timeout_seconds),
        )?;
        renderer.line(
            MessageStyle::Info,
            &format!("  retry attempts: {}", mcp_cfg.retry_attempts),
        )?;

        if let Some(seconds) = mcp_cfg.startup_timeout_seconds {
            renderer.line(
                MessageStyle::Info,
                &format!("  startup timeout: {}s", seconds),
            )?;
        }

        if let Some(seconds) = mcp_cfg.tool_timeout_seconds {
            renderer.line(MessageStyle::Info, &format!("  tool timeout: {}s", seconds))?;
        }

        if mcp_cfg.server.enabled {
            renderer.line(MessageStyle::Info, "  MCP server exposure: enabled")?;
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "    bind: {}:{}",
                    mcp_cfg.server.bind_address, mcp_cfg.server.port
                ),
            )?;
        } else {
            renderer.line(MessageStyle::Info, "  MCP server exposure: disabled")?;
        }

        if mcp_cfg.allowlist.enforce {
            let provider_overrides = mcp_cfg.allowlist.providers.len();
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "  allow list: enforced ({} provider override{})",
                    provider_overrides,
                    if provider_overrides == 1 { "" } else { "s" }
                ),
            )?;
        } else {
            renderer.line(MessageStyle::Info, "  allow list: not enforced")?;
        }

        if mcp_cfg.security.auth_enabled {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "  server auth: enabled (env: {})",
                    mcp_cfg.security.api_key_env.as_deref().unwrap_or("<unset>")
                ),
            )?;
        } else {
            renderer.line(MessageStyle::Info, "  server auth: disabled")?;
        }
    } else {
        match session_bootstrap.mcp_enabled {
            Some(true) => renderer.line(
                MessageStyle::Info,
                "  MCP enabled via runtime defaults (vtcode.toml not loaded)",
            )?,
            Some(false) => {
                renderer.line(MessageStyle::Info, "  MCP disabled via runtime defaults")?
            }
            None => renderer.line(
                MessageStyle::Info,
                "  No vtcode.toml found; using default MCP settings",
            )?,
        }
    }

    renderer.line(MessageStyle::Info, "Configured providers:")?;
    display_mcp_providers(renderer, session_bootstrap, async_mcp_manager).await?;
    Ok(())
}

pub(crate) async fn render_mcp_config_edit_guidance(
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<()> {
    renderer.line(MessageStyle::Status, "MCP configuration editing:")?;

    let workspace_path = workspace.to_path_buf();
    let config_path = task::spawn_blocking({
        let workspace_clone = workspace_path.clone();
        move || -> Result<Option<PathBuf>> {
            let manager = ConfigManager::load_from_workspace(&workspace_clone)?;
            Ok(manager.config_path().map(Path::to_path_buf))
        }
    })
    .await??;

    let target_path = config_path.unwrap_or_else(|| workspace_path.join("vtcode.toml"));
    let exists = fs::try_exists(&target_path).await.unwrap_or(false);

    renderer.line(
        MessageStyle::Info,
        &format!("  File: {}", target_path.display()),
    )?;

    if exists {
        renderer.line(
            MessageStyle::Info,
            "  Open this file in your editor and update the [mcp] section.",
        )?;
    } else {
        renderer.line(
            MessageStyle::Info,
            "  File not found. Run `vtcode config bootstrap` or create it manually.",
        )?;
    }

    renderer.line(
        MessageStyle::Info,
        "  Reload providers with /mcp refresh after saving changes.",
    )?;

    Ok(())
}

pub(crate) async fn repair_mcp_runtime(
    renderer: &mut AnsiRenderer,
    async_mcp_manager: Option<&AsyncMcpManager>,
    tool_registry: &mut ToolRegistry,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<()> {
    renderer.line(MessageStyle::Status, "Repairing MCP runtime:")?;

    if let Some(cfg) = vt_cfg {
        match validate_mcp_config(&cfg.mcp) {
            Ok(_) => renderer.line(MessageStyle::Info, "  Configuration validation: ok")?,
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("  Configuration validation failed: {}", err),
                )?;
                renderer.line(
                    MessageStyle::Info,
                    "  Update vtcode.toml and rerun /mcp repair.",
                )?;
                return Ok(());
            }
        }
    } else {
        renderer.line(
            MessageStyle::Info,
            "  vtcode.toml not detected; using default MCP settings.",
        )?;
    }

    let Some(manager) = async_mcp_manager else {
        renderer.line(
            MessageStyle::Info,
            "  MCP runtime is not active in this session.",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  Enable MCP in vtcode.toml and restart the agent.",
        )?;
        return Ok(());
    };

    if let McpInitStatus::Disabled = manager.get_status().await {
        renderer.line(
            MessageStyle::Info,
            "  MCP is disabled; update vtcode.toml to enable it.",
        )?;
        return Ok(());
    }

    renderer.line(
        MessageStyle::Info,
        "  Shutting down existing MCP connections…",
    )?;
    manager.shutdown().await?;

    renderer.line(MessageStyle::Info, "  Restarting MCP manager…")?;
    if let Err(err) = manager.start_initialization() {
        renderer.line(
            MessageStyle::Error,
            &format!("  Failed to restart MCP manager: {}", err),
        )?;
        return Ok(());
    }

    const MAX_ATTEMPTS: usize = 20;
    let mut stabilized = false;
    for attempt in 0..MAX_ATTEMPTS {
        match manager.get_status().await {
            McpInitStatus::Ready { .. } => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("  MCP reinitialized after {} check(s).", attempt + 1),
                )?;
                stabilized = true;
                break;
            }
            McpInitStatus::Error { message } => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("  MCP restart error: {}", message),
                )?;
                return Ok(());
            }
            McpInitStatus::Disabled => {
                renderer.line(
                    MessageStyle::Info,
                    "  MCP disabled during restart; check vtcode.toml settings.",
                )?;
                return Ok(());
            }
            _ => {
                if attempt == MAX_ATTEMPTS - 1 {
                    renderer.line(
                        MessageStyle::Info,
                        "  MCP still initializing; check /mcp status shortly.",
                    )?;
                } else {
                    sleep(Duration::from_millis(250)).await;
                }
            }
        }
    }

    if stabilized {
        refresh_mcp_tools(renderer, tool_registry).await?;
    }

    renderer.line(
        MessageStyle::Info,
        "  Repair complete. Use /mcp diagnose for additional checks.",
    )?;

    Ok(())
}

pub(crate) async fn diagnose_mcp(
    renderer: &mut AnsiRenderer,
    vt_cfg: Option<&VTCodeConfig>,
    session_bootstrap: &SessionBootstrap,
    async_mcp_manager: Option<&AsyncMcpManager>,
    tool_registry: &mut ToolRegistry,
    mcp_panel_state: &mcp_events::McpPanelState,
) -> Result<()> {
    renderer.line(MessageStyle::Status, "Running MCP diagnostics:")?;

    if let Some(cfg) = vt_cfg {
        match validate_mcp_config(&cfg.mcp) {
            Ok(_) => renderer.line(MessageStyle::Info, "  Configuration validation: ok")?,
            Err(err) => renderer.line(
                MessageStyle::Error,
                &format!("  Configuration validation failed: {}", err),
            )?,
        }
    } else {
        renderer.line(
            MessageStyle::Info,
            "  vtcode.toml not detected; using default MCP settings.",
        )?;
    }

    display_mcp_status(
        renderer,
        session_bootstrap,
        tool_registry,
        async_mcp_manager,
        mcp_panel_state,
    )
    .await?;
    display_mcp_providers(renderer, session_bootstrap, async_mcp_manager).await?;
    display_mcp_tools(renderer, tool_registry).await?;

    renderer.line(
        MessageStyle::Info,
        "Diagnostics complete. Use /mcp repair to restart providers if issues remain.",
    )?;
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
        "VT Code delegates OAuth to the CLI today.",
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

/// Render detailed MCP status including errors and connection issues
#[allow(dead_code)]
pub(crate) async fn display_detailed_mcp_status(
    renderer: &mut AnsiRenderer,
    async_mcp_manager: Option<&AsyncMcpManager>,
) -> Result<()> {
    if let Some(manager) = async_mcp_manager {
        let status = manager.get_status().await;

        match &status {
            super::async_mcp_manager::McpInitStatus::Disabled => {
                renderer.line(MessageStyle::Status, "MCP: Disabled in configuration")?;
            }
            super::async_mcp_manager::McpInitStatus::Initializing { progress } => {
                renderer.line(
                    MessageStyle::Info,
                    &format!("MCP: Initializing - {}", progress),
                )?;
            }
            super::async_mcp_manager::McpInitStatus::Ready { client } => {
                let client_status = client.get_status();
                renderer.line(
                    MessageStyle::Status,
                    &format!(
                        "MCP: Connected ({} active, {} configured)",
                        client_status.active_connections, client_status.provider_count
                    ),
                )?;

                // Show provider-specific status
                for provider_name in &client_status.configured_providers {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("  - Provider '{}' connected", provider_name),
                    )?;
                }
            }
            super::async_mcp_manager::McpInitStatus::Error { message } => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("MCP: Error occurred - {}", message),
                )?;

                // Provide specific guidance based on common error types
                if message.contains("EPIPE") || message.contains("Broken pipe") {
                    renderer.line(
                        MessageStyle::Info,
                        "  - This error often occurs when MCP server processes exit unexpectedly",
                    )?;
                    renderer.line(
                        MessageStyle::Info,
                        "  - Check if your MCP provider processes are running correctly",
                    )?;
                } else if message.contains("timeout") {
                    renderer.line(
                        MessageStyle::Info,
                        "  - This error may indicate network issues or slow server response",
                    )?;
                    renderer.line(
                        MessageStyle::Info,
                        "  - Consider increasing startup_timeout_seconds in config",
                    )?;
                } else if message.contains("No such process") {
                    renderer.line(
                        MessageStyle::Info,
                        "  - The MCP server process may have failed to start",
                    )?;
                    renderer.line(
                        MessageStyle::Info,
                        "  - Verify the command path and arguments in your vtcode.toml",
                    )?;
                }
            }
        }
    } else {
        renderer.line(MessageStyle::Info, "MCP: Not initialized in this session")?;
    }

    Ok(())
}
