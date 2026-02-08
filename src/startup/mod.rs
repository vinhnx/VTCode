use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use vtcode_config::agent_teams::TeammateMode;
use vtcode_core::agent_teams::{TeamContext, TeamRole};
use vtcode_core::cli::TeamRoleArg;
use vtcode_core::dotfile_protection::init_global_guardian;
use vtcode_core::utils::validation::{
    validate_is_directory, validate_non_empty, validate_path_exists,
};

mod first_run;

use crate::tools::RipgrepStatus;

/// Validate custom session ID suffix
fn validate_session_id_suffix(suffix: &str) -> Result<()> {
    validate_non_empty(suffix, "Custom session ID suffix")?;
    if suffix.len() > 64 {
        bail!("Custom session ID suffix too long (maximum 64 characters)");
    }
    if !suffix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!(
            "Custom session ID suffix must contain only alphanumeric characters, dashes, or underscores"
        );
    }
    Ok(())
}

/// Validate additional directories and resolve to absolute paths
fn validate_additional_directories(dirs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut validated_dirs = Vec::new();

    for dir in dirs {
        validate_is_directory(dir, "Additional directory")?;

        // Resolve to absolute path
        let absolute_dir = dir
            .canonicalize()
            .with_context(|| format!("Failed to resolve path '{}'", dir.display()))?;

        validated_dirs.push(absolute_dir);
    }

    Ok(validated_dirs)
}

/// Apply permission mode override from CLI
fn apply_permission_mode_override(config: &mut VTCodeConfig, mode: &str) -> Result<()> {
    use vtcode_config::constants::tools;

    match mode.to_lowercase().as_str() {
        "ask" => {
            config.security.human_in_the_loop = true;
            config.security.require_write_tool_for_claims = true;
            config.automation.full_auto.enabled = false;
        }
        "suggest" => {
            config.security.human_in_the_loop = true;
            config.security.require_write_tool_for_claims = false;
            config.automation.full_auto.enabled = false;
        }
        "auto-approved" => {
            config.security.human_in_the_loop = false;
            config.security.require_write_tool_for_claims = false;
            config.automation.full_auto.enabled = true;
            config.automation.full_auto.allowed_tools = vec![
                tools::READ_FILE.to_string(),
                tools::LIST_FILES.to_string(),
                tools::GREP_FILE.to_string(),
            ];
        }
        "full-auto" => {
            config.security.human_in_the_loop = false;
            config.security.require_write_tool_for_claims = false;
            config.automation.full_auto.enabled = true;
            config.automation.full_auto.allowed_tools = vec![];
        }
        "plan" => {
            return Ok(());
        }
        _ => {
            bail!(
                "Invalid permission mode '{}'. Valid options: ask, suggest, auto-approved, full-auto, plan",
                mode
            );
        }
    }

    Ok(())
}

