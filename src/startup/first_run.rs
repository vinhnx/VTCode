use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use vtcode_config::api_keys::{
    CredentialSource, DiscoveredProvider, discover_available_providers_with_mode, provider_credential_detail_with_mode,
};
use vtcode_config::auth::AuthCredentialsStoreMode;
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::constants::{defaults, llm_generation, tool_limits};
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::config::{ConfigService, ConfigWriteRequest, ConfigWriteStrategy, ConfigWriteTarget};
use vtcode_core::utils::dot_config::{WorkspaceTrustLevel, update_workspace_trust};
use vtcode_core::utils::file_utils::ensure_dir_exists_sync;
use vtcode_core::{initialize_dot_folder, update_model_preference};
use vtcode_tui_runtime::utils::ansi::{AnsiRenderer, MessageStyle};

use super::first_run_prompts::{
    default_model_for_provider, prompt_agent_mode, prompt_api_key_interactive, prompt_bundled_skills_consent,
    prompt_lightweight_model, prompt_model, prompt_permission_mode, prompt_persistent_memory, prompt_provider,
    prompt_reasoning_effort, prompt_trust, resolve_bundled_skills_answer, resolve_initial_persistent_memory_enabled,
    resolve_initial_provider,
};
use vtcode_config::consent::BUNDLED_SKILLS_SETTING_PATH;
use vtcode_config::core::permissions::DefaultPermissionMode;

/// Drive the first-run interactive setup wizard when a workspace lacks VT Code artifacts.
pub(crate) async fn maybe_run_first_run_setup(args: &Cli, workspace: &Path, config: &mut VTCodeConfig) -> Result<bool> {
    if !is_fresh_workspace(workspace) {
        return Ok(false);
    }

    if args.provider.is_some() || args.model.is_some() {
        return Ok(false);
    }

    // If the user has a configuration file, they are configured: do not run
    // first-run setup even if this workspace lacks vtcode.toml. (Checking the
    // effective provider/model fields cannot work here — struct defaults fill
    // them on every fresh install, which previously made this wizard
    // unreachable.)
    if user_config_exists() {
        return Ok(false);
    }

    // Automatic setup requires an interactive terminal: without a TTY the
    // wizard could only accept silent defaults while writing configuration
    // files as a side effect of an unrelated invocation (including test
    // harnesses). Explicit `vtcode setup` always runs; its text fallbacks
    // handle piped stdin.
    if !std::io::stdin().is_terminal() {
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
            skip_confirmations: args.skip_confirmations,
        }
    } else {
        SetupMode::Interactive
    };

    run_first_run_setup(workspace, config, mode).await?;
    Ok(true)
}

enum SetupMode {
    Interactive,
    NonInteractive { full_auto: bool, skip_confirmations: bool },
}

/// Interactive wizard steps, in prompt order. The consent step keeps its
/// number even when a prior answer short-circuits it, so progress stays
/// truthful across reruns.
const TOTAL_SETUP_STEPS: u8 = 10;

fn step_header(current: u8, title: &str) -> String {
    format!("Step {current} of {TOTAL_SETUP_STEPS} — {title}")
}

/// Whether the user layer has a configuration file. Absent on a truly fresh
/// install; present once the user has configured anything (including a prior
/// wizard run, which records the chosen provider/model there).
fn user_config_exists() -> bool {
    match vtcode_commons::VtCodePaths::resolve() {
        Ok(paths) => paths.config_file().exists(),
        Err(error) => {
            tracing::warn!(%error, "Could not resolve user config path; skipping first-run setup");
            true
        }
    }
}

