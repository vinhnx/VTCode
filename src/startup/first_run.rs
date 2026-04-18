use std::path::Path;

use anyhow::{Context, Result, anyhow};
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::PermissionMode;
use vtcode_core::config::constants::{defaults, llm_generation};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::{WorkspaceTrustLevel, update_workspace_trust};
use vtcode_core::utils::file_utils::ensure_dir_exists_sync;
use vtcode_core::{initialize_dot_folder, update_model_preference};

use super::dependency_advisories::render_optional_search_tools_notice;
use super::first_run_prompts::{
    StartupMode, default_model_for_provider, prompt_lightweight_model, prompt_model,
    prompt_persistent_memory, prompt_provider, prompt_reasoning_effort, prompt_startup_mode,
    prompt_trust, resolve_initial_persistent_memory_enabled, resolve_initial_provider,
    resolve_initial_startup_mode,
};

/// Drive the first-run interactive setup wizard when a workspace lacks VT Code artifacts.
pub(crate) async fn maybe_run_first_run_setup(
    args: &Cli,
    workspace: &Path,
    config: &mut VTCodeConfig,
) -> Result<bool> {
    if !is_fresh_workspace(workspace) {
        return Ok(false);
    }

    if args.provider.is_some() || args.model.is_some() {
        return Ok(false);
    }

    if let Some(command) = &args.command {
        match command {
            Commands::Chat | Commands::ChatVerbose => {}
            _ => return Ok(false),
        }
    }

    let full_auto_requested = args.full_auto.is_some();
    let non_interactive = args.skip_confirmations || full_auto_requested;
    let mode = if non_interactive {
        SetupMode::NonInteractive {
            full_auto: full_auto_requested,
        }
    } else {
        SetupMode::Interactive
    };

    run_first_run_setup(workspace, config, mode).await?;
    Ok(true)
}

