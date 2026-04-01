use std::path::{Path, PathBuf};

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
    apply_cli_permission_overrides, apply_permission_mode_override,
    validate_full_auto_configuration, validate_startup_configuration,
};
use vtcode_config::auth::{OpenAIChatGptAuthHandle, resolve_openai_auth};
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::PermissionMode;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::config::validator::{
    check_openai_hosted_shell_compat, check_prompt_cache_retention_compat,
};
use vtcode_core::copilot::{CopilotAuthStatusKind, probe_auth_status};
use vtcode_core::core::agent::config::{
    RuntimeModelSelection, api_key_env_var, build_runtime_agent_config, provider_label,
    resolve_runtime_model_selection,
};
use vtcode_core::core::interfaces::session::PlanModeEntrySource;
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
    pub(crate) summarize_fork: bool,
    pub(crate) plan_mode_entry_source: PlanModeEntrySource,
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

        let mut config = loaded.config;
        apply_autonomous_mode_compat(&mut config, args.permission_mode.as_deref());

        // Determine plan mode: CLI flag takes precedence, then config default_editing_mode
        let plan_mode_from_cli = args
            .permission_mode
            .as_ref()
            .is_some_and(|m| m.eq_ignore_ascii_case("plan"));

        // Check config for default_editing_mode = "plan" if not explicitly set via CLI
        let plan_mode_from_config =
            !plan_mode_from_cli && config.agent.default_editing_mode.is_read_only();

        let plan_mode_entry_source = if plan_mode_from_cli {
            PlanModeEntrySource::CliFlag
        } else if plan_mode_from_config {
            PlanModeEntrySource::ConfigDefault
        } else {
            PlanModeEntrySource::None
        };

        if let Some(ref permission_mode) = args.permission_mode {
            apply_permission_mode_override(&mut config, permission_mode)?;
        }
        apply_cli_permission_overrides(&mut config, &args.allowed_tools, &args.disallowed_tools);

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
        vtcode_core::utils::session_archive::apply_session_history_config_from_vtcode(&config);
        vtcode_core::utils::ansi::apply_file_opener_config(config.file_opener);

        // Validate API key AFTER first-run setup so new users can complete setup first
        let (api_key, openai_chatgpt_auth) = if command_skips_provider_auth(args.command.as_ref()) {
            (String::new(), None)
        } else {
            match resolve_runtime_provider_auth(
                &config,
                &loaded.workspace,
                &selection,
                loaded.first_run_occurred,
            )
            .await
            {
                Ok(auth) => auth,
                Err(err) if can_start_without_provider_auth(args.command.as_ref()) => {
                    tracing::warn!("starting VT Code without provider auth: {err}");
                    (String::new(), None)
                }
                Err(err) => return Err(err),
            }
        };

        let mut agent_config = build_runtime_agent_config(
            args,
            &config,
            loaded.workspace.clone(),
            selection,
            api_key,
            theme_selection,
        );
        agent_config.openai_chatgpt_auth = openai_chatgpt_auth;

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

        if let Some(msg) =
            check_openai_hosted_shell_compat(&config, &agent_config.model, &agent_config.provider)
        {
            tracing::warn!("{}", msg);
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
            summarize_fork: args.summarize,
            plan_mode_entry_source,
        })
    }
}

fn apply_autonomous_mode_compat(config: &mut VTCodeConfig, cli_permission_mode: Option<&str>) {
    if cli_permission_mode.is_some() {
        return;
    }

    if config.permissions.default_mode == PermissionMode::Default && config.agent.autonomous_mode {
        tracing::warn!(
            "config field [agent].autonomous_mode is deprecated; use [permissions].default_mode = \"auto\" instead"
        );
        config.permissions.default_mode = PermissionMode::Auto;
    }
}

