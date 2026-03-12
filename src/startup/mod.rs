use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vtcode_core::dotfile_protection::init_global_guardian;
use vtcode_core::utils::validation::validate_path_exists;

mod config_loading;
mod dependency_advisories;
mod first_run;
mod first_run_prompts;
mod resume;
mod theme;
mod validation;
mod workspace_trust;

use config_loading::load_startup_config;
pub(crate) use dependency_advisories::{SearchToolsBundleNotice, take_search_tools_bundle_notice};
use resume::{resolve_session_resume, validate_resume_all_usage};
use theme::determine_theme;
use validation::{
    apply_permission_mode_override, validate_full_auto_configuration,
    validate_startup_configuration,
};
use vtcode_core::cli::args::Cli;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::config::validator::check_prompt_cache_retention_compat;
use vtcode_core::core::agent::config::{
    api_key_env_var, build_runtime_agent_config, provider_label, resolve_runtime_model_selection,
};
use vtcode_core::{initialize_dot_folder, update_theme_preference};
pub(crate) use workspace_trust::{
    ensure_full_auto_workspace_trust, require_full_auto_workspace_trust,
};

/// Aggregated data required for CLI command execution after startup.
#[derive(Debug, Clone)]
pub(crate) struct StartupContext {
    pub(crate) workspace: PathBuf,
    pub(crate) config: VTCodeConfig,
    pub(crate) agent_config: CoreAgentConfig,
    pub(crate) skip_confirmations: bool,
    pub(crate) full_auto_requested: bool,
    pub(crate) automation_prompt: Option<String>,
    pub(crate) session_resume: Option<SessionResumeMode>,
    pub(crate) resume_show_all: bool,
    pub(crate) custom_session_id: Option<String>,
    pub(crate) plan_mode_requested: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum SessionResumeMode {
    Interactive,
    Latest,
    Specific(String),
    Fork(String), // Fork from specific session ID
}

impl StartupContext {
    pub(crate) async fn from_cli_args(args: &Cli) -> Result<Self> {
        let loaded = load_startup_config(args).await?;
        if args.workspace_path.is_some() {
            validate_path_exists(&loaded.workspace, "Workspace")?;
        }
        if loaded.full_auto_requested {
            validate_full_auto_configuration(&loaded.config, &loaded.workspace)?;
        }

        // Determine plan mode: CLI flag takes precedence, then config default_editing_mode
        let plan_mode_from_cli = args
            .permission_mode
            .as_ref()
            .is_some_and(|m| m.eq_ignore_ascii_case("plan"));

        // Check config for default_editing_mode = "plan" if not explicitly set via CLI
        let plan_mode_from_config =
            !plan_mode_from_cli && loaded.config.agent.default_editing_mode.is_read_only();

        let plan_mode_requested = plan_mode_from_cli || plan_mode_from_config;

        let mut config = loaded.config;
        if let Some(ref permission_mode) = args.permission_mode {
            apply_permission_mode_override(&mut config, permission_mode)?;
        }

        // Validate configuration against models database
        validate_startup_configuration(&config, &loaded.workspace, args.quiet)?;

        let (custom_session_id, session_resume) = resolve_session_resume(args)?;
        validate_resume_all_usage(args, session_resume.as_ref())?;

        if session_resume.is_some() && args.command.is_some() {
            bail!(
                "--resume/--continue/--fork-session cannot be combined with other commands. Run the operation without a subcommand."
            );
        }

        let selection = resolve_runtime_model_selection(args, &config);

        initialize_dot_folder().await.ok();

        // Initialize dotfile protection with configuration
        if let Err(e) = init_global_guardian(config.dotfile_protection.clone()).await {
            tracing::warn!("Failed to initialize dotfile protection: {}", e);
        }

        let theme_selection = determine_theme(args, &config).await?;

        update_theme_preference(&theme_selection).await.ok();

        // Validate API key AFTER first-run setup so new users can complete setup first
        let api_key = get_api_key(&selection.provider, &ApiKeySources::default())
            .with_context(|| {
                let first_run_occurred = loaded.first_run_occurred;
                let provider_name = provider_label(&selection.provider);
                let env_var = api_key_env_var(&selection.provider);
                if first_run_occurred {
                    format!(
                        "API key not found for {}. To fix:\n  1. Set {} environment variable, or\n  2. Add to .env file, or\n  3. Configure in vtcode.toml\n\nRun `/init` anytime to reconfigure.",
                        provider_name,
                        env_var
                    )
                } else {
                    format!(
                        "API key not found for provider '{}'. Set {} environment variable (or add to .env file) or configure in vtcode.toml.",
                        selection.provider,
                        api_key_env_var(&selection.provider)
                    )
                }
            })?;

        let agent_config = build_runtime_agent_config(
            args,
            &config,
            loaded.workspace.clone(),
            selection,
            api_key,
            theme_selection,
        );

        let skip_confirmations = args.skip_confirmations || loaded.full_auto_requested;

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
            Some(&loaded.workspace),
        );