use first_run::maybe_run_first_run_setup;
use vtcode_core::cli::args::Cli;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::{ConfigBuilder, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use vtcode_core::config::validator::ConfigValidator;
use vtcode_core::llm::factory::infer_provider;
use vtcode_core::ui::theme::{self as ui_theme, DEFAULT_THEME_ID};
use vtcode_core::{initialize_dot_folder, load_user_config, update_theme_preference};

fn provider_label(provider: &str) -> String {
    match provider.to_lowercase().as_str() {
        "openai" => "OpenAI".to_string(),
        "anthropic" => "Anthropic".to_string(),
        "gemini" => "Gemini".to_string(),
        "deepseek" => "DeepSeek".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        "xai" => "xAI".to_string(),
        "zai" => "Z.AI".to_string(),
        "ollama" => "Ollama".to_string(),
        "lmstudio" => "LM Studio".to_string(),
        _ => provider.to_string(),
    }
}

fn api_key_env_var(provider: &str) -> String {
    match provider.to_lowercase().as_str() {
        "openai" => "OPENAI_API_KEY".to_string(),
        "anthropic" => "ANTHROPIC_API_KEY".to_string(),
        "gemini" => "GEMINI_API_KEY".to_string(),
        "deepseek" => "DEEPSEEK_API_KEY".to_string(),
        "openrouter" => "OPENROUTER_API_KEY".to_string(),
        "xai" => "XAI_API_KEY".to_string(),
        "zai" => "ZAI_API_KEY".to_string(),
        "ollama" => "OLLAMA_API_KEY".to_string(),
        "lmstudio" => "LMSTUDIO_API_KEY".to_string(),
        _ => format!("{}_API_KEY", provider.to_uppercase()),
    }
}

/// Aggregated data required for CLI command execution after startup.
#[derive(Debug, Clone)]
pub struct StartupContext {
    pub workspace: PathBuf,
    pub additional_dirs: Vec<PathBuf>,
    pub config: VTCodeConfig,
    pub agent_config: CoreAgentConfig,
    pub skip_confirmations: bool,
    pub full_auto_requested: bool,
    pub automation_prompt: Option<String>,
    pub session_resume: Option<SessionResumeMode>,
    pub custom_session_id: Option<String>,
    pub plan_mode_requested: bool,
    pub team_context: Option<vtcode_core::agent_teams::TeamContext>,
}

#[derive(Debug, Clone)]
pub enum SessionResumeMode {
    Interactive,
    Latest,
    Specific(String),
    Fork(String), // Fork from specific session ID
}

impl StartupContext {
    pub async fn from_cli_args(args: &Cli) -> Result<Self> {
        let workspace_override = args
            .workspace_path
            .clone()
            .or_else(|| args.workspace.clone());

        let workspace = resolve_workspace_path(workspace_override)
            .context("Failed to resolve workspace directory")?;

        if args.workspace_path.is_some() {
            validate_path_exists(&workspace, "Workspace")?;
        }

        // Validate and resolve additional directories
        let additional_dirs = validate_additional_directories(&args.additional_dirs)?;

        let (cli_config_path_override, inline_config_overrides) =
            parse_cli_config_entries(&args.config);
        let env_config_path_override = std::env::var("VTCODE_CONFIG_PATH").ok().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(PathBuf::from(trimmed))
            }
        });
        let config_path_override = cli_config_path_override.or(env_config_path_override);

        let mut builder = ConfigBuilder::new().workspace(workspace.clone());
        if let Some(path_override) = config_path_override {
            let resolved_path = resolve_config_path(&workspace, &path_override);
            unsafe {
                std::env::set_var("VTCODE_CONFIG_PATH", &resolved_path);
            }
            builder = builder.config_file(resolved_path);
        }

        if !inline_config_overrides.is_empty() {
            builder = builder.cli_overrides(&inline_config_overrides);
        }

        // Apply explicit CLI overrides for model and provider
        if let Some(ref model) = args.model {
            builder =
                builder.cli_override("agent.model".to_owned(), toml::Value::String(model.clone()));
        }
        if let Some(ref provider) = args.provider {
            builder = builder.cli_override(
                "agent.provider".to_owned(),
                toml::Value::String(provider.clone()),
            );
        }

        let manager = builder.build().context("Failed to load configuration")?;
        let mut config = manager.config().clone();

        let (full_auto_requested, automation_prompt) = match args.full_auto.clone() {
            Some(value) => {
                if value.trim().is_empty() {
                    (true, None)
                } else {
                    (true, Some(value))
                }
            }
            None => (false, None),
        };

        let _first_run = maybe_run_first_run_setup(args, &workspace, &mut config).await?;

        if automation_prompt.is_some() && args.command.is_some() {
            bail!(
                "--auto/--full-auto with a prompt cannot be combined with other commands. Provide only the prompt."
            );
        }

        if full_auto_requested {
            validate_full_auto_configuration(&config, &workspace)?;
        }

        // Determine plan mode: CLI flag takes precedence, then config default_editing_mode
        let plan_mode_from_cli = args
            .permission_mode
            .as_ref()
            .is_some_and(|m| m.eq_ignore_ascii_case("plan"));

        // Check config for default_editing_mode = "plan" if not explicitly set via CLI
        let plan_mode_from_config =
            !plan_mode_from_cli && config.agent.default_editing_mode.is_read_only();

        let plan_mode_requested = plan_mode_from_cli || plan_mode_from_config;

        if let Some(ref permission_mode) = args.permission_mode {
            apply_permission_mode_override(&mut config, permission_mode)?;
        }

        if let Some(mode) = args.teammate_mode.clone() {
            config.agent_teams.teammate_mode = mode.into();
        }

        let team_context = resolve_team_context(&mut config, args)?;

        // Validate configuration against models database
        validate_startup_configuration(&config, &workspace, args.quiet)?;

        // Validate custom session ID if provided
        let custom_session_id = args.session_id.clone();
        if let Some(ref suffix) = custom_session_id {
            validate_session_id_suffix(suffix)?;
        }

        // Parse session resume mode and handle fork logic
        let session_resume = if let Some(fork_id) = args.fork_session.as_ref() {
            // --fork-session takes precedence
            Some(SessionResumeMode::Fork(fork_id.clone()))
        } else if let Some(value) = args.resume_session.as_ref() {
            if value == "__interactive__" {
                // --resume with interactive mode
                Some(SessionResumeMode::Interactive)
            } else {
                // --resume with specific ID
                if custom_session_id.is_some() {
                    // --resume + --session-id becomes fork
                    Some(SessionResumeMode::Fork(value.clone()))
                } else {
                    Some(SessionResumeMode::Specific(value.clone()))
                }
            }
        } else if args.continue_latest {
            // --continue (resume latest)
            if custom_session_id.is_some() {
                // --continue + --session-id becomes fork from latest
                Some(SessionResumeMode::Fork("__latest__".to_string()))
            } else {
                Some(SessionResumeMode::Latest)
            }
        } else {
            None
        };

        if session_resume.is_some() && args.command.is_some() {
            bail!(
                "--resume/--continue/--fork-session cannot be combined with other commands. Run the operation without a subcommand."
            );
        }

        // Determine model: --agent flag takes precedence, then --model, then config
        let (model, model_source) = if let Some(agent) = args.agent.clone() {
            (agent, ModelSelectionSource::CliOverride)
        } else if let Some(value) = args.model.clone() {
            (value, ModelSelectionSource::CliOverride)
        } else {
            (
                config.agent.default_model.clone(),
                ModelSelectionSource::WorkspaceConfig,
            )
        };

        let provider = match args.provider.clone() {
            Some(value) => value,
            None => infer_provider(None, &model)
                .map(|provider| provider.to_string())
                .unwrap_or_else(|| config.agent.provider.clone()),
        };

        initialize_dot_folder().await.ok();

        // Initialize dotfile protection with configuration
        if let Err(e) = init_global_guardian(config.dotfile_protection.clone()).await {
            tracing::warn!("Failed to initialize dotfile protection: {}", e);
        }

        let theme_selection = determine_theme(args, &config).await?;

        update_theme_preference(&theme_selection).await.ok();

        // Validate API key AFTER first-run setup so new users can complete setup first
        let api_key = get_api_key(&provider, &ApiKeySources::default())
            .with_context(|| {
                let first_run_occurred = _first_run;
                let provider_name = provider_label(&provider);
                let env_var = api_key_env_var(&provider);
                if first_run_occurred {
                    format!(
                        "API key not found for {}. To fix:\n  1. Set {} environment variable, or\n  2. Add to .env file, or\n  3. Configure in vtcode.toml\n\nRun `/init` anytime to reconfigure.",
                        provider_name,
                        env_var
                    )
                } else {
                    format!(
                        "API key not found for provider '{}'. Set {} environment variable (or add to .env file) or configure in vtcode.toml.",
                        provider,
                        api_key_env_var(&provider)
                    )
                }
            })?;

        let provider_enum = Provider::from_str(&provider).unwrap_or(Provider::Gemini);
        let cli_api_key_env = args.api_key_env.trim();
        let api_key_env_override = if cli_api_key_env.is_empty()
            || cli_api_key_env.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV)
        {
            None
        } else {
            Some(cli_api_key_env.to_owned())
        };

        let configured_api_key_env = config.agent.api_key_env.trim();
        // Compute provider default env once and reuse
        let provider_default_env = provider_enum.default_api_key_env();
        let resolved_api_key_env = if configured_api_key_env.is_empty()
            || configured_api_key_env.eq_ignore_ascii_case(defaults::DEFAULT_API_KEY_ENV)
        {
            provider_default_env.to_owned()
        } else {
            configured_api_key_env.to_owned()
        };

        let api_key_env = api_key_env_override.unwrap_or(resolved_api_key_env);

        let checkpointing_storage_dir =
            config.agent.checkpointing.storage_dir.as_ref().map(|dir| {
                let candidate = PathBuf::from(dir);
                if candidate.is_absolute() {
                    candidate
                } else {
                    workspace.join(candidate)
                }
            });

        let agent_config = CoreAgentConfig {
            model,
            api_key,
            provider,
            api_key_env,
            workspace: workspace.clone(),
            verbose: args.verbose,
            quiet: args.quiet,
            theme: theme_selection,
            reasoning_effort: config.agent.reasoning_effort,
            ui_surface: config.agent.ui_surface,
            prompt_cache: config.prompt_cache.clone(),
            model_source,
            custom_api_keys: config.agent.custom_api_keys.clone(),
            checkpointing_enabled: config.agent.checkpointing.enabled,
            checkpointing_storage_dir,
            checkpointing_max_snapshots: config.agent.checkpointing.max_snapshots,
            checkpointing_max_age_days: config.agent.checkpointing.max_age_days,
            max_conversation_turns: config.agent.max_conversation_turns,
        };

        let skip_confirmations = args.skip_confirmations || full_auto_requested;

        // CLI validation: warn if prompt_cache_retention is set but model does not use Responses API
        if agent_config.provider.eq_ignore_ascii_case("openai")
            && let Some(ref retention) = agent_config
                .prompt_cache
                .providers
                .openai
                .prompt_cache_retention
            && !retention.trim().is_empty()
        {
            // Use constants list to identify which models use Responses API
            if let Some(msg) = check_prompt_cache_retention_compat(
                &config,
                &agent_config.model,
                &agent_config.provider,
            ) {
                tracing::warn!("{}", msg);
            }
        }

        vtcode_core::telemetry::perf::initialize_perf_telemetry(&config.telemetry);
        vtcode_core::tools::cache::configure_file_cache(&config.optimization.file_read_cache);
        vtcode_core::tools::command_cache::configure_command_cache(
            &config.optimization.command_cache,
        );
        vtcode_core::utils::gatekeeper::initialize_gatekeeper(
            &config.security.gatekeeper,
            Some(&workspace),
        );

        Ok(StartupContext {
            workspace,
            additional_dirs,
            config,
            agent_config,
            skip_confirmations: args.dangerously_skip_permissions || skip_confirmations,
            full_auto_requested,
            automation_prompt,
            session_resume,
            custom_session_id,
            plan_mode_requested,
            team_context,
        })
    }
}