async fn resolve_runtime_provider_auth(
    config: &VTCodeConfig,
    workspace: &Path,
    selection: &RuntimeModelSelection,
    first_run_occurred: bool,
) -> Result<(String, Option<OpenAIChatGptAuthHandle>)> {
    if selection
        .provider
        .eq_ignore_ascii_case(crate::codex_app_server::CODEX_PROVIDER)
    {
        return Ok((String::new(), None));
    }

    if selection.provider.eq_ignore_ascii_case("openai") {
        let api_key = get_api_key(&selection.provider, &ApiKeySources::default()).ok();
        let resolved = resolve_openai_auth(
            &config.auth.openai,
            config.agent.credential_storage_mode,
            api_key,
        )
        .with_context(|| missing_api_key_message(config, selection, first_run_occurred))?;
        return Ok((resolved.api_key().to_string(), resolved.handle()));
    }

    if selection.provider.eq_ignore_ascii_case("copilot") {
        let status = probe_auth_status(&config.auth.copilot, Some(workspace)).await;
        return match status.kind {
            CopilotAuthStatusKind::Authenticated => Ok((String::new(), None)),
            CopilotAuthStatusKind::Unauthenticated | CopilotAuthStatusKind::AuthFlowFailed => {
                Err(anyhow::anyhow!(status.message.unwrap_or_else(|| {
                    missing_api_key_message(config, selection, first_run_occurred)
                })))
            }
            CopilotAuthStatusKind::ServerUnavailable => Err(anyhow::anyhow!(
                status.message.unwrap_or_else(|| {
                    "GitHub Copilot CLI is unavailable. Install `copilot`, set `VTCODE_COPILOT_COMMAND`, or configure `[auth.copilot].command`."
                        .to_string()
                })
            )),
        };
    }

    if let Some(custom_provider) = config.custom_provider(&selection.provider) {
        let api_key_env = custom_provider.resolved_api_key_env();
        if let Ok(api_key) = std::env::var(&api_key_env)
            && !api_key.trim().is_empty()
        {
            return Ok((api_key, None));
        }
    }

    let api_key = get_api_key(&selection.provider, &ApiKeySources::default())
        .with_context(|| missing_api_key_message(config, selection, first_run_occurred))?;
    Ok((api_key, None))
}

fn missing_api_key_message(
    config: &VTCodeConfig,
    selection: &RuntimeModelSelection,
    first_run_occurred: bool,
) -> String {
    let provider_name = provider_label(&selection.provider, Some(config));
    if selection
        .provider
        .eq_ignore_ascii_case(crate::codex_app_server::CODEX_PROVIDER)
    {
        return "Codex authentication is managed by the official `codex app-server`. Run `vtcode auth codex`, `vtcode login codex`, or install `codex` if it is unavailable.".to_string();
    }

    if selection.provider.eq_ignore_ascii_case("copilot") {
        return "Authentication not found for GitHub Copilot. Run `vtcode login copilot`. Install `copilot` first if needed; `gh` is only an optional fallback."
            .to_string();
    }

    if let Some(custom_provider) = config.custom_provider(&selection.provider) {
        let env_var = custom_provider.resolved_api_key_env();
        if first_run_occurred {
            return format!(
                "API key not found for {}. To fix:\n  1. Set {} environment variable, or\n  2. Add to .env file, or\n  3. Configure in vtcode.toml under [[custom_providers]]\n\nRun `/init` anytime to reconfigure.",
                provider_name, env_var
            );
        }

        return format!(
            "API key not found for custom provider '{}'. Set {} environment variable (or add to .env file) or configure it in vtcode.toml under [[custom_providers]].",
            provider_name, env_var
        );
    }

    let env_var = selection
        .provider
        .parse::<Provider>()
        .ok()
        .filter(|provider| !provider.uses_managed_auth())
        .map(|provider| provider.default_api_key_env().to_string())
        .unwrap_or_else(|| api_key_env_var(&selection.provider));
    if selection.provider.eq_ignore_ascii_case("openai") {
        return format!(
            "Authentication not found for OpenAI. Set {env_var}, configure it in vtcode.toml, or run `vtcode login openai`."
        );
    }

    if first_run_occurred {
        format!(
            "API key not found for {}. To fix:\n  1. Set {} environment variable, or\n  2. Add to .env file, or\n  3. Configure in vtcode.toml\n\nRun `/init` anytime to reconfigure.",
            provider_name, env_var
        )
    } else {
        format!(
            "API key not found for provider '{}'. Set {} environment variable (or add to .env file) or configure in vtcode.toml.",
            selection.provider,
            api_key_env_var(&selection.provider)
        )
    }
}

fn command_skips_provider_auth(command: Option<&Commands>) -> bool {
    matches!(
        command,
        Some(
            Commands::ToolPolicy { .. }
                | Commands::Login { .. }
                | Commands::Logout { .. }
                | Commands::Auth { .. }
                | Commands::AppServer { .. }
                | Commands::Schedule { .. }
        )
    )
}