enum SetupMode {
    Interactive,
    NonInteractive { full_auto: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StartupModeConfig {
    permission_mode: PermissionMode,
}

fn is_fresh_workspace(workspace: &Path) -> bool {
    let config_path = workspace.join("vtcode.toml");
    let dot_dir = workspace.join(".vtcode");
    !config_path.exists() && !dot_dir.exists()
}

async fn run_first_run_setup(
    workspace: &Path,
    config: &mut VTCodeConfig,
    mode: SetupMode,
) -> Result<()> {
    initialize_dot_folder().await.ok();

    if !workspace.exists() {
        return Err(anyhow!(
            "Workspace '{}' does not exist for setup",
            workspace.display()
        ));
    }

    let workspace_dot_dir = workspace.join(".vtcode");
    ensure_dir_exists_sync(&workspace_dot_dir).with_context(|| {
        format!(
            "Failed to create workspace .vtcode directory at {}",
            workspace_dot_dir.display()
        )
    })?;

    let mut renderer = AnsiRenderer::stdout();
    renderer.line(
        MessageStyle::Info,
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
    )?;
    renderer.line(MessageStyle::Info, "  VT Code - First-time setup wizard")?;
    renderer.line(
        MessageStyle::Info,
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
    )?;
    let (provider, model, lightweight_model, reasoning, startup_mode, persistent_memory, trust) =
        match mode {
            SetupMode::Interactive => {
                renderer.line(
                    MessageStyle::Status,
                    "Let's configure your default provider, model, lightweight model route, reasoning effort, startup mode, persistent memory, and workspace trust.",
                )?;
                renderer.line(
                    MessageStyle::Status,
                    "Press Enter to accept the suggested value in brackets.",
                )?;
                renderer.line(MessageStyle::Info, "")?;

                let provider = resolve_initial_provider(config);
                let provider = prompt_provider(&mut renderer, provider)?;
                renderer.line(MessageStyle::Info, &api_key_hint(provider))?;
                renderer.line(MessageStyle::Info, "")?;

                let default_model = default_model_for_provider(provider);
                let model = prompt_model(&mut renderer, provider, default_model)?;
                renderer.line(MessageStyle::Info, "")?;

                let lightweight_model = prompt_lightweight_model(&mut renderer, provider, &model)?;
                renderer.line(MessageStyle::Info, "")?;

                let reasoning =
                    prompt_reasoning_effort(&mut renderer, config.agent.reasoning_effort)?;
                renderer.line(MessageStyle::Info, "")?;

                let startup_mode =
                    prompt_startup_mode(&mut renderer, resolve_initial_startup_mode(config))?;
                renderer.line(MessageStyle::Info, "")?;

                let persistent_memory = prompt_persistent_memory(
                    &mut renderer,
                    resolve_initial_persistent_memory_enabled(config),
                )?;
                renderer.line(MessageStyle::Info, "")?;

                let trust = prompt_trust(&mut renderer, WorkspaceTrustLevel::ToolsPolicy)?;
                renderer.line(MessageStyle::Info, "")?;

                (
                    provider,
                    model,
                    lightweight_model,
                    reasoning,
                    startup_mode,
                    persistent_memory,
                    trust,
                )
            }
            SetupMode::NonInteractive { full_auto } => {
                renderer.line(
                    MessageStyle::Status,
                    "Non-interactive setup flags detected. Applying defaults without prompts.",
                )?;
                renderer.line(MessageStyle::Info, "")?;

                let provider = resolve_initial_provider(config);
                let default_model = default_model_for_provider(provider);
                let model = default_model.to_owned();
                let lightweight_model = String::new();
                let reasoning = config.agent.reasoning_effort;
                let startup_mode = resolve_initial_startup_mode(config);
                let persistent_memory = resolve_initial_persistent_memory_enabled(config);
                let trust = if full_auto {
                    WorkspaceTrustLevel::FullAuto
                } else {
                    WorkspaceTrustLevel::ToolsPolicy
                };

                renderer.line(
                    MessageStyle::Info,
                    &format!("Provider: {}", provider.label()),
                )?;
                renderer.line(MessageStyle::Info, &format!("Model: {}", model))?;
                renderer.line(
                    MessageStyle::Info,
                    "Lightweight model: Automatic (same-provider lightweight route)",
                )?;
                renderer.line(
                    MessageStyle::Info,
                    &format!("Reasoning effort: {}", reasoning.as_str()),
                )?;
                renderer.line(
                    MessageStyle::Info,
                    &format!("Startup mode: {}", startup_mode.label()),
                )?;
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Persistent memory: {}",
                        persistent_memory_label(persistent_memory)
                    ),
                )?;
                renderer.line(MessageStyle::Info, &api_key_hint(provider))?;
                renderer.line(
                    MessageStyle::Info,
                    &format!("Workspace trust: {}", trust_label(trust)),
                )?;
                renderer.line(MessageStyle::Info, "")?;

                (
                    provider,
                    model,
                    lightweight_model,
                    reasoning,
                    startup_mode,
                    persistent_memory,
                    trust,
                )
            }
        };

    renderer.line(
        MessageStyle::Status,
        "Saving your configuration to vtcode.toml ...",
    )?;

    // Compute provider key once to avoid repeated allocations from `to_string()`.
    let provider_key = provider.to_string();
    // Set API key environment name from provider defaults; do this once to avoid recomputing.
    config.agent.api_key_env = provider.default_api_key_env().to_owned();
    apply_selection(
        config,
        &provider_key,
        &model,
        &lightweight_model,
        reasoning,
        startup_mode,
        persistent_memory,
    );

    let config_path = workspace.join("vtcode.toml");
    ConfigManager::save_config_to_path(&config_path, config).with_context(|| {
        format!(
            "Failed to write initial configuration to {}",
            config_path.display()
        )
    })?;

    update_model_preference(&provider_key, &model).await.ok();

    update_workspace_trust(workspace, trust)
        .await
        .with_context(|| {
            format!(
                "Failed to persist workspace trust level for {}",
                workspace.display()
            )
        })?;

    renderer.line(MessageStyle::Info, "")?;
    render_setup_summary(
        &mut renderer,
        provider,
        &model,
        &lightweight_model,
        reasoning,
        startup_mode,
        persistent_memory,
        trust,
    )?;
    render_optional_search_tools_notice(&mut renderer).await?;
    renderer.line(
        MessageStyle::Status,
        "Tip: run `/init` anytime to rerun this setup and refresh other workspace defaults.",
    )?;
    renderer.line(MessageStyle::Info, "")?;