fn resolve_team_context(config: &mut VTCodeConfig, args: &Cli) -> Result<Option<TeamContext>> {
    let has_team_args = args.team.is_some() || args.teammate.is_some() || args.team_role.is_some();
    if !has_team_args {
        return Ok(None);
    }

    let role = match args.team_role {
        Some(TeamRoleArg::Lead) => TeamRole::Lead,
        Some(TeamRoleArg::Teammate) => TeamRole::Teammate,
        None => {
            if args.teammate.is_some() {
                TeamRole::Teammate
            } else {
                TeamRole::Lead
            }
        }
    };

    let team_name = args
        .team
        .clone()
        .ok_or_else(|| anyhow!("--team is required when joining a team"))?;

    let teammate_name = match role {
        TeamRole::Lead => None,
        TeamRole::Teammate => Some(
            args.teammate
                .clone()
                .ok_or_else(|| anyhow!("--teammate is required for teammate role"))?,
        ),
    };

    let mode = args
        .teammate_mode
        .clone()
        .map(TeammateMode::from)
        .unwrap_or(config.agent_teams.teammate_mode);

    config.agent_teams.enabled = true;

    Ok(Some(TeamContext {
        team_name,
        role,
        teammate_name,
        mode,
    }))
}

/// Validate whether prompt_cache_retention is applicable for the given model and provider.
/// Returns an optional warning message if compatibility is lacking.
pub fn check_prompt_cache_retention_compat(
    config: &VTCodeConfig,
    model: &str,
    provider: &str,
) -> Option<String> {
    // Only relevant for OpenAI provider
    if !provider.eq_ignore_ascii_case("openai") {
        return None;
    }

    if let Some(ref retention) = config.prompt_cache.providers.openai.prompt_cache_retention {
        if retention.trim().is_empty() {
            return None;
        }
        if !vtcode_core::config::constants::models::openai::RESPONSES_API_MODELS.contains(&model) {
            return Some(format!(
                "`prompt_cache_retention` is set but the selected model '{}' does not use the OpenAI Responses API. The setting will be ignored for this model. Run `vtcode models list --provider openai` to see supported Responses API models.",
                model
            ));
        }
    }

    None
}