fn can_start_without_provider_auth(command: Option<&Commands>) -> bool {
    matches!(
        command,
        None | Some(
            Commands::ToolPolicy { .. }
                | Commands::AgentClientProtocol { .. }
                | Commands::AppServer { .. }
                | Commands::Schedule { .. }
        )
    )
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
        assert!(check_prompt_cache_retention_compat(&cfg, model, provider).is_some());
    }

    #[test]
    fn retention_ok_for_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.prompt_cache.providers.openai.prompt_cache_retention = Some("24h".to_owned());
        let model = vtcode_core::config::constants::models::openai::GPT_5; // responses model
        let provider = "openai";
        assert!(check_prompt_cache_retention_compat(&cfg, model, provider).is_none());
    }

    #[test]
    fn interactive_sessions_can_start_without_provider_auth() {
        assert!(can_start_without_provider_auth(None));
        assert!(!can_start_without_provider_auth(Some(&Commands::Login {
            provider: "openai".to_string(),
        })));
    }

    #[test]
    fn acp_can_start_without_provider_auth() {
        assert!(can_start_without_provider_auth(Some(
            &Commands::AgentClientProtocol {
                target: vtcode_core::cli::args::AgentClientProtocolTarget::Zed,
            },
        )));
    }

    #[test]
    fn app_server_can_start_without_provider_auth() {
        assert!(can_start_without_provider_auth(Some(
            &Commands::AppServer {
                listen: "stdio://".to_string(),
            }
        )));
    }

    #[test]
    fn tool_policy_can_start_without_provider_auth() {
        let command = Commands::ToolPolicy {
            command: vtcode_core::cli::tool_policy_commands::ToolPolicyCommands::Status,
        };

        assert!(command_skips_provider_auth(Some(&command)));
        assert!(can_start_without_provider_auth(Some(&command)));
    }

    #[test]
    fn missing_api_key_message_uses_custom_provider_label_and_env_key() {
        let mut cfg = VTCodeConfig::default();
        cfg.custom_providers
            .push(vtcode_config::core::CustomProviderConfig {
                name: "mycorp".to_string(),
                display_name: "MyCorporateName".to_string(),
                base_url: "https://llm.example/v1".to_string(),
                api_key_env: "MYCORP_API_KEY".to_string(),
                model: "gpt-5-mini".to_string(),
            });

        let selection = RuntimeModelSelection {
            model: "gpt-5-mini".to_string(),
            provider: "mycorp".to_string(),
            model_source: vtcode_core::config::types::ModelSelectionSource::WorkspaceConfig,
        };

        let message = missing_api_key_message(&cfg, &selection, true);

        assert!(message.contains("MyCorporateName"));
        assert!(message.contains("MYCORP_API_KEY"));
        assert!(message.contains("[[custom_providers]]"));
    }

    #[test]
    fn missing_api_key_message_uses_codex_guidance() {
        let cfg = VTCodeConfig::default();
        let selection = RuntimeModelSelection {
            model: "gpt-5-codex".to_string(),
            provider: "codex".to_string(),
            model_source: vtcode_core::config::types::ModelSelectionSource::WorkspaceConfig,
        };

        let message = missing_api_key_message(&cfg, &selection, true);

        assert!(message.contains("codex app-server"));
        assert!(message.contains("vtcode auth codex"));
    }

    #[test]
    fn hosted_shell_warning_for_non_responses_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.provider.openai.hosted_shell.enabled = true;

        let msg = check_openai_hosted_shell_compat(&cfg, "gpt-oss-20b", "openai");
        assert!(msg.is_some());
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

    #[test]
    fn validate_resume_all_usage_accepts_summarized_interactive_fork_via_session_suffix() {
        let args = Cli::parse_from([
            "vtcode",
            "--resume",
            "--session-id",
            "fork-copy",
            "--summarize",
        ]);

        let (_, session_resume) = resolve_session_resume(&args).expect("session resume");

        assert!(matches!(
            session_resume,
            Some(SessionResumeMode::Interactive)
        ));
        assert!(validate_resume_all_usage(&args, session_resume.as_ref()).is_ok());
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
    async fn cli_override_with_hosted_shell_on_non_responses_model_warns() {
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
            "provider.openai.hosted_shell.enabled=true",
        ]);

        let ctx = StartupContext::from_cli_args(&args)
            .await
            .expect("startup success");
        let maybe_warning =
            check_openai_hosted_shell_compat(&ctx.config, &ctx.agent_config.model, "openai");

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
