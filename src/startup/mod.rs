use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow, bail};
use vtcode_config::agent_teams::TeammateMode;
use vtcode_core::agent_teams::{TeamContext, TeamRole};
use vtcode_core::cli::TeamRoleArg;
use vtcode_core::dotfile_protection::init_global_guardian;
use vtcode_core::utils::validation::validate_path_exists;

mod first_run;
mod first_run_prompts;
mod helpers;

use first_run::maybe_run_first_run_setup;
use helpers::{
    api_key_env_var, apply_permission_mode_override, determine_theme, parse_cli_config_entries,
    provider_label, resolve_config_path, resolve_workspace_path, validate_additional_directories,
    validate_full_auto_configuration, validate_session_id_suffix, validate_startup_configuration,
};
use vtcode_core::cli::args::Cli;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::{ConfigBuilder, VTCodeConfig};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, ModelSelectionSource};
use vtcode_core::llm::factory::infer_provider;
use vtcode_core::{initialize_dot_folder, update_theme_preference};

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

        let provider = resolve_provider(
            args.provider.clone().or_else(provider_env_override),
            config.agent.provider.as_str(),
            &model,
            model_source,
        );

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
            model_behavior: Some(config.model.clone()),
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

fn resolve_provider(
    cli_provider: Option<String>,
    configured_provider: &str,
    model: &str,
    model_source: ModelSelectionSource,
) -> String {
    if let Some(provider) = cli_provider {
        return provider;
    }

    if matches!(model_source, ModelSelectionSource::CliOverride)
        && let Some(provider) = infer_provider(None, model)
    {
        return provider.to_string();
    }

    let configured_provider = configured_provider.trim();
    if !configured_provider.is_empty() {
        return configured_provider.to_owned();
    }

    infer_provider(None, model)
        .map(|provider| provider.to_string())
        .unwrap_or_else(|| defaults::DEFAULT_PROVIDER.to_owned())
}

fn provider_env_override() -> Option<String> {
    std::env::var("VTCODE_PROVIDER")
        .ok()
        .or_else(|| std::env::var("provider").ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
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

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[test]
    fn retention_warning_for_non_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.prompt_cache.providers.openai.prompt_cache_retention = Some("24h".to_owned());
        let model = "gpt-oss-20b"; // not in responses API list
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

    #[test]
    fn provider_resolution_prefers_configured_provider_for_config_model() {
        let provider = resolve_provider(
            None,
            "zai",
            vtcode_core::config::constants::models::ollama::MINIMAX_M25_CLOUD,
            ModelSelectionSource::WorkspaceConfig,
        );
        assert_eq!(provider, "zai");
    }

    #[test]
    fn provider_resolution_infers_from_cli_model_without_cli_provider() {
        let provider = resolve_provider(
            None,
            "zai",
            vtcode_core::config::constants::models::ollama::MINIMAX_M25_CLOUD,
            ModelSelectionSource::CliOverride,
        );
        assert_eq!(provider, "ollama");
    }

    #[test]
    fn provider_resolution_uses_cli_provider_when_present() {
        let provider = resolve_provider(
            Some("minimax".to_owned()),
            "zai",
            vtcode_core::config::constants::models::ollama::MINIMAX_M25_CLOUD,
            ModelSelectionSource::CliOverride,
        );
        assert_eq!(provider, "minimax");
    }
}