fn is_fresh_workspace(workspace: &Path) -> bool {
    let explicit_override = vtcode_config::loader::explicit_config_path();
    if let Some(path) = explicit_override {
        // A session-explicit config file is the workspace's config source:
        // when it exists the workspace is not fresh, regardless of whether
        // the default `vtcode.toml` paths exist.
        return !path.exists();
    }
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
    let storage_mode = config.agent.credential_storage_mode;
    let discovered = discover_available_providers_with_mode(storage_mode);
    render_discovery_summary(&mut renderer, &discovered)?;

    // Ask-first consent is recorded once in the user layer; later workspaces reuse it.
    let prior_consent = resolve_bundled_skills_answer(workspace);

    // Same primary-agent catalog the desktop mode picker offers.
    let workspace_for_modes = workspace.to_path_buf();
    let agent_specs = tokio::task::spawn_blocking(move || {
        vtcode_config::discover_subagents(&vtcode_config::SubagentDiscoveryInput::new(workspace_for_modes))
            .map(|discovered| {
                discovered
                    .effective
                    .into_iter()
                    .filter(vtcode_config::SubagentSpec::is_primary)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    let (provider, model, lightweight_model, reasoning, persistent_memory, trust, permission_mode, agent_name, consent) =
        match mode {
            SetupMode::Interactive => {
                renderer.line(
                    MessageStyle::Status,
                    "Let's configure your default provider, model, lightweight model route, reasoning effort, persistent memory, workspace trust, permission mode, and agent mode.",
                )?;
                renderer.line(MessageStyle::Status, "Press Enter to accept the suggested value in brackets.")?;
                renderer.line(MessageStyle::Info, "")?;

                renderer.line(MessageStyle::Status, &step_header(1, "Provider"))?;
                let provider = resolve_initial_provider(config, &discovered);
                let provider = prompt_provider(&mut renderer, provider, &discovered, &config.providers_whitelist)?;
                renderer.line(MessageStyle::Info, "")?;

                // Interactive API key entry — skips the paste prompt when the key is
                // already in the environment or OS keyring; stores pasted keys in
                // the OS keyring (not workspace .env).
                renderer.line(MessageStyle::Status, &step_header(2, "API key"))?;
                prompt_api_key_interactive(&mut renderer, provider, storage_mode)?;
                renderer.line(MessageStyle::Info, "")?;

                renderer.line(MessageStyle::Status, &step_header(3, "Model"))?;
                let default_model = default_model_for_provider(provider);
                let model = prompt_model(&mut renderer, provider, default_model)?;
                renderer.line(MessageStyle::Info, "")?;

                renderer.line(MessageStyle::Status, &step_header(4, "Lightweight model"))?;
                let lightweight_model = prompt_lightweight_model(&mut renderer, provider, &model)?;
                renderer.line(MessageStyle::Info, "")?;

                renderer.line(MessageStyle::Status, &step_header(5, "Reasoning effort"))?;
                let reasoning = prompt_reasoning_effort(&mut renderer, config.agent.reasoning_effort)?;
                renderer.line(MessageStyle::Info, "")?;

                renderer.line(MessageStyle::Status, &step_header(6, "Persistent memory"))?;
                let persistent_memory =
                    prompt_persistent_memory(&mut renderer, resolve_initial_persistent_memory_enabled(config))?;
                renderer.line(MessageStyle::Info, "")?;

                renderer.line(MessageStyle::Status, &step_header(7, "Workspace trust"))?;
                let trust = prompt_trust(&mut renderer, WorkspaceTrustLevel::ToolsPolicy)?;
                renderer.line(MessageStyle::Info, "")?;

                renderer.line(MessageStyle::Status, &step_header(8, "Permission mode"))?;
                let permission_mode = prompt_permission_mode(&mut renderer, config.permissions.default_mode)?;
                renderer.line(MessageStyle::Info, "")?;

                renderer.line(MessageStyle::Status, &step_header(9, "Agent mode"))?;
                let agent_name = prompt_agent_mode(&mut renderer, &agent_specs, &config.default_primary_agent)?;
                renderer.line(MessageStyle::Info, "")?;

                renderer.line(MessageStyle::Status, &step_header(10, "Bundled skills"))?;
                let consent = match prior_consent {
                    Some(allowed) => {
                        renderer.line(
                            MessageStyle::Info,
                            &format!("Bundled skills: already {}.", if allowed { "allowed" } else { "declined" }),
                        )?;
                        renderer.line(MessageStyle::Info, "")?;
                        allowed
                    }
                    None => {
                        let allowed = prompt_bundled_skills_consent(&mut renderer)?;
                        renderer.line(MessageStyle::Info, "")?;
                        allowed
                    }
                };

                (
                    provider,
                    model,
                    lightweight_model,
                    reasoning,
                    persistent_memory,
                    trust,
                    permission_mode,
                    agent_name,
                    Some(consent),
                )
            }
            SetupMode::NonInteractive { full_auto, skip_confirmations } => {
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
                // Honor the flags that selected non-interactive setup so the
                // persisted default matches this run's behavior.
                let permission_mode = if full_auto || skip_confirmations {
                    DefaultPermissionMode::AutoApprove
                } else {
                    DefaultPermissionMode::Ask
                };
                let agent_name = config.default_primary_agent.clone();

                renderer.line(MessageStyle::Info, &format!("Provider: {}", provider.label()))?;
                renderer.line(MessageStyle::Info, &format!("Model: {model}"))?;
                renderer.line(MessageStyle::Info, "Lightweight model: Automatic (same-provider lightweight route)")?;
                renderer.line(MessageStyle::Info, &format!("Reasoning effort: {}", reasoning.as_str()))?;
                renderer.line(
                    MessageStyle::Info,
                    &format!("Persistent memory: {}", persistent_memory_label(persistent_memory)),
                )?;
                renderer.line(MessageStyle::Info, &api_key_hint(provider, storage_mode))?;
                renderer.line(MessageStyle::Info, &format!("Workspace trust: {}", trust_label(trust)))?;
                renderer.line(
                    MessageStyle::Info,
                    &format!("Permission mode: {}", permission_mode_label(permission_mode)),
                )?;
                renderer.line(MessageStyle::Info, &format!("Agent mode: {agent_name}"))?;
                match prior_consent {
                    Some(allowed) => renderer.line(
                        MessageStyle::Info,
                        &format!("Bundled skills: already {}.", if allowed { "allowed" } else { "declined" }),
                    )?,
                    None => renderer.line(
                        MessageStyle::Info,
                        "Bundled skills: not asked (non-interactive) — staying uninstalled until you allow them.",
                    )?,
                }
                renderer.line(MessageStyle::Info, "")?;

                (
                    provider,
                    model,
                    lightweight_model,
                    reasoning,
                    persistent_memory,
                    trust,
                    permission_mode,
                    agent_name,
                    prior_consent,
                )
            }
        };

    let selected_config_path = vtcode_config::loader::explicit_config_path();
    let is_explicit_config = selected_config_path.is_some();
    let config_path = selected_config_path.unwrap_or_else(|| workspace.join("vtcode.toml"));
    renderer.line(MessageStyle::Status, &format!("Saving your configuration to {} ...", config_path.display()))?;

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
        persistent_memory,
        permission_mode,
        &agent_name,
    );

    let save_result = if is_explicit_config {
        ConfigManager::save_config_to_path(&config_path, config)
    } else {
        ConfigManager::save_repository_config_to_path(&config_path, config)
    };
    save_result.with_context(|| format!("Failed to write initial configuration to {}", config_path.display()))?;

    update_model_preference(&provider_key, &model).await.ok();

    // Provider/model/permission choices are home-scoped: record them in the
    // user layer so later workspaces inherit them (and later fresh workspaces
    // skip this wizard). An explicit `vtcode setup` rerun intentionally
    // refreshes them.
    let permission_value = toml::Value::try_from(permission_mode).context("Failed to serialize permission mode")?;
    for (path, value) in [
        ("agent.provider", toml::Value::String(provider_key.clone())),
        ("agent.default_model", toml::Value::String(model.clone())),
        ("agent.api_key_env", toml::Value::String(config.agent.api_key_env.clone())),
        ("permissions.default_mode", permission_value),
    ] {
        ConfigService::write(ConfigWriteRequest {
            workspace: workspace.to_path_buf(),
            target: ConfigWriteTarget::User,
            path: path.to_string(),
            value,
            strategy: ConfigWriteStrategy::Replace,
            expected_layer_version: None,
        })
        .with_context(|| format!("Failed to persist {path} to the user configuration"))?;
    }

    // Consent is home-scoped, so a fresh answer goes to the user layer (not
    // the workspace file). Pre-existing answers are never rewritten here.
    if prior_consent.is_none()
        && let Some(allowed) = consent
    {
        ConfigService::write(ConfigWriteRequest {
            workspace: workspace.to_path_buf(),
            target: ConfigWriteTarget::User,
            path: BUNDLED_SKILLS_SETTING_PATH.to_string(),
            value: toml::Value::Boolean(allowed),
            strategy: ConfigWriteStrategy::Replace,
            expected_layer_version: None,
        })
        .with_context(|| "Failed to persist bundled-skills consent to the user configuration")?;
    }

    update_workspace_trust(workspace, trust)
        .await
        .with_context(|| format!("Failed to persist workspace trust level for {}", workspace.display()))?;

    renderer.line(MessageStyle::Info, "")?;
    let summary = SetupSummary {
        provider,
        model: &model,
        lightweight_model: &lightweight_model,
        reasoning,
        persistent_memory_enabled: persistent_memory,
        trust,
        permission_mode,
        agent_name: &agent_name,
        storage_mode,
        bundled_skills_consent: consent,
    };
    render_setup_summary(&mut renderer, &summary)?;
    renderer.line(
        MessageStyle::Status,
        "Tip: run `vtcode setup` anytime to rerun this setup and refresh other workspace defaults.",
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
    permission_mode: DefaultPermissionMode,
    agent_name: &str,
) {
    config.agent.provider = provider_key.to_owned();
    config.agent.default_model = model.to_owned();
    config.agent.small_model.model = lightweight_model.to_owned();
    config.agent.reasoning_effort = reasoning;
    config.default_primary_agent = agent_name.to_owned();
    config.permissions.default_mode = permission_mode;
    config.features.memories = persistent_memory_enabled;
    config.agent.persistent_memory.enabled = persistent_memory_enabled;
    config.agent.theme = defaults::DEFAULT_THEME.to_owned();
    config.agent.max_conversation_turns = tool_limits::DEFAULT_MAX_CONVERSATION_TURNS;
    config.agent.temperature = llm_generation::DEFAULT_TEMPERATURE;
}

/// Inputs for the setup completion summary, bundled to keep the render
/// helpers readable as wizard steps grow.
struct SetupSummary<'a> {
    provider: Provider,
    model: &'a str,
    lightweight_model: &'a str,
    reasoning: ReasoningEffortLevel,
    persistent_memory_enabled: bool,
    trust: WorkspaceTrustLevel,
    permission_mode: DefaultPermissionMode,
    agent_name: &'a str,
    storage_mode: AuthCredentialsStoreMode,
    bundled_skills_consent: Option<bool>,
}

fn render_setup_summary(renderer: &mut AnsiRenderer, summary: &SetupSummary) -> Result<()> {
    for (style, line) in setup_summary_lines(summary) {
        renderer.line(style, &line)?;
    }
    renderer.line(MessageStyle::Info, "")?;
    Ok(())
}

fn setup_summary_lines(summary: &SetupSummary) -> Vec<(MessageStyle, String)> {
    let mut lines = Vec::with_capacity(16);
    lines.push((
        MessageStyle::Status,
        format!(
            "Setup complete. Provider: {} • Model: {} • Lightweight: {} • Reasoning: {}",
            summary.provider.label(),
            summary.model,
            lightweight_model_label(summary.lightweight_model, summary.model),
            summary.reasoning.as_str()
        ),
    ));
    lines.push((
        MessageStyle::Info,
        format!(
            "Primary agent: {} • Persistent memory: {} • Trust: {} • Permissions: {}",
            summary.agent_name,
            persistent_memory_label(summary.persistent_memory_enabled),
            trust_label(summary.trust),
            permission_mode_label(summary.permission_mode),
        ),
    ));
    lines.push((MessageStyle::Status, format!("Auth: {}", api_key_hint(summary.provider, summary.storage_mode))));
    lines.push((MessageStyle::Info, format!("Bundled skills: {}", consent_label(summary.bundled_skills_consent))));
    lines.push((MessageStyle::Info, vtcode_config::consent::CONSENT_MCP_DISCLOSURE.to_string()));
    lines.push((MessageStyle::Status, "What's available now:".to_string()));
    lines.extend(
        capability_highlight_lines(summary.persistent_memory_enabled)
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
        "- Subagents let you delegate bounded work; use `/agent` or the Local Agents drawer.".to_string(),
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

fn api_key_hint(provider: Provider, storage_mode: AuthCredentialsStoreMode) -> String {
    if provider.is_local() {
        return "No API key required for local provider.".to_string();
    }
    if provider.uses_managed_auth() {
        return format!("Run `vtcode login {}` to authenticate.", provider);
    }
    match provider_credential_detail_with_mode(provider, storage_mode) {
        Some(detail) => match detail.source {
            CredentialSource::Env => {
                let var = detail.env_var.unwrap_or_else(|| provider.default_api_key_env());
                format!("Using {var} from your environment.")
            }
            CredentialSource::Workspace => {
                let var = detail.env_var.unwrap_or_else(|| provider.default_api_key_env());
                format!("Using {var} from the workspace .env.")
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
    // Only "real" credentials (env/workspace/keyring/OAuth) are worth advertising — local
    // and managed-auth providers are always nominally ready but not what most
    // users are choosing between on first run.
    let ready: Vec<&DiscoveredProvider> = discovered
        .iter()
        .filter(|d| {
            matches!(
                d.source,
                CredentialSource::Env
                    | CredentialSource::Workspace
                    | CredentialSource::SecureStorage
                    | CredentialSource::OAuth
            )
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

fn permission_mode_label(mode: DefaultPermissionMode) -> &'static str {
    match mode {
        DefaultPermissionMode::Ask => "Ask",
        DefaultPermissionMode::AutoApprove => "Auto-approve",
    }
}

/// Explicit re-run entry for `vtcode setup`: same wizard, no freshness gate.
pub(crate) async fn run_setup_wizard(workspace: &Path, config: &mut VTCodeConfig) -> Result<()> {
    run_first_run_setup(workspace, config, SetupMode::Interactive).await
}

fn consent_label(consent: Option<bool>) -> &'static str {
    match consent {
        Some(true) => "allowed",
        Some(false) => "declined (VT Code works without them; re-enable in /config)",
        None => "not asked — staying uninstalled until allowed",
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

        apply_selection(
            &mut config,
            "openai",
            "gpt-5.6-sol",
            "",
            ReasoningEffortLevel::None,
            false,
            DefaultPermissionMode::Ask,
            "build",
        );

        assert!(config.permissions.allow.is_empty());
        assert_eq!(config.default_primary_agent, "build");
    }

    #[test]
    fn first_run_selection_does_not_write_old_mode_fields() {
        let mut config = base_config();

        apply_selection(
            &mut config,
            "openai",
            "gpt-5.6-sol",
            "",
            ReasoningEffortLevel::None,
            false,
            DefaultPermissionMode::Ask,
            "build",
        );

        assert!(config.permissions.allow.is_empty());
        assert!(config.permissions.deny.is_empty());
        assert_eq!(config.default_primary_agent, "build");
        assert_eq!(config.permissions.default_mode, DefaultPermissionMode::Ask);
    }

    #[test]
    fn first_run_selection_persists_permission_and_agent_choice() {
        let mut config = base_config();

        apply_selection(
            &mut config,
            "openai",
            "gpt-5.6-sol",
            "",
            ReasoningEffortLevel::None,
            false,
            DefaultPermissionMode::AutoApprove,
            "plan",
        );

        assert_eq!(config.permissions.default_mode, DefaultPermissionMode::AutoApprove);
        assert_eq!(config.default_primary_agent, "plan");
    }

    #[test]
    fn persistent_memory_selection_persists_opt_in() {
        let mut config = base_config();

        apply_selection(
            &mut config,
            "openai",
            "gpt-5.6-sol",
            "",
            ReasoningEffortLevel::None,
            true,
            DefaultPermissionMode::Ask,
            "build",
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
            "gpt-5.6-sol",
            "",
            ReasoningEffortLevel::None,
            false,
            DefaultPermissionMode::Ask,
            "build",
        );

        assert!(!config.features.memories);
        assert!(!config.agent.persistent_memory.enabled);
    }

    fn test_summary(agent_name: &str, permission_mode: DefaultPermissionMode, consent: Option<bool>) -> String {
        let summary = SetupSummary {
            provider: Provider::OpenAI,
            model: "gpt-5.6-sol",
            lightweight_model: "",
            reasoning: ReasoningEffortLevel::None,
            persistent_memory_enabled: false,
            trust: WorkspaceTrustLevel::ToolsPolicy,
            permission_mode,
            agent_name,
            storage_mode: AuthCredentialsStoreMode::default(),
            bundled_skills_consent: consent,
        };
        setup_summary_lines(&summary)
            .into_iter()
            .map(|(_style, line)| line)
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn setup_summary_mentions_new_capabilities() {
        let rendered = test_summary("build", DefaultPermissionMode::Ask, Some(true));

        assert!(rendered.contains("`--full-auto` is separate"));
        assert!(rendered.contains("Persistent repository memory"));
        assert!(rendered.contains("Primary agent: build"));
        assert!(rendered.contains("Permissions: Ask"));
        assert!(rendered.contains("Subagents"));
        assert!(rendered.contains("Skills"));
        assert!(rendered.contains("lightweight model route"));
        assert!(rendered.contains("Granular permissions"));
        assert!(!rendered.contains("Startup mode"));
        assert!(!rendered.contains("permission mode"));
    }

    #[test]
    fn setup_summary_reports_agent_and_permission_choices() {
        let rendered = test_summary("plan", DefaultPermissionMode::AutoApprove, Some(true));

        assert!(rendered.contains("Primary agent: plan"), "summary: {rendered}");
        assert!(rendered.contains("Permissions: Auto-approve"), "summary: {rendered}");
    }

    fn summary_with_consent(consent: Option<bool>) -> String {
        test_summary("build", DefaultPermissionMode::Ask, consent)
    }

    #[test]
    fn setup_summary_reports_consent_state_and_mcp_disclosure() {
        let allowed = summary_with_consent(Some(true));
        assert!(allowed.contains("Bundled skills: allowed"), "summary: {allowed}");
        assert!(allowed.contains("never installed automatically"), "summary: {allowed}");

        let declined = summary_with_consent(Some(false));
        assert!(declined.contains("Bundled skills: declined"), "summary: {declined}");
        assert!(declined.contains("re-enable in /config"), "summary: {declined}");

        let unasked = summary_with_consent(None);
        assert!(unasked.contains("Bundled skills: not asked"), "summary: {unasked}");
        assert!(unasked.contains("staying uninstalled"), "summary: {unasked}");
    }

    #[test]
    fn step_headers_number_all_interactive_prompts() {
        assert_eq!(step_header(1, "Provider"), "Step 1 of 10 — Provider");
        assert_eq!(step_header(10, "Bundled skills"), "Step 10 of 10 — Bundled skills");
        assert_eq!(TOTAL_SETUP_STEPS, 10);
    }
}
