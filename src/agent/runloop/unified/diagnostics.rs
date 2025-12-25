use anyhow::Result;

use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::ToolPolicy;
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
    loaded_skills: Option<&std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<String, vtcode_core::skills::types::Skill>>>>,
) -> Result<()> {
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Status,
        "═══════════════════════════════════════════════════════════════",
    )?;
    renderer.line(
        MessageStyle::Status,
        &format!("VTCode Doctor v{}", env!("CARGO_PKG_VERSION")),
    )?;
    renderer.line(
        MessageStyle::Status,
        "═══════════════════════════════════════════════════════════════",
    )?;
    renderer.line(MessageStyle::Info, "")?;

    // Core environment checks
    renderer.line(MessageStyle::Status, "[Core Environment]")?;

    let workspace_result = if config.workspace.is_dir() {
        Ok(format!("{}", config.workspace.display()))
    } else {
        Err("Workspace directory is missing or inaccessible".to_string())
    };
    render_doctor_check(renderer, "  Workspace", workspace_result)?;

    let cli_version = format!("VTCode {}", env!("CARGO_PKG_VERSION"));
    render_doctor_check(renderer, "  CLI Version", Ok(cli_version))?;

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Configuration]")?;

    let config_result = match ConfigManager::load_from_workspace(&config.workspace) {
        Ok(manager) => {
            if let Some(path) = manager.config_path() {
                Ok(format!("Loaded from {}", path.display()))
            } else {
                Ok("Using runtime defaults (no vtcode.toml found)".to_string())
            }
        }
        Err(err) => Err(err.to_string()),
    };
    render_doctor_check(renderer, "  Config File", config_result)?;

    // Config-specific diagnostics
    if let Some(cfg) = vt_cfg {
        render_doctor_check(
            renderer,
            "  Theme",
            Ok(cfg.agent.theme.clone()),
        )?;

        let model_info = if cfg.agent.small_model.enabled {
            let small_model = if cfg.agent.small_model.model.is_empty() {
                "auto-select".to_string()
            } else {
                cfg.agent.small_model.model.clone()
            };
            format!(
                "{} (+ small model: {})",
                cfg.agent.default_model,
                small_model
            )
        } else {
            cfg.agent.default_model.clone()
        };
        render_doctor_check(renderer, "  Model", Ok(model_info))?;

        render_doctor_check(
            renderer,
            "  Max Turns",
            Ok(format!("{}", cfg.agent.max_conversation_turns)),
        )?;

        render_doctor_check(
            renderer,
            "  Context Tokens",
            Ok(format!("{}", cfg.context.max_context_tokens)),
        )?;

        // Token budget status
        let token_budget_status = if cfg.context.token_budget.enabled {
            format!(
                "Enabled (model: {})",
                cfg.context.token_budget.model
            )
        } else {
            "Disabled".to_string()
        };
        render_doctor_check(renderer, "  Token Budget", Ok(token_budget_status))?;

        // Decision ledger status
        let ledger_status = if cfg.context.ledger.enabled {
            format!("Enabled (max {} entries)", cfg.context.ledger.max_entries)
        } else {
            "Disabled".to_string()
        };
        render_doctor_check(renderer, "  Decision Ledger", Ok(ledger_status))?;

        // Tool limits
        render_doctor_check(
            renderer,
            "  Max Tool Loops",
            Ok(format!("{}", cfg.tools.max_tool_loops)),
        )?;

        // Security configuration
        render_doctor_check(
            renderer,
            "  HITL Enabled",
            Ok(format!(
                "{}",
                if cfg.security.human_in_the_loop { "Yes" } else { "No" }
            )),
        )?;

        // Tool default policy
        let policy = match cfg.tools.default_policy {
            ToolPolicy::Allow => "Allow all (no confirmation)",
            ToolPolicy::Deny => "Deny all (security)",
            ToolPolicy::Prompt => "Prompt on tool use",
        };
        render_doctor_check(
            renderer,
            "  Tool Policy",
            Ok(policy.to_string()),
        )?;

        // PTY configuration
        render_doctor_check(
            renderer,
            "  PTY Enabled",
            Ok(format!(
                "{}",
                if cfg.pty.enabled { "Yes" } else { "No" }
            )),
        )?;
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[API & Providers]")?;

    let provider_label = if provider_name.trim().is_empty() {
        "gemini"
    } else {
        provider_name
    };
    let api_key_result = match get_api_key(provider_label, &ApiKeySources::default()) {
        Ok(_) => Ok(format!(
            "API key configured for '{}'",
            provider_label
        )),
        Err(err) => Err(format!(
            "Missing API key for '{}': {}",
            provider_label, err
        )),
    };
    render_doctor_check(renderer, "  API Key", api_key_result)?;

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Dependencies]")?;

    let node_result = detect_command_version("node", &["--version"])
        .map(|version| format!("Node.js {}", version));
    render_doctor_check(renderer, "  Node.js", node_result)?;

    let npm_result = detect_command_version("npm", &["--version"])
        .map(|version| format!("npm {}", version));
    render_doctor_check(renderer, "  npm", npm_result)?;

    let ripgrep_result = match detect_command_version("rg", &["--version"]) {
        Ok(version) => Ok(format!("Ripgrep {}", version.lines().next().unwrap_or(&version))),
        Err(e) => {
            if e.contains("not found") {
                Err("Not installed (searches will fall back to built-in grep)".to_string())
            } else {
                Err(e)
            }
        }
    };
    render_doctor_check(renderer, "  Ripgrep", ripgrep_result)?;

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[External Services]")?;

    let mcp_result = if let Some(cfg) = vt_cfg {
        if cfg.mcp.enabled {
            if let Some(manager) = async_mcp_manager {
                let status = manager.get_status().await;
                match status {
                    McpInitStatus::Ready { client } => {
                        let runtime_status = client.get_status();
                        Ok(format!(
                            "{} configured, {} active connection(s)",
                            runtime_status.configured_providers.len(),
                            runtime_status.active_connections
                        ))
                    }
                    McpInitStatus::Initializing { progress } => {
                        Ok(format!("Initializing: {}", progress))
                    }
                    McpInitStatus::Error { message } => {
                        Err(format!("Init error: {}", message))
                    }
                    McpInitStatus::Disabled => Ok("Disabled in config".to_string()),
                }
            } else {
                Err("Enabled in config but manager not initialized".to_string())
            }
        } else {
            Ok("Disabled".to_string())
        }
    } else {
        Ok("No MCP configuration".to_string())
    };
    render_doctor_check(renderer, "  MCP", mcp_result)?;

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Workspace Links]")?;

    if linked_directories.is_empty() {
        renderer.line(MessageStyle::Output, "  No linked directories")?;
    } else {
        for (idx, entry) in linked_directories.iter().enumerate() {
            renderer.line(
                MessageStyle::Output,
                &format!("  [{}] {} → {}", idx + 1, entry.display_path, entry.original.display()),
            )?;
        }
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Skills]")?;

    if let Some(skills_map) = loaded_skills {
        let skills = skills_map.read().await;
        if skills.is_empty() {
            renderer.line(MessageStyle::Output, "  No skills loaded in session")?;
        } else {
            renderer.line(
                MessageStyle::Output,
                &format!("  {} loaded skill(s):", skills.len()),
            )?;
            for (idx, (name, skill)) in skills.iter().enumerate() {
                let scope = format!("{:?}", skill.scope).to_lowercase();
                renderer.line(
                    MessageStyle::Output,
                    &format!("    [{}] {} ({})", idx + 1, name, scope),
                )?;
            }
        }
    } else {
        renderer.line(MessageStyle::Output, "  Skills context unavailable")?;
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Status,
        "═══════════════════════════════════════════════════════════════",
    )?;
    
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Recommended Next Actions]")?;
    renderer.line(
        MessageStyle::Info,
        "[OK] All checks passed? You're ready to go. Try `/status` for session details.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "[FAIL] Failures detected? Follow the suggestions above to resolve issues.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "[TIP] For more details: `/skills list` (available skills), `/status` (session), `/context` (memory)",
    )?;
    renderer.line(
        MessageStyle::Info,
        "[DOCS] See docs/development/DOCTOR_REFERENCE.md for comprehensive troubleshooting.",
    )?;
    
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Status,
        "═══════════════════════════════════════════════════════════════",
    )?;
    renderer.line(MessageStyle::Info, "")?;
    Ok(())
}