fn parse_cli_config_entries(entries: &[String]) -> (Option<PathBuf>, Vec<(String, String)>) {
    let mut config_path: Option<PathBuf> = None;
    let mut overrides = Vec::new();

    for entry in entries {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            overrides.push((key.to_owned(), value.trim().to_owned()));
        } else if config_path.is_none() {
            config_path = Some(PathBuf::from(trimmed));
        }
    }

    (config_path, overrides)
}

fn resolve_config_path(workspace: &Path, candidate: &Path) -> PathBuf {
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }

    let workspace_candidate = workspace.join(candidate);
    if workspace_candidate.exists() {
        return workspace_candidate;
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| workspace.to_path_buf());
    let cwd_candidate = cwd.join(candidate);
    if cwd_candidate.exists() {
        cwd_candidate
    } else {
        workspace_candidate
    }
}

async fn determine_theme(args: &Cli, config: &VTCodeConfig) -> Result<String> {
    // Apply color accessibility settings from config
    let color_config = ui_theme::ColorAccessibilityConfig {
        minimum_contrast: config.ui.minimum_contrast,
        bold_is_bright: config.ui.bold_is_bright,
        safe_colors_only: config.ui.safe_colors_only,
    };
    ui_theme::set_color_accessibility_config(color_config);

    let user_theme_pref = load_user_config().await.ok().and_then(|dot| {
        let trimmed = dot.preferences.theme.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    });

    let config_theme = config.agent.theme.trim();

    // Handle auto theme selection based on color_scheme_mode
    let auto_theme = match config.ui.color_scheme_mode {
        vtcode_config::root::ColorSchemeMode::Auto => {
            Some(ui_theme::suggest_theme_for_terminal().to_owned())
        }
        vtcode_config::root::ColorSchemeMode::Light => Some("vitesse-light".to_owned()),
        vtcode_config::root::ColorSchemeMode::Dark => None, // Use default dark theme
    };

    let mut theme_selection = args
        .theme
        .clone()
        .or_else(|| (!config_theme.is_empty()).then(|| config_theme.to_string()))
        .or(user_theme_pref)
        .or(auto_theme)
        .unwrap_or_else(|| DEFAULT_THEME_ID.to_owned());

    if let Err(err) = ui_theme::set_active_theme(&theme_selection) {
        if args.theme.is_some() {
            return Err(err.context(format!("Failed to activate theme '{}'", theme_selection)));
        }

        theme_selection = DEFAULT_THEME_ID.to_owned();
        ui_theme::set_active_theme(&theme_selection)
            .with_context(|| format!("Failed to activate theme '{}'", theme_selection))?;
    }

    // Validate theme contrast if user wants feedback
    let validation = ui_theme::validate_theme_contrast(&theme_selection);
    for warning in &validation.warnings {
        tracing::debug!(theme = %theme_selection, warning = %warning, "Theme contrast warning");
    }

    Ok(theme_selection)
}

