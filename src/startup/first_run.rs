use std::path::Path;

use anyhow::{Context, Result, anyhow};
use vtcode_config::api_keys::{
    CredentialSource, DiscoveredProvider, discover_available_providers, provider_credential_detail,
};
use vtcode_core::cli::args::{Cli, Commands};
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
    default_model_for_provider, prompt_api_key_interactive, prompt_lightweight_model, prompt_model,
    prompt_persistent_memory, prompt_provider, prompt_reasoning_effort, prompt_trust,
    resolve_initial_persistent_memory_enabled, resolve_initial_provider,
};

/// Drive the first-run interactive setup wizard when a workspace lacks VT Code artifacts.
pub(crate) async fn maybe_run_first_run_setup(args: &Cli, workspace: &Path, config: &mut VTCodeConfig) -> Result<bool> {
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
        SetupMode::NonInteractive { full_auto: full_auto_requested }
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

fn is_fresh_workspace(workspace: &Path) -> bool {
    let config_path = workspace.join("vtcode.toml");
    let dot_dir = workspace.join(".vtcode");
    !config_path.exists() && !dot_dir.exists()
}

async fn run_first_run_setup(workspace: &Path, config: &mut VTCodeConfig, mode: SetupMode) -> Result<()> {
    initialize_dot_folder().await.ok();

    if !workspace.exists() {
        return Err(anyhow!("Workspace '{}' does not exist for setup", workspace.display()));
    }

    let workspace_dot_dir = workspace.join(".vtcode");
    ensure_dir_exists_sync(&workspace_dot_dir)
        .with_context(|| format!("Failed to create workspace .vtcode directory at {}", workspace_dot_dir.display()))?;

    let mut renderer = AnsiRenderer::stdout();
    renderer.line(MessageStyle::Info, "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")?;
    renderer.line(MessageStyle::Info, "  VT Code - First-time setup wizard")?;
    renderer.line(MessageStyle::Info, "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")?;

    // Discover providers that already have a usable credential (shell-exported
    // env vars, loaded .env, OS keyring, OAuth, managed auth, or local). This
    // drives the provider list's "ready" markers and the default cursor, and
    // lets the API-key prompt skip re-pasting keys the user already has.
    let discovered = discover_available_providers();
    render_discovery_summary(&mut renderer, &discovered)?;

    let (provider, model, lightweight_model, reasoning, persistent_memory, trust) = match mode {
        SetupMode::Interactive => {
            renderer.line(
                    MessageStyle::Status,
                    "Let's configure your default provider, model, lightweight model route, reasoning effort, persistent memory, and workspace trust.",
                )?;
            renderer.line(MessageStyle::Status, "Press Enter to accept the suggested value in brackets.")?;
            renderer.line(MessageStyle::Info, "")?;

            let provider = resolve_initial_provider(config, &discovered);
            let provider = prompt_provider(&mut renderer, provider, &discovered)?;
            renderer.line(MessageStyle::Info, "")?;

            // Interactive API key entry — skips the paste prompt when the key is
            // already in the environment or OS keyring; stores pasted keys in
            // the OS keyring (not workspace .env).
            prompt_api_key_interactive(&mut renderer, provider)?;
            renderer.line(MessageStyle::Info, "")?;

            let default_model = default_model_for_provider(provider);
            let model = prompt_model(&mut renderer, provider, default_model)?;
            renderer.line(MessageStyle::Info, "")?;

            let lightweight_model = prompt_lightweight_model(&mut renderer, provider, &model)?;
            renderer.line(MessageStyle::Info, "")?;

            let reasoning = prompt_reasoning_effort(&mut renderer, config.agent.reasoning_effort)?;
            renderer.line(MessageStyle::Info, "")?;

            let persistent_memory =
                prompt_persistent_memory(&mut renderer, resolve_initial_persistent_memory_enabled(config))?;
            renderer.line(MessageStyle::Info, "")?;

            let trust = prompt_trust(&mut renderer, WorkspaceTrustLevel::ToolsPolicy)?;
            renderer.line(MessageStyle::Info, "")?;

            (provider, model, lightweight_model, reasoning, persistent_memory, trust)
        }
        SetupMode::NonInteractive { full_auto } => {
            renderer.line(
                MessageStyle::Status,
                "Non-interactive setup flags detected. Applying defaults without prompts.",
            )?;
            renderer.line(MessageStyle::Info, "")?;

            let provider = resolve_initial_provider(config, &discovered);
            let default_model = default_model_for_provider(provider);
            let model = default_model.to_owned();
            let lightweight_model = String::new();
            let reasoning = config.agent.reasoning_effort;
            let persistent_memory = resolve_initial_persistent_memory_enabled(config);
            let trust = if full_auto {
                WorkspaceTrustLevel::FullAuto
            } else {
                WorkspaceTrustLevel::ToolsPolicy
            };

            renderer.line(MessageStyle::Info, &format!("Provider: {}", provider.label()))?;
            renderer.line(MessageStyle::Info, &format!("Model: {model}"))?;
            renderer.line(MessageStyle::Info, "Lightweight model: Automatic (same-provider lightweight route)")?;
            renderer.line(MessageStyle::Info, &format!("Reasoning effort: {}", reasoning.as_str()))?;
            renderer.line(
                MessageStyle::Info,
                &format!("Persistent memory: {}", persistent_memory_label(persistent_memory)),
            )?;
            renderer.line(MessageStyle::Info, &api_key_hint(provider))?;
            renderer.line(MessageStyle::Info, &format!("Workspace trust: {}", trust_label(trust)))?;
            renderer.line(MessageStyle::Info, "")?;

            (provider, model, lightweight_model, reasoning, persistent_memory, trust)
        }
    };

    renderer.line(MessageStyle::Status, "Saving your configuration to vtcode.toml ...")?;

    // Compute provider key once to avoid repeated allocations from `to_string()`.
    let provider_key = provider.to_string();
    // Set API key environment name from provider defaults; do this once to avoid recomputing.
    config.agent.api_key_env = provider.default_api_key_env().to_owned();
    apply_selection(config, &provider_key, &model, &lightweight_model, reasoning, persistent_memory);

    let config_path = workspace.join("vtcode.toml");
    ConfigManager::save_config_to_path(&config_path, config)
        .with_context(|| format!("Failed to write initial configuration to {}", config_path.display()))?;

    update_model_preference(&provider_key, &model).await.ok();

    update_workspace_trust(workspace, trust)
        .await
        .with_context(|| format!("Failed to persist workspace trust level for {}", workspace.display()))?;

    renderer.line(MessageStyle::Info, "")?;
    render_setup_summary(&mut renderer, provider, &model, &lightweight_model, reasoning, persistent_memory, trust)?;
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
    persistent_memory_enabled: bool,
) {
    config.agent.provider = provider_key.to_owned();
    config.agent.default_model = model.to_owned();
    config.agent.small_model.model = lightweight_model.to_owned();
    config.agent.reasoning_effort = reasoning;
    config.default_primary_agent = defaults::DEFAULT_PRIMARY_AGENT_NAME.to_owned();
    config.features.memories = persistent_memory_enabled;
    config.agent.persistent_memory.enabled = persistent_memory_enabled;
    config.agent.theme = defaults::DEFAULT_THEME.to_owned();
    config.agent.max_conversation_turns = defaults::DEFAULT_MAX_CONVERSATION_TURNS;
    config.agent.temperature = llm_generation::DEFAULT_TEMPERATURE;
}

fn render_setup_summary(
    renderer: &mut AnsiRenderer,
    provider: Provider,
    model: &str,
    lightweight_model: &str,
    reasoning: ReasoningEffortLevel,
    persistent_memory_enabled: bool,
    trust: WorkspaceTrustLevel,
) -> Result<()> {
    for (style, line) in
        setup_summary_lines(provider, model, lightweight_model, reasoning, persistent_memory_enabled, trust)
    {
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
            "Primary agent: {} • Persistent memory: {} • Trust: {}",
            defaults::DEFAULT_PRIMARY_AGENT_NAME,
            persistent_memory_label(persistent_memory_enabled),
            trust_label(trust)
        ),
    ));
    lines.push((MessageStyle::Status, format!("Auth: {}", api_key_hint(provider))));
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
        "- Use `/plan` for read-only planning, then finish planning before implementation.".to_string(),
        "- Auto permission review uses classifier-backed checks inside the normal session. `--full-auto` is separate and uses the explicit `[automation.full_auto]` allow-list.".to_string(),
        format!(
            "- Persistent repository memory is {} for this workspace. Change `[features].memories` or `agent.persistent_memory.enabled` later in `vtcode.toml` if you want a different default.",
            persistent_memory_label(persistent_memory_enabled).to_ascii_lowercase()
        ),
        "- Subagents let you delegate bounded work; use `/agent`, `/agents`, or the Local Agents drawer.".to_string(),
        "- Skills add reusable capabilities; browse them with `/skills` or the CLI skills commands.".to_string(),
        "- Prompt suggestions and the lightweight model route help with faster suggestions, memory triage, and smaller delegated tasks.".to_string(),
        "- Granular permissions are configured with `[permissions]` defaults and allow/ask/auto/deny lists in `vtcode.toml`.".to_string(),
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
        return "No API key required for local provider.".to_string();
    }
    if provider.uses_managed_auth() {
        return format!("Run `vtcode login {}` to authenticate.", provider);
    }
    match provider_credential_detail(provider) {
        Some(detail) => match detail.source {
            CredentialSource::Env => {
                let var = detail.env_var.unwrap_or_else(|| provider.default_api_key_env());
                format!("Using {var} from your environment.")
            }
            CredentialSource::SecureStorage => {
                format!("Using the {} key from your OS keyring.", provider.label())
            }
            CredentialSource::OAuth => {
                format!("Using your active {} OAuth session.", provider.label())
            }
            CredentialSource::ManagedAuth | CredentialSource::Local => {
                format!("{} is ready.", provider.label())
            }
        },
        None => format!("Set {} in your environment, or add it with `/secret`.", provider.default_api_key_env()),
    }
}

