use anyhow::Result;

use crate::startup::StartupContext;
use vtcode_core::cli::args::Cli;

mod acp;
mod action_resolution;
mod adapters;
mod anthropic_api;
mod auto;
mod checkpoints;
mod config;
mod create_project;
mod dispatch;
mod env;
mod init;
mod init_project;
mod man;
mod sessions;
mod skills_index;
mod snapshots;
mod trajectory;

pub mod analyze;
pub mod benchmark;
pub mod dependencies;
pub mod exec;
pub mod review;
pub mod schema;
pub mod skills;
pub mod skills_ref;
pub mod update;

mod revert;

use action_resolution::ResolvedCliAction;
use dispatch::{handle_ask_single_command, handle_chat_command, handle_resume_session_command};

pub(crate) use action_resolution::resolve_action;
pub use env::{set_additional_dirs_env, set_workspace_env};

/// Skills command options
#[derive(Debug)]
pub struct SkillsCommandOptions {
    pub workspace: std::path::PathBuf,
}

pub async fn dispatch(
    args: &Cli,
    startup: &StartupContext,
    print_mode: Option<String>,
    potential_prompt: Option<String>,
) -> Result<()> {
    let cfg = &startup.config;
    let core_cfg = &startup.agent_config;

    if args.ide
        && args.command.is_none()
        && let Some(ide_target) = crate::main_helpers::detect_available_ide()?
    {
        acp::handle_acp_command(core_cfg, cfg, ide_target).await?;
        return Ok(());
    }

    match resolve_action(args, startup, print_mode, potential_prompt)? {
        ResolvedCliAction::Ask { prompt, options } => {
            handle_ask_single_command(core_cfg.clone(), prompt, options).await?;
        }
        ResolvedCliAction::FullAuto { prompt } => {
            auto::handle_auto_task_command(core_cfg, cfg, &prompt).await?;
        }
        ResolvedCliAction::Resume { mode } => {
            handle_resume_session_command(
                core_cfg,
                mode,
                startup.resume_show_all,
                startup.custom_session_id.clone(),
                startup.skip_confirmations,
            )
            .await?;
        }
        ResolvedCliAction::Command(command) => {
            dispatch::dispatch_command(args, startup, command).await?;
        }
        ResolvedCliAction::Chat => {
            handle_chat_command(
                core_cfg.clone(),
                startup.config.clone(),
                startup.skip_confirmations,
                startup.full_auto_requested,
                startup.plan_mode_requested,
            )
            .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ResolvedCliAction, dispatch::handle_resume_session_command, resolve_action};
    use crate::startup::{SessionResumeMode, StartupContext};
    use clap::Parser;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use vtcode_core::cli::args::{Cli, Commands};
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::{
        AgentConfig as CoreAgentConfig, ModelSelectionSource, ReasoningEffortLevel,
        UiSurfacePreference,
    };
    use vtcode_core::core::agent::snapshots::{
        DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
    };

    fn runtime_config() -> CoreAgentConfig {
        CoreAgentConfig {
            model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW
                .to_string(),
            api_key: "test-key".to_string(),
            provider: "gemini".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: std::env::current_dir().expect("current_dir"),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
            checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
            max_conversation_turns: 1000,
            model_behavior: None,
        }
    }

    fn parse_cli(args: &[&str]) -> Cli {
        Cli::parse_from(args)
    }

    fn startup_context() -> StartupContext {
        StartupContext {
            workspace: PathBuf::from("."),
            additional_dirs: Vec::new(),
            config: VTCodeConfig::default(),
            agent_config: runtime_config(),
            skip_confirmations: false,
            full_auto_requested: false,
            automation_prompt: None,
            session_resume: None,
            resume_show_all: false,
            custom_session_id: None,
            plan_mode_requested: false,
        }
    }

    #[test]
    fn resolve_action_prefers_print_mode() {
        let args = parse_cli(&["vtcode", "chat"]);
        let mut startup = startup_context();
        startup.automation_prompt = Some("auto prompt".to_string());
        startup.session_resume = Some(SessionResumeMode::Latest);

        let action = resolve_action(
            &args,
            &startup,
            Some("summarize this".to_string()),
            Some("workspace prompt".to_string()),
        )
        .expect("print mode should resolve");

        match action {
            ResolvedCliAction::Ask { prompt, .. } => {
                assert_eq!(
                    prompt,
                    Some(
                        crate::main_helpers::build_print_prompt("summarize this".to_string())
                            .expect("print prompt")
                    )
                );
            }
            other => panic!("expected ask action, got {other:?}"),
        }
    }

    #[test]
    fn resolve_action_prefers_workspace_prompt_over_auto_and_resume() {
        let args = parse_cli(&["vtcode", "chat"]);
        let mut startup = startup_context();
        startup.automation_prompt = Some("auto prompt".to_string());
        startup.session_resume = Some(SessionResumeMode::Latest);

        let action = resolve_action(&args, &startup, None, Some("workspace prompt".to_string()))
            .expect("workspace prompt should resolve");

        match action {
            ResolvedCliAction::Ask { prompt, .. } => {
                assert_eq!(prompt.as_deref(), Some("workspace prompt"));
            }
            other => panic!("expected ask action, got {other:?}"),
        }
    }

    #[test]
    fn resolve_action_prefers_auto_over_resume_and_command() {
        let args = parse_cli(&["vtcode", "chat"]);
        let mut startup = startup_context();
        startup.automation_prompt = Some("auto prompt".to_string());
        startup.session_resume = Some(SessionResumeMode::Latest);

        let action = resolve_action(&args, &startup, None, None).expect("auto should resolve");

        match action {
            ResolvedCliAction::FullAuto { prompt } => assert_eq!(prompt, "auto prompt"),
            other => panic!("expected full-auto action, got {other:?}"),
        }
    }

    #[test]
    fn resolve_action_prefers_resume_over_command() {
        let args = parse_cli(&["vtcode", "chat"]);
        let mut startup = startup_context();
        startup.session_resume = Some(SessionResumeMode::Specific("session-123".to_string()));

        let action = resolve_action(&args, &startup, None, None).expect("resume should resolve");

        match action {
            ResolvedCliAction::Resume {
                mode: SessionResumeMode::Specific(session_id),
            } => assert_eq!(session_id, "session-123"),
            other => panic!("expected specific resume action, got {other:?}"),
        }
    }

    #[test]
    fn resolve_action_returns_command_when_explicit_subcommand_exists() {
        let args = parse_cli(&["vtcode", "chat"]);
        let startup = startup_context();

        let action = resolve_action(&args, &startup, None, None).expect("command should resolve");

        match action {
            ResolvedCliAction::Command(Commands::Chat) => {}
            other => panic!("expected chat command action, got {other:?}"),
        }
    }

    #[test]
    fn resolve_action_returns_chat_when_no_special_mode_or_command_exists() {
        let args = parse_cli(&["vtcode"]);
        let startup = startup_context();

        let action = resolve_action(&args, &startup, None, None).expect("chat should resolve");

        assert!(matches!(action, ResolvedCliAction::Chat));
    }

    #[tokio::test]
    async fn resume_session_command_is_wired_to_sessions_handler() {
        let cfg = runtime_config();
        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix time")
            .as_nanos();
        let fake_id = format!("nonexistent-session-{unique_suffix}");

        let result = handle_resume_session_command(
            &cfg,
            SessionResumeMode::Specific(fake_id),
            false,
            None,
            true,
        )
        .await;

        let err = result.expect_err("expected missing session error");
        assert!(
            err.to_string().contains("No session with identifier"),
            "unexpected error: {err:#}"
        );
    }
}