fn render_doctor_check(
    renderer: &mut AnsiRenderer,
    label: &str,
    outcome: std::result::Result<String, String>,
) -> Result<()> {
    match outcome {
        Ok(detail) => {
            renderer.line(MessageStyle::Status, &format!("✓ {}: {}", label, detail))?
        }
        Err(detail) => {
            renderer.line(
                MessageStyle::Error,
                &format!("✗ {}: {}", label, detail),
            )?;
            // Show suggestion for specific failures
            if let Some(suggestion) = get_suggestion_for_failure(label, &detail) {
                renderer.line(MessageStyle::Info, &format!("  → {}", suggestion))?;
            }
        }
    }
    Ok(())
}

fn get_suggestion_for_failure(label: &str, error: &str) -> Option<String> {
    let label_lower = label.to_lowercase();
    
    if label_lower.contains("workspace") {
        Some("Ensure workspace directory is accessible and not deleted.".to_string())
    } else if label_lower.contains("api key") {
        Some("Set API key: export OPENAI_API_KEY=sk-... or similar for your provider.".to_string())
    } else if label_lower.contains("config") && error.contains("not found") {
        Some("Copy vtcode.toml.example to vtcode.toml to customize settings.".to_string())
    } else if label_lower.contains("node.js") {
        Some("Install Node.js: brew install node (macOS) or see nodejs.org".to_string())
    } else if label_lower.contains("npm") {
        Some("Install npm with Node.js or update: npm install -g npm@latest".to_string())
    } else if label_lower.contains("ripgrep") {
        Some("Install Ripgrep: brew install ripgrep (macOS), apt install ripgrep (Linux), or cargo install ripgrep".to_string())
    } else if label_lower.contains("mcp") && error.contains("error") {
        Some("Check MCP configuration in vtcode.toml: ensure servers are running and timeouts are reasonable.".to_string())
    } else if label_lower.contains("mcp") && error.contains("disabled") {
        Some("Enable MCP in vtcode.toml: set [mcp] enabled = true".to_string())
    } else {
        None
    }
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