/// Print a one-line summary of which providers already have a usable credential,
/// so the user knows they can pick any of them without re-entering a key.
fn render_discovery_summary(renderer: &mut AnsiRenderer, discovered: &[DiscoveredProvider]) -> Result<()> {
    // Only "real" credentials (env/keyring/OAuth) are worth advertising — local
    // and managed-auth providers are always nominally ready but not what most
    // users are choosing between on first run.
    let ready: Vec<&DiscoveredProvider> = discovered
        .iter()
        .filter(|d| {
            matches!(d.source, CredentialSource::Env | CredentialSource::SecureStorage | CredentialSource::OAuth)
        })
        .collect();

    if ready.is_empty() {
        // No real credentials — but local providers (Ollama / LM Studio /
        // llama.cpp) need no key, so the user is not stuck. Mention them so the
        // empty state has a concrete next step rather than a dead end.
        let local: Vec<&str> = discovered
            .iter()
            .filter(|d| matches!(d.source, CredentialSource::Local))
            .map(|d| d.provider.label())
            .collect();
        let local_hint = if local.is_empty() {
            String::new()
        } else {
            format!(" You can also use a local provider ({}) — no key required.", local.join(", "))
        };
        renderer.line(
            MessageStyle::Info,
            &format!(
                "No provider API keys found in your environment or OS keyring. You can add one with `/secret`, export one (e.g. in ~/.zshrc) and re-run /init, or press Enter to skip.{local_hint}"
            ),
        )?;
    } else {
        // Surface the specific env var that was found so the user knows
        // exactly what vtcode read (e.g. GOOGLE_API_KEY vs GEMINI_API_KEY).
        let labels: Vec<String> = ready
            .iter()
            .map(|d| match d.env_var {
                Some(var) => format!("{} ({var})", d.provider.label()),
                None => d.provider.label().to_string(),
            })
            .collect();
        renderer.line(
            MessageStyle::Status,
            &format!("Found credentials for: {}. Pick any of these — no need to re-enter a key.", labels.join(", ")),
        )?;
    }
    renderer.line(MessageStyle::Info, "")?;
    Ok(())
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
    fn edit_startup_mode_keeps_permissions_config_shape() {
        let mut config = base_config();

        apply_selection(&mut config, "openai", "gpt-5.4", "", ReasoningEffortLevel::None, false);

        assert!(config.permissions.allow.is_empty());
        assert_eq!(config.default_primary_agent, "build");
    }

    #[test]
    fn first_run_selection_does_not_write_old_mode_fields() {
        let mut config = base_config();

        apply_selection(&mut config, "openai", "gpt-5.4", "", ReasoningEffortLevel::None, false);

        assert!(config.permissions.allow.is_empty());
        assert!(config.permissions.deny.is_empty());
        assert_eq!(config.default_primary_agent, "build");
    }

    #[test]
    fn persistent_memory_selection_persists_opt_in() {
        let mut config = base_config();

        apply_selection(&mut config, "openai", "gpt-5.4", "", ReasoningEffortLevel::None, true);

        assert!(config.features.memories);
        assert!(config.agent.persistent_memory.enabled);
    }

    #[test]
    fn persistent_memory_selection_persists_opt_out() {
        let mut config = base_config();
        config.features.memories = true;
        config.agent.persistent_memory.enabled = true;

        apply_selection(&mut config, "openai", "gpt-5.4", "", ReasoningEffortLevel::None, false);

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
            false,
            WorkspaceTrustLevel::ToolsPolicy,
        );
        let rendered = lines.into_iter().map(|(_style, line)| line).collect::<Vec<_>>().join("\n");

        assert!(rendered.contains("`--full-auto` is separate"));
        assert!(rendered.contains("Persistent repository memory"));
        assert!(rendered.contains("Primary agent: build"));
        assert!(rendered.contains("Subagents"));
        assert!(rendered.contains("Skills"));
        assert!(rendered.contains("lightweight model route"));
        assert!(rendered.contains("Granular permissions"));
        assert!(!rendered.contains("Startup mode"));
        assert!(!rendered.contains("permission mode"));
    }
}