    Ok(())
}

fn apply_selection(
    config: &mut VTCodeConfig,
    provider_key: &str,
    model: &str,
    lightweight_model: &str,
    reasoning: ReasoningEffortLevel,
    startup_mode: StartupMode,
    persistent_memory_enabled: bool,
) {
    config.agent.provider = provider_key.to_owned();
    config.agent.default_model = model.to_owned();
    config.agent.small_model.model = lightweight_model.to_owned();
    config.agent.reasoning_effort = reasoning;
    let startup_mode_config = startup_mode_config(startup_mode);
    config.permissions.default_mode = startup_mode_config.permission_mode;
    config.features.memories = persistent_memory_enabled;
    config.agent.persistent_memory.enabled = persistent_memory_enabled;
    config.agent.theme = defaults::DEFAULT_THEME.to_owned();
    config.agent.max_conversation_turns = defaults::DEFAULT_MAX_CONVERSATION_TURNS;
    config.agent.temperature = llm_generation::DEFAULT_TEMPERATURE;
}

fn startup_mode_config(mode: StartupMode) -> StartupModeConfig {
    match mode {
        StartupMode::Edit => StartupModeConfig {
            permission_mode: PermissionMode::Default,
        },
        StartupMode::Auto => StartupModeConfig {
            permission_mode: PermissionMode::Auto,
        },
        StartupMode::Plan => StartupModeConfig {
            permission_mode: PermissionMode::Plan,
        },
    }
}

fn render_setup_summary(
    renderer: &mut AnsiRenderer,
    provider: Provider,
    model: &str,
    lightweight_model: &str,
    reasoning: ReasoningEffortLevel,
    startup_mode: StartupMode,
    persistent_memory_enabled: bool,
    trust: WorkspaceTrustLevel,
) -> Result<()> {
    for (style, line) in setup_summary_lines(
        provider,
        model,
        lightweight_model,
        reasoning,
        startup_mode,
        persistent_memory_enabled,
        trust,
    ) {
        renderer.line(style, &line)?;
    }
    renderer.line(MessageStyle::Info, "")?;
    Ok(())
}

fn setup_summary_lines(
    provider: Provider,
    model: &str,
    lightweight_model: &str,
    reasoning: ReasoningEffortLevel,
    startup_mode: StartupMode,
    persistent_memory_enabled: bool,
    trust: WorkspaceTrustLevel,
) -> Vec<(MessageStyle, String)> {
    let mut lines = Vec::with_capacity(12);
    lines.push((
        MessageStyle::Status,
        format!(
            "Setup complete. Provider: {} • Model: {} • Lightweight: {} • Reasoning: {}",
            provider.label(),
            model,
            lightweight_model_label(lightweight_model, model),
            reasoning.as_str()
        ),
    ));
    lines.push((
        MessageStyle::Info,
        format!(
            "Startup: {} • Persistent memory: {} • Trust: {}",
            startup_mode.label(),
            persistent_memory_label(persistent_memory_enabled),
            trust_label(trust)
        ),
    ));
    lines.push((
        MessageStyle::Status,
        format!("Auth: {}", api_key_hint(provider)),
    ));
    lines.push((MessageStyle::Status, "What's available now:".to_string()));
    lines.extend(
        capability_highlight_lines(persistent_memory_enabled)
            .into_iter()
            .map(|line| (MessageStyle::Info, line)),
    );
    lines
}

fn capability_highlight_lines(persistent_memory_enabled: bool) -> Vec<String> {
    vec![
        "- Switch modes anytime with `/mode` or `Shift+Tab` to move between Edit, Auto, and Plan.".to_string(),
        "- Auto mode uses classifier-backed permission checks inside the normal session. `--full-auto` is separate and uses the explicit `[automation.full_auto]` allow-list.".to_string(),
        format!(
            "- Persistent repository memory is {} for this workspace. Change `[features].memories` or `agent.persistent_memory.enabled` later in `vtcode.toml` if you want a different default.",
            persistent_memory_label(persistent_memory_enabled).to_ascii_lowercase()
        ),
        "- Subagents let you delegate bounded work; use `/agent`, `/agents`, or the Local Agents drawer.".to_string(),
        "- Skills add reusable capabilities; browse them with `/skills` or the CLI skills commands.".to_string(),
        "- Prompt suggestions and the lightweight model route help with faster suggestions, memory triage, and smaller delegated tasks.".to_string(),
        "- Use `/loop` for recurring prompts in the current session and `/schedule` for durable scheduled tasks and automations.".to_string(),
        "- Advanced config-only permission modes such as `accept_edits`, `dont_ask`, and `bypass_permissions` remain available in `vtcode.toml` if you need them later.".to_string(),
    ]
}