fn validate_full_auto_configuration(config: &VTCodeConfig, workspace: &Path) -> Result<()> {
    let automation_cfg = &config.automation.full_auto;
    if !automation_cfg.enabled {
        bail!(
            "Full-auto mode is disabled in configuration. Enable it under [automation.full_auto]."
        );
    }

    if automation_cfg.require_profile_ack {
        let profile_path = automation_cfg.profile_path.clone().ok_or_else(|| {
            anyhow!(
                "Full-auto mode requires 'profile_path' in [automation.full_auto] when require_profile_ack = true."
            )
        })?;
        let resolved_profile = if profile_path.is_absolute() {
            profile_path
        } else {
            workspace.join(profile_path)
        };

        if !resolved_profile.exists() {
            bail!(
                "Full-auto profile '{}' not found. Create the acknowledgement file before using --full-auto.",
                resolved_profile.display()
            );
        }
    }

    Ok(())
}

fn resolve_workspace_path(workspace_arg: Option<PathBuf>) -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Failed to determine current working directory")?;

    let mut resolved = match workspace_arg {
        Some(path) if path.is_absolute() => path,
        Some(path) => cwd.join(path),
        None => cwd,
    };

    if resolved.exists() {
        resolved = resolved.canonicalize().with_context(|| {
            format!(
                "Failed to canonicalize workspace path {}",
                resolved.display()
            )
        })?;
    }

    Ok(resolved)
}

