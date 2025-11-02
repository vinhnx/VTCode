use anyhow::Result;

use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::async_mcp_manager::{AsyncMcpManager, McpInitStatus};
use super::workspace_links::LinkedDirectory;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

pub(crate) async fn run_doctor_diagnostics(
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    provider_name: &str,
    async_mcp_manager: Option<&AsyncMcpManager>,
    linked_directories: &[LinkedDirectory],
) -> Result<()> {
    renderer.line(MessageStyle::Status, "Running VTCode doctor checks:")?;

    let workspace_result = if config.workspace.is_dir() {
        Ok(format!("{}", config.workspace.display()))
    } else {
        Err("Workspace directory is missing or inaccessible".to_string())
    };
    render_doctor_check(renderer, "Workspace", workspace_result)?;

    let config_result = match ConfigManager::load_from_workspace(&config.workspace) {
        Ok(manager) => {
            if let Some(path) = manager.config_path() {
                Ok(format!("Loaded configuration from {}", path.display()))
            } else {
                Ok("Using runtime defaults (no vtcode.toml found)".to_string())
            }
        }
        Err(err) => Err(err.to_string()),
    };
    render_doctor_check(renderer, "Configuration", config_result)?;

    let provider_label = if provider_name.trim().is_empty() {
        "gemini"
    } else {
        provider_name
    };
    let api_key_result = match get_api_key(provider_label, &ApiKeySources::default()) {
        Ok(_) => Ok(format!(
            "API key detected for provider '{}'.",
            provider_label
        )),
        Err(err) => Err(format!(
            "Missing API key for provider '{}': {}",
            provider_label, err
        )),
    };
    render_doctor_check(renderer, "API key", api_key_result)?;

    let cli_version = format!("VTCode {}", env!("CARGO_PKG_VERSION"));
    render_doctor_check(renderer, "VTCode CLI", Ok(cli_version))?;

    let node_result = detect_command_version("node", &["--version"])
        .map(|version| format!("Found Node.js {}", version));
    render_doctor_check(renderer, "Node.js", node_result)?;

    let npm_result = detect_command_version("npm", &["--version"])
        .map(|version| format!("Found npm {}", version));
    render_doctor_check(renderer, "npm", npm_result)?;

    let claude_result = detect_command_version("claude", &["--version"])
        .map(|version| format!("Found Claude CLI {}", version));
    render_doctor_check(renderer, "Claude CLI", claude_result)?;

    let mcp_result = if let Some(cfg) = vt_cfg {
        if cfg.mcp.enabled {
            if let Some(manager) = async_mcp_manager {
                let status = manager.get_status().await;
                match status {
                    McpInitStatus::Ready { client } => {
                        let runtime_status = client.get_status();
                        Ok(format!(
                            "Enabled with {} configured provider(s), {} active",
                            runtime_status.configured_providers.len(),
                            runtime_status.active_connections
                        ))
                    }
                    McpInitStatus::Initializing { progress } => {
                        Ok(format!("Initializing: {}", progress))
                    }
                    McpInitStatus::Error { message } => {
                        Err(format!("Initialization error: {}", message))
                    }
                    McpInitStatus::Disabled => Ok("Disabled in configuration".to_string()),
                }
            } else {
                Err("Enabled in configuration but manager not initialized".to_string())
            }
        } else {
            Ok("Disabled in configuration".to_string())
        }
    } else {
        Ok("No MCP configuration detected".to_string())
    };
    render_doctor_check(renderer, "MCP", mcp_result)?;

    if linked_directories.is_empty() {
        renderer.line(MessageStyle::Status, "Linked directories: none")?;
    } else {
        let aliases: Vec<String> = linked_directories
            .iter()
            .map(|entry| entry.display_path.clone())
            .collect();
        renderer.line(
            MessageStyle::Status,
            &format!("Linked directories: {}", aliases.join(", ")),
        )?;
    }

    renderer.line(
        MessageStyle::Info,
        "Doctor finished. Run `cargo check` or `vtcode mcp list` for more details if needed.",
    )?;
    Ok(())
}

fn render_doctor_check(
    renderer: &mut AnsiRenderer,
    label: &str,
    outcome: std::result::Result<String, String>,
) -> Result<()> {
    match outcome {
        Ok(detail) => {
            renderer.line(MessageStyle::Status, &format!("[ok] {}: {}", label, detail))?
        }
        Err(detail) => renderer.line(
            MessageStyle::Error,
            &format!("[fail] {}: {}", label, detail),
        )?,
    }
    Ok(())
}

fn detect_command_version(command: &str, args: &[&str]) -> std::result::Result<String, String> {
    use std::process::Command;

    match Command::new(command).args(args).output() {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let version_text = if !stdout.is_empty() { stdout } else { stderr };
                if version_text.is_empty() {
                    Err(format!(
                        "{} {} returned no version output",
                        command,
                        args.join(" ")
                    ))
                } else {
                    Ok(version_text)
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let reason = if !stderr.is_empty() {
                    stderr
                } else {
                    output.status.to_string()
                };
                Err(format!(
                    "{} {} exited with error: {}",
                    command,
                    args.join(" "),
                    reason
                ))
            }
        }
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                Err(format!("{} not found in PATH", command))
            } else {
                Err(format!("Failed to execute {}: {}", command, err))
            }
        }
    }
}