fn lightweight_model_label(lightweight_model: &str, model: &str) -> String {
    if lightweight_model.trim().is_empty() {
        "automatic".to_string()
    } else if lightweight_model == model {
        "main model".to_string()
    } else {
        lightweight_model.to_string()
    }
}

fn persistent_memory_label(enabled: bool) -> &'static str {
    if enabled { "On" } else { "Off" }
}

fn api_key_hint(provider: Provider) -> String {
    if provider.is_local() {
        "No API key required for local provider.".to_string()
    } else {
        format!(
            "Set {} in your environment.",
            provider.default_api_key_env()
        )
    }
}

fn trust_label(level: WorkspaceTrustLevel) -> &'static str {
    match level {
        WorkspaceTrustLevel::ToolsPolicy => "Tools policy",
        WorkspaceTrustLevel::FullAuto => "Full auto",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::loader::VTCodeConfig;

    fn base_config() -> VTCodeConfig {
        VTCodeConfig::default()
    }

    #[test]
    fn edit_startup_mode_writes_permission_default() {
        let mut config = base_config();

        apply_selection(
            &mut config,
            "openai",
            "gpt-5.4",
            "",
            ReasoningEffortLevel::None,
            StartupMode::Edit,
            false,
        );

        assert_eq!(config.permissions.default_mode, PermissionMode::Default);
    }

    #[test]
    fn auto_startup_mode_writes_auto_permission_mode() {
        let mut config = base_config();

        apply_selection(
            &mut config,
            "openai",
            "gpt-5.4",
            "",
            ReasoningEffortLevel::None,
            StartupMode::Auto,
            false,
        );

        assert_eq!(config.permissions.default_mode, PermissionMode::Auto);
    }

    #[test]
    fn plan_startup_mode_writes_plan_permission_mode() {
        let mut config = base_config();

        apply_selection(
            &mut config,
            "openai",
            "gpt-5.4",
            "",
            ReasoningEffortLevel::None,
            StartupMode::Plan,
            false,
        );

        assert_eq!(config.permissions.default_mode, PermissionMode::Plan);
    }

    #[test]
    fn persistent_memory_selection_persists_opt_in() {
        let mut config = base_config();

        apply_selection(
            &mut config,
            "openai",
            "gpt-5.4",
            "",
            ReasoningEffortLevel::None,
            StartupMode::Edit,
            true,
        );

        assert!(config.features.memories);
        assert!(config.agent.persistent_memory.enabled);
    }

    #[test]
    fn persistent_memory_selection_persists_opt_out() {
        let mut config = base_config();
        config.features.memories = true;
        config.agent.persistent_memory.enabled = true;

        apply_selection(
            &mut config,
            "openai",
            "gpt-5.4",
            "",
            ReasoningEffortLevel::None,
            StartupMode::Edit,
            false,
        );

        assert!(!config.features.memories);
        assert!(!config.agent.persistent_memory.enabled);
    }

    #[test]
    fn setup_summary_mentions_new_capabilities() {
        let lines = setup_summary_lines(
            Provider::OpenAI,
            "gpt-5.4",
            "",
            ReasoningEffortLevel::None,
            StartupMode::Auto,
            false,
            WorkspaceTrustLevel::ToolsPolicy,
        );
        let rendered = lines
            .into_iter()
            .map(|(_style, line)| line)
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("`--full-auto` is separate"));
        assert!(rendered.contains("Persistent repository memory"));
        assert!(rendered.contains("Subagents"));
        assert!(rendered.contains("Skills"));
        assert!(rendered.contains("lightweight model route"));
        assert!(rendered.contains("/loop"));
        assert!(rendered.contains("/schedule"));
    }
}