fn validate_startup_configuration(
    config: &VTCodeConfig,
    workspace: &Path,
    quiet: bool,
) -> Result<()> {
    // Check and optionally install ripgrep on startup
    check_ripgrep_availability(quiet);

    // Find models.json in workspace or standard locations
    let mut models_json_paths = vec![workspace.join("docs/models.json")];

    if let Ok(cwd) = std::env::current_dir() {
        models_json_paths.push(cwd.join("docs/models.json"));
    }

    let models_json_path = models_json_paths
        .iter()
        .find(|p| p.exists())
        .map(|p| p.to_path_buf());

    if let Some(models_path) = models_json_path {
        match ConfigValidator::new(&models_path) {
            Ok(validator) => {
                match validator.validate(config) {
                    Ok(result) => {
                        // Display warnings (errors would have been caught earlier)
                        if !result.warnings.is_empty() && !quiet {}
                    }
                    Err(_e) => {
                        // Non-critical validation error - log but don't fail startup
                        if !quiet {}
                    }
                }
            }
            Err(e) => {
                // Non-critical validator creation error
                if !quiet {
                    eprintln!(
                        "vtcode: warning: could not load models database for validation: {}",
                        e
                    );
                }
            }
        }
    }

    Ok(())
}

/// Check ripgrep availability and attempt auto-installation if missing
fn check_ripgrep_availability(quiet: bool) {
    match RipgrepStatus::check() {
        RipgrepStatus::Available { version } => {
            // Ripgrep is available, log silently at trace level only
            tracing::debug!("Ripgrep available: {}", version);
        }
        RipgrepStatus::NotFound => {
            if quiet {
                return;
            }
            // Show warning and attempt auto-installation

            // Attempt auto-installation
            match RipgrepStatus::install() {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!("Ripgrep installation failed: {}", e);
                }
            }
        }
        RipgrepStatus::Error { reason } => {
            tracing::warn!("Ripgrep check error: {}", reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::TempDir;
    use std::env;
    use std::sync::{Mutex, OnceLock};

    fn workspace_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("workspace lock")
    }

    #[test]
    fn resolves_current_dir_when_none() -> Result<()> {
        let _guard = workspace_guard();
        let original_cwd = env::current_dir()?;
        let temp_dir = TempDir::new()?;
        env::set_current_dir(temp_dir.path())?;

        let resolved = resolve_workspace_path(None)?;
        assert_eq!(resolved, temp_dir.path().canonicalize()?);

        env::set_current_dir(original_cwd)?;
        Ok(())
    }

    #[test]
    fn resolves_relative_workspace_path() -> Result<()> {
        let _guard = workspace_guard();
        let original_cwd = env::current_dir()?;
        let temp_dir = TempDir::new()?;
        let workspace_dir = temp_dir.path().join("project");
        std::fs::create_dir(&workspace_dir)?;
        env::set_current_dir(temp_dir.path())?;

        let resolved = resolve_workspace_path(Some(PathBuf::from("project")))?;
        assert_eq!(resolved, workspace_dir.canonicalize()?);

        env::set_current_dir(original_cwd)?;
        Ok(())
    }

    #[test]
    fn parses_cli_config_entries_with_overrides() {
        let entries = vec![
            "agent.provider=openai".to_owned(),
            "custom-config/vtcode.toml".to_owned(),
        ];

        let (path, overrides) = parse_cli_config_entries(&entries);

        assert_eq!(path, Some(PathBuf::from("custom-config/vtcode.toml")));
        assert_eq!(
            overrides,
            vec![("agent.provider".to_owned(), "openai".to_owned())]
        );
    }

    #[test]
    fn applies_inline_overrides_to_config() -> Result<()> {
        let overrides = vec![("agent.provider".to_owned(), "\"openai\"".to_owned())];

        let manager = ConfigBuilder::new().cli_overrides(&overrides).build()?;
        let config = manager.config();

        assert_eq!(config.agent.provider, "openai");
        Ok(())
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[test]
    fn retention_warning_for_non_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.prompt_cache.providers.openai.prompt_cache_retention = Some("24h".to_owned());
        let model = "codex-mini-latest"; // not in responses API list
        let provider = "openai";
        let msg = check_prompt_cache_retention_compat(&cfg, model, provider);
        assert!(msg.is_some());
    }

    #[test]
    fn retention_ok_for_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.prompt_cache.providers.openai.prompt_cache_retention = Some("24h".to_owned());
        let model = vtcode_core::config::constants::models::openai::GPT_5; // responses model
        let provider = "openai";
        let msg = check_prompt_cache_retention_compat(&cfg, model, provider);
        assert!(msg.is_none());
    }
}
