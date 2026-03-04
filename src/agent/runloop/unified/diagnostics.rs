use anyhow::Result;

use vtcode_core::config::ToolPolicy;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::async_mcp_manager::{AsyncMcpManager, McpInitStatus};
use super::workspace_links::LinkedDirectory;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DoctorOptions {
    pub quick: bool,
}

#[derive(Default)]
struct DoctorSummary {
    passed: usize,
    warnings: usize,
    failures: usize,
}

impl DoctorSummary {
    fn record(&mut self, outcome: &DoctorCheckOutcome) {
        match outcome {
            DoctorCheckOutcome::Pass(_) => self.passed += 1,
            DoctorCheckOutcome::Warn(_) => self.warnings += 1,
            DoctorCheckOutcome::Fail(_) => self.failures += 1,
        }
    }
}

enum DoctorCheckOutcome {
    Pass(String),
    Warn(String),
    Fail(String),
}

pub(crate) async fn run_doctor_diagnostics(
    renderer: &mut AnsiRenderer,
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    provider_name: &str,
    async_mcp_manager: Option<&AsyncMcpManager>,
    linked_directories: &[LinkedDirectory],
    loaded_skills: Option<
        &std::sync::Arc<
            tokio::sync::RwLock<hashbrown::HashMap<String, vtcode_core::skills::types::Skill>>,
        >,
    >,
    options: DoctorOptions,
) -> Result<()> {
    let mut summary = DoctorSummary::default();

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Status,
        "═══════════════════════════════════════════════════════════════",
    )?;
    renderer.line(
        MessageStyle::Status,
        &format!("VT Code Doctor v{}", env!("CARGO_PKG_VERSION")),
    )?;
    renderer.line(
        MessageStyle::Status,
        "═══════════════════════════════════════════════════════════════",
    )?;
    if options.quick {
        renderer.line(
            MessageStyle::Info,
            "Mode: quick (skips dependency, external service, and session context checks)",
        )?;
    }
    renderer.line(MessageStyle::Info, "")?;

    renderer.line(MessageStyle::Status, "[Core Environment]")?;

    let workspace_result = if config.workspace.is_dir() {
        DoctorCheckOutcome::Pass(format!("{}", config.workspace.display()))
    } else {
        DoctorCheckOutcome::Fail("Workspace directory is missing or inaccessible".to_string())
    };
    render_doctor_check(renderer, &mut summary, "Workspace", workspace_result)?;

    let cli_version = format!("VT Code {}", env!("CARGO_PKG_VERSION"));
    render_doctor_check(
        renderer,
        &mut summary,
        "CLI Version",
        DoctorCheckOutcome::Pass(cli_version),
    )?;

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Configuration]")?;

    let config_result = match ConfigManager::load_from_workspace(&config.workspace) {
        Ok(manager) => {
            if let Some(path) = manager.config_path() {
                DoctorCheckOutcome::Pass(format!("Loaded from {}", path.display()))
            } else {
                DoctorCheckOutcome::Warn(
                    "Using runtime defaults (no vtcode.toml found)".to_string(),
                )
            }
        }
        Err(err) => DoctorCheckOutcome::Fail(err.to_string()),
    };
    render_doctor_check(renderer, &mut summary, "Config File", config_result)?;

    if let Some(cfg) = vt_cfg {
        render_doctor_check(
            renderer,
            &mut summary,
            "Theme",
            DoctorCheckOutcome::Pass(cfg.agent.theme.clone()),
        )?;

        let model_info = if cfg.agent.small_model.enabled {
            let small_model = if cfg.agent.small_model.model.is_empty() {
                "auto-select".to_string()
            } else {
                cfg.agent.small_model.model.clone()
            };
            format!(
                "{} (+ small model: {})",
                cfg.agent.default_model, small_model
            )
        } else {
            cfg.agent.default_model.clone()
        };
        render_doctor_check(
            renderer,
            &mut summary,
            "Model",
            DoctorCheckOutcome::Pass(model_info),
        )?;

        render_doctor_check(
            renderer,
            &mut summary,
            "Max Turns",
            DoctorCheckOutcome::Pass(format!("{}", cfg.agent.max_conversation_turns)),
        )?;

        render_doctor_check(
            renderer,
            &mut summary,
            "Context Tokens",
            DoctorCheckOutcome::Pass(format!("{}", cfg.context.max_context_tokens)),
        )?;

        render_doctor_check(
            renderer,
            &mut summary,
            "Token Budget",
            DoctorCheckOutcome::Pass("Disabled".to_string()),
        )?;

        let ledger_status = if cfg.context.ledger.enabled {
            format!("Enabled (max {} entries)", cfg.context.ledger.max_entries)
        } else {
            "Disabled".to_string()
        };
        render_doctor_check(
            renderer,
            &mut summary,
            "Decision Ledger",
            DoctorCheckOutcome::Pass(ledger_status),
        )?;

        render_doctor_check(
            renderer,
            &mut summary,
            "Max Tool Loops",
            DoctorCheckOutcome::Pass(format!("{}", cfg.tools.max_tool_loops)),
        )?;

        render_doctor_check(
            renderer,
            &mut summary,
            "HITL Enabled",
            DoctorCheckOutcome::Pass(
                (if cfg.security.human_in_the_loop {
                    "Yes"
                } else {
                    "No"
                })
                .to_string(),
            ),
        )?;

        let policy = match cfg.tools.default_policy {
            ToolPolicy::Allow => "Allow all (no confirmation)",
            ToolPolicy::Deny => "Deny all (security)",
            ToolPolicy::Prompt => "Prompt on tool use",
        };
        render_doctor_check(
            renderer,
            &mut summary,
            "Tool Policy",
            DoctorCheckOutcome::Pass(policy.to_string()),
        )?;

        render_doctor_check(
            renderer,
            &mut summary,
            "PTY Enabled",
            DoctorCheckOutcome::Pass((if cfg.pty.enabled { "Yes" } else { "No" }).to_string()),
        )?;
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[API & Providers]")?;

    let configured_provider = canonical_provider_name(&config.provider);
    let runtime_provider_source = if provider_name.trim().is_empty() {
        config.provider.as_str()
    } else {
        provider_name
    };
    let runtime_provider = canonical_provider_name(runtime_provider_source);

    if runtime_provider == "openresponses" {
        render_doctor_check(
            renderer,
            &mut summary,
            "Provider",
            DoctorCheckOutcome::Pass(format!(
                "Configured '{}', runtime adapter '{}'",
                configured_provider, runtime_provider
            )),
        )?;
    } else if configured_provider == runtime_provider {
        render_doctor_check(
            renderer,
            &mut summary,
            "Provider",
            DoctorCheckOutcome::Pass(format!("Configured and active: {}", runtime_provider)),
        )?;
    } else {
        render_doctor_check(
            renderer,
            &mut summary,
            "Provider",
            DoctorCheckOutcome::Warn(format!(
                "Configured '{}', active '{}'",
                configured_provider, runtime_provider
            )),
        )?;
    }

    let provider_for_api_check = if runtime_provider == "openresponses" {
        configured_provider.as_str()
    } else {
        runtime_provider.as_str()
    };

    let api_key_result = if provider_requires_api_key(provider_for_api_check) {
        match get_api_key(provider_for_api_check, &ApiKeySources::default()) {
            Ok(_) => DoctorCheckOutcome::Pass(format!(
                "API key configured for '{}'",
                provider_for_api_check
            )),
            Err(err) => {
                let detail = err.to_string();
                if detail.contains("Unsupported provider") {
                    DoctorCheckOutcome::Warn(format!(
                        "Skipped API-key validation for unsupported provider '{}'",
                        provider_for_api_check
                    ))
                } else {
                    DoctorCheckOutcome::Fail(format!(
                        "Missing API key for '{}': {} (expected: {})",
                        provider_for_api_check,
                        detail,
                        api_key_env_hint(provider_for_api_check)
                    ))
                }
            }
        }
    } else {
        DoctorCheckOutcome::Pass(format!(
            "Not required for local provider '{}'",
            provider_for_api_check
        ))
    };
    render_doctor_check(renderer, &mut summary, "API Key", api_key_result)?;

    if options.quick {
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Status, "[Skipped Checks]")?;
        renderer.line(
            MessageStyle::Info,
            "Dependencies, MCP status, workspace links, and loaded skills are skipped in quick mode.",
        )?;
    } else {
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Status, "[Dependencies]")?;

        let node_result = match detect_command_version("node", &["--version"]) {
            Ok(version) => DoctorCheckOutcome::Pass(format!("Node.js {}", version)),
            Err(err) => DoctorCheckOutcome::Warn(err),
        };
        render_doctor_check(renderer, &mut summary, "Node.js", node_result)?;

        let npm_result = match detect_command_version("npm", &["--version"]) {
            Ok(version) => DoctorCheckOutcome::Pass(format!("npm {}", version)),
            Err(err) => DoctorCheckOutcome::Warn(err),
        };
        render_doctor_check(renderer, &mut summary, "npm", npm_result)?;

        let ripgrep_result = match detect_command_version("rg", &["--version"]) {
            Ok(version) => DoctorCheckOutcome::Pass(format!(
                "Ripgrep {}",
                version.lines().next().unwrap_or(&version)
            )),
            Err(err) => {
                if err.contains("not found") {
                    DoctorCheckOutcome::Warn(
                        "Not installed (searches will fall back to built-in grep)".to_string(),
                    )
                } else {
                    DoctorCheckOutcome::Warn(err)
                }
            }
        };
        render_doctor_check(renderer, &mut summary, "Ripgrep", ripgrep_result)?;

        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Status, "[External Services]")?;

        let mcp_result = if let Some(cfg) = vt_cfg {
            if cfg.mcp.enabled {
                if let Some(manager) = async_mcp_manager {
                    let status = manager.get_status().await;
                    match status {
                        McpInitStatus::Ready { client } => {
                            let runtime_status = client.get_status();
                            DoctorCheckOutcome::Pass(format!(
                                "{} configured, {} active connection(s)",
                                runtime_status.configured_providers.len(),
                                runtime_status.active_connections
                            ))
                        }
                        McpInitStatus::Initializing { progress } => {
                            DoctorCheckOutcome::Warn(format!("Initializing: {}", progress))
                        }
                        McpInitStatus::Error { message } => {
                            DoctorCheckOutcome::Fail(format!("Init error: {}", message))
                        }
                        McpInitStatus::Disabled => {
                            DoctorCheckOutcome::Pass("Disabled in config".to_string())
                        }
                    }
                } else {
                    DoctorCheckOutcome::Fail(
                        "Enabled in config but manager not initialized".to_string(),
                    )
                }
            } else {
                DoctorCheckOutcome::Pass("Disabled".to_string())
            }
        } else {
            DoctorCheckOutcome::Warn("No MCP configuration".to_string())
        };
        render_doctor_check(renderer, &mut summary, "MCP", mcp_result)?;

        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Status, "[Workspace Links]")?;

        if linked_directories.is_empty() {
            renderer.line(MessageStyle::Output, "  No linked directories")?;
        } else {
            for (idx, entry) in linked_directories.iter().enumerate() {
                renderer.line(
                    MessageStyle::Output,
                    &format!(
                        "  [{}] {} -> {}",
                        idx + 1,
                        entry.display_path,
                        entry.original.display()
                    ),
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
                let mut sorted_skills: Vec<_> = skills.iter().collect();
                sorted_skills.sort_by(|(name_a, _), (name_b, _)| name_a.cmp(name_b));
                for (idx, (name, skill)) in sorted_skills.iter().enumerate() {
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
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Status,
        "═══════════════════════════════════════════════════════════════",
    )?;

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Summary]")?;
    renderer.line(
        MessageStyle::Status,
        &format!(
            "  {} passed, {} warning(s), {} failure(s)",
            summary.passed, summary.warnings, summary.failures
        ),
    )?;
    if summary.failures > 0 {
        renderer.line(
            MessageStyle::Error,
            "[FAIL] Resolve failures first, then rerun `/doctor`.",
        )?;
    } else if summary.warnings > 0 {
        renderer.line(
            MessageStyle::Warning,
            "[WARN] Core checks passed, but there are warnings to review.",
        )?;
    } else {
        renderer.line(
            MessageStyle::Status,
            "[OK] All checks passed. You are ready to use VT Code.",
        )?;
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Recommended Next Actions]")?;
    if summary.failures > 0 {
        renderer.line(
            MessageStyle::Info,
            "[FAIL] Follow the remediation hints above and run `/doctor` again.",
        )?;
    }
    if summary.warnings > 0 {
        renderer.line(
            MessageStyle::Info,
            "[WARN] Address warnings for better reliability (they are not always blocking).",
        )?;
    }
    if options.quick {
        renderer.line(
            MessageStyle::Info,
            "[TIP] Run `/doctor --full` to include dependencies, MCP, links, and skills checks.",
        )?;
    }
    renderer.line(
        MessageStyle::Info,
        "[TIP] For more details: `/skills list` (available skills), `/status` (session), `/compact` (conversation compaction)",
    )?;
    renderer.line(
        MessageStyle::Info,
        "[DOCS] See docs/ide/troubleshooting.md for troubleshooting guidance.",
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
    summary: &mut DoctorSummary,
    label: &str,
    outcome: DoctorCheckOutcome,
) -> Result<()> {
    summary.record(&outcome);
    match outcome {
        DoctorCheckOutcome::Pass(detail) => {
            renderer.line(MessageStyle::Status, &format!("✓  {}: {}", label, detail))?
        }
        DoctorCheckOutcome::Warn(detail) => {
            renderer.line(MessageStyle::Warning, &format!("!  {}: {}", label, detail))?;
            if let Some(suggestion) = get_suggestion_for_issue(label, &detail) {
                renderer.line(MessageStyle::Info, &format!("   -> {}", suggestion))?;
            }
        }
        DoctorCheckOutcome::Fail(detail) => {
            renderer.line(MessageStyle::Error, &format!("✗  {}: {}", label, detail))?;
            if let Some(suggestion) = get_suggestion_for_issue(label, &detail) {
                renderer.line(MessageStyle::Info, &format!("   -> {}", suggestion))?;
            }
        }
    }
    Ok(())
}

fn get_suggestion_for_issue(label: &str, detail: &str) -> Option<String> {
    let label_lower = label.to_ascii_lowercase();
    let detail_lower = detail.to_ascii_lowercase();

    if label_lower.contains("workspace") {
        Some("Ensure workspace directory is accessible and not deleted.".to_string())
    } else if label_lower.contains("api key") {
        Some(
            "Set the provider API key in your environment or vtcode.toml, then rerun `/doctor`."
                .to_string(),
        )
    } else if label_lower.contains("config file") && detail_lower.contains("no vtcode.toml") {
        Some("Run `/init` to generate vtcode.toml with guided setup.".to_string())
    } else if label_lower.contains("node.js") {
        Some("Install Node.js: brew install node (macOS) or see nodejs.org".to_string())
    } else if label_lower == "npm" {
        Some("Install npm with Node.js or update it: npm install -g npm@latest".to_string())
    } else if label_lower.contains("ripgrep") {
        Some("Install Ripgrep: brew install ripgrep (macOS), apt install ripgrep (Linux), or cargo install ripgrep".to_string())
    } else if label_lower.contains("mcp") && detail_lower.contains("error") {
        Some(
            "Check MCP configuration in vtcode.toml, server availability, and timeouts."
                .to_string(),
        )
    } else {
        None
    }
}

fn canonical_provider_name(raw: &str) -> String {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return "gemini".to_string();
    }

    match normalized.as_str() {
        "open-responses" => "openresponses".to_string(),
        "lm studio" | "lm_studio" => "lmstudio".to_string(),
        "z.ai" | "z-ai" => "zai".to_string(),
        _ => normalized,
    }
}

fn provider_requires_api_key(provider: &str) -> bool {
    !matches!(provider, "ollama" | "lmstudio")
}

fn api_key_env_hint(provider: &str) -> String {
    match provider {
        "gemini" => "GEMINI_API_KEY or GOOGLE_API_KEY".to_string(),
        "openai" => "OPENAI_API_KEY".to_string(),
        "anthropic" => "ANTHROPIC_API_KEY".to_string(),
        "deepseek" => "DEEPSEEK_API_KEY".to_string(),
        "openrouter" => "OPENROUTER_API_KEY".to_string(),
        "zai" => "ZAI_API_KEY".to_string(),
        "moonshot" => "MOONSHOT_API_KEY".to_string(),
        "minimax" => "MINIMAX_API_KEY".to_string(),
        "huggingface" => "HF_TOKEN".to_string(),
        "ollama" => "OLLAMA_API_KEY".to_string(),
        "lmstudio" => "LMSTUDIO_API_KEY".to_string(),
        other => format!("{}_API_KEY", other.to_ascii_uppercase()),
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

#[cfg(test)]
mod tests {
    use super::{api_key_env_hint, canonical_provider_name, provider_requires_api_key};

    #[test]
    fn canonical_provider_keeps_openresponses_runtime_adapter() {
        assert_eq!(canonical_provider_name("openresponses"), "openresponses");
        assert_eq!(canonical_provider_name("open-responses"), "openresponses");
    }

    #[test]
    fn canonical_provider_normalizes_variants() {
        assert_eq!(canonical_provider_name(" LM Studio "), "lmstudio");
        assert_eq!(canonical_provider_name("Z.AI"), "zai");
        assert_eq!(canonical_provider_name(""), "gemini");
    }

    #[test]
    fn local_providers_do_not_require_api_keys() {
        assert!(!provider_requires_api_key("ollama"));
        assert!(!provider_requires_api_key("lmstudio"));
        assert!(provider_requires_api_key("openai"));
    }

    #[test]
    fn api_key_hint_is_provider_specific() {
        assert_eq!(api_key_env_hint("openrouter"), "OPENROUTER_API_KEY");
        assert_eq!(api_key_env_hint("huggingface"), "HF_TOKEN");
    }
}