        Ok(StartupContext {
            workspace: loaded.workspace,
            config,
            agent_config,
            skip_confirmations: args.dangerously_skip_permissions || skip_confirmations,
            full_auto_requested: loaded.full_auto_requested,
            automation_prompt: loaded.automation_prompt,
            session_resume,
            resume_show_all: args.all,
            custom_session_id,
            plan_mode_requested,
        })
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    use assert_fs::TempDir;
    use clap::Parser;
    use vtcode_core::cli::args::Cli;

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
    fn resolve_session_resume_treats_resume_with_session_suffix_as_fork() {
        let args = Cli::parse_from([
            "vtcode",
            "--resume",
            "session-123",
            "--session-id",
            "fork-copy",
        ]);

        let (custom_session_id, session_resume) =
            resolve_session_resume(&args).expect("session resume should resolve");

        assert_eq!(custom_session_id.as_deref(), Some("fork-copy"));
        assert!(matches!(
            session_resume,
            Some(SessionResumeMode::Fork(ref id)) if id == "session-123"
        ));
    }

    #[test]
    fn resolve_session_resume_treats_continue_with_session_suffix_as_latest_fork() {
        let args = Cli::parse_from(["vtcode", "--continue", "--session-id", "fork-copy"]);

        let (custom_session_id, session_resume) =
            resolve_session_resume(&args).expect("continue should resolve");

        assert_eq!(custom_session_id.as_deref(), Some("fork-copy"));
        assert!(matches!(
            session_resume,
            Some(SessionResumeMode::Fork(ref id)) if id == "__latest__"
        ));
    }

    #[test]
    fn validate_resume_all_usage_accepts_resume_and_continue_modes() {
        for args in [
            Cli::parse_from(["vtcode", "--resume", "session-123", "--all"]),
            Cli::parse_from(["vtcode", "--continue", "--all"]),
        ] {
            let (_, session_resume) =
                resolve_session_resume(&args).expect("session resume should resolve");
            assert!(validate_resume_all_usage(&args, session_resume.as_ref()).is_ok());
        }
    }

    #[test]
    fn validate_resume_all_usage_rejects_unscoped_all_flag() {
        let args = Cli::parse_from(["vtcode", "--all"]);
        let (_, session_resume) = resolve_session_resume(&args).expect("session resume");
        let err = validate_resume_all_usage(&args, session_resume.as_ref())
            .expect_err("all flag should be rejected");

        assert!(err.to_string().contains(
            "--all can only be used with resume, continue, fork-session, or exec resume"
        ));
    }

    #[tokio::test]
    async fn cli_model_override_updates_merged_startup_config() {
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();

        unsafe {
            std::env::set_var("OPENAI_API_KEY", "test");
        }
        let args = Cli::parse_from([
            "vtcode",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--model",
            vtcode_core::config::constants::models::openai::GPT_5,
        ]);

        let ctx = StartupContext::from_cli_args(&args)
            .await
            .expect("startup success");

        assert_eq!(
            ctx.config.agent.default_model,
            vtcode_core::config::constants::models::openai::GPT_5
        );
        assert_eq!(
            ctx.agent_config.model,
            vtcode_core::config::constants::models::openai::GPT_5
        );
    }

    #[tokio::test]
    async fn cli_override_with_non_responses_model_warns() {
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();

        unsafe {
            std::env::set_var("OPENAI_API_KEY", "test");
        }
        let args = Cli::parse_from([
            "vtcode",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--model",
            "gpt-oss-20b",
            "--config",
            "prompt_cache.providers.openai.prompt_cache_retention=24h",
        ]);

        let ctx = StartupContext::from_cli_args(&args)
            .await
            .expect("startup success");
        let maybe_warning = check_prompt_cache_retention_compat(
            &ctx.config,
            &ctx.agent_config.model,
            &ctx.agent_config.provider,
        );

        assert!(maybe_warning.is_some());
    }

    #[tokio::test]
    async fn cli_override_with_responses_model_no_warn() {
        let temp = TempDir::new().expect("temp dir");
        let workspace = temp.path().to_path_buf();

        unsafe {
            std::env::set_var("OPENAI_API_KEY", "test");
        }
        let args = Cli::parse_from([
            "vtcode",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--model",
            vtcode_core::config::constants::models::openai::GPT_5,
            "--config",
            "prompt_cache.providers.openai.prompt_cache_retention=24h",
        ]);

        let ctx = StartupContext::from_cli_args(&args)
            .await
            .expect("startup success");
        let maybe_warning = check_prompt_cache_retention_compat(
            &ctx.config,
            &ctx.agent_config.model,
            &ctx.agent_config.provider,
        );

        assert!(maybe_warning.is_none());
    }
}
