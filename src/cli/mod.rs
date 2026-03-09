use std::path::{Path, PathBuf};

use anyhow::Result;
use vtcode_core::cli::args::{Cli, Commands, SkillsRefSubcommand, SkillsSubcommand};
use vtcode_core::mcp::cli::handle_mcp_command;

use vtcode::startup::StartupContext;

mod acp;
mod auto;
mod config;
mod create_project;
mod init;
mod init_project;
mod man;
mod sessions;
mod skills_index;
mod snapshots;
mod trajectory;

/// Skills command options
#[derive(Debug)]
pub struct SkillsCommandOptions {
    pub workspace: PathBuf,
}

pub mod analyze;
pub mod benchmark;
pub mod dependencies;
pub mod exec;
pub mod review;
pub mod schema;
pub mod skills;
pub mod skills_ref;
pub mod update;

use vtcode_core::cli::args::AskCommandOptions;

mod revert;

use self::acp::handle_acp_command;

#[derive(Debug, Clone)]
enum ResolvedCliAction {
    Ask {
        prompt: Option<String>,
        options: AskCommandOptions,
    },
    FullAuto {
        prompt: String,
    },
    Resume {
        mode: vtcode::startup::SessionResumeMode,
    },
    Command(Commands),
    Chat,
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
        handle_acp_command(core_cfg, cfg, ide_target).await?;
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
            dispatch_command(args, startup, command).await?;
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

fn resolve_action(
    args: &Cli,
    startup: &StartupContext,
    print_mode: Option<String>,
    potential_prompt: Option<String>,
) -> Result<ResolvedCliAction> {
    if let Some(print_value) = print_mode {
        return Ok(ResolvedCliAction::Ask {
            prompt: Some(crate::main_helpers::build_print_prompt(print_value)?),
            options: ask_options(args, None, startup.skip_confirmations),
        });
    }

    if let Some(prompt) = potential_prompt {
        return Ok(ResolvedCliAction::Ask {
            prompt: Some(prompt),
            options: ask_options(args, None, startup.skip_confirmations),
        });
    }

    if let Some(prompt) = startup.automation_prompt.clone() {
        return Ok(ResolvedCliAction::FullAuto { prompt });
    }

    if let Some(mode) = startup.session_resume.clone() {
        return Ok(ResolvedCliAction::Resume { mode });
    }

    Ok(match args.command.clone() {
        Some(command) => ResolvedCliAction::Command(command),
        None => ResolvedCliAction::Chat,
    })
}

async fn dispatch_command(args: &Cli, startup: &StartupContext, command: Commands) -> Result<()> {
    let cfg = &startup.config;
    let core_cfg = &startup.agent_config;
    let skip_confirmations = startup.skip_confirmations;
    let full_auto_requested = startup.full_auto_requested;

    match command {
        Commands::AgentClientProtocol { target } => {
            handle_acp_command(core_cfg, cfg, target).await?;
        }
        Commands::ToolPolicy { command } => {
            vtcode_core::cli::tool_policy_commands::handle_tool_policy_command(command).await?;
        }
        Commands::Mcp { command } => {
            handle_mcp_command(command).await?;
        }
        Commands::A2a { command } => {
            vtcode_core::cli::a2a::execute_a2a_command(command).await?;
        }
        Commands::Models { command } => {
            vtcode_core::cli::models_commands::handle_models_command(args, &command).await?;
        }
        Commands::Chat | Commands::ChatVerbose => {
            handle_chat_command(
                core_cfg.clone(),
                startup.config.clone(),
                skip_confirmations,
                full_auto_requested,
                startup.plan_mode_requested,
            )
            .await?;
        }
        Commands::Ask {
            prompt,
            output_format,
        } => {
            handle_ask_single_command(
                core_cfg.clone(),
                prompt,
                ask_options(args, output_format, skip_confirmations),
            )
            .await?;
        }
        Commands::Exec {
            json,
            dry_run,
            events,
            last_message_file,
            command,
            prompt,
        } => {
            let command = exec::resolve_exec_command(command, prompt)?;
            let options = exec::ExecCommandOptions {
                json,
                dry_run,
                events_path: events,
                last_message_file,
                command,
            };
            exec::handle_exec_command(core_cfg, cfg, options).await?;
        }
        Commands::Review(review) => {
            let files = review
                .files
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            let spec = vtcode_core::review::build_review_spec(
                review.last_diff,
                review.target.clone(),
                files,
                review.style.clone(),
            )?;
            let options = review::ReviewCommandOptions {
                json: review.json,
                events_path: review.events.clone(),
                last_message_file: review.last_message_file.clone(),
                spec,
            };
            review::handle_review_command(core_cfg, cfg, options).await?;
        }
        Commands::Schema { command } => {
            schema::handle_schema_command(command).await?;
        }
        Commands::Analyze { analysis_type } => {
            let analysis_type = match analysis_type.as_str() {
                "full" => analyze::AnalysisType::Full,
                "structure" => analyze::AnalysisType::Structure,
                "security" => analyze::AnalysisType::Security,
                "performance" => analyze::AnalysisType::Performance,
                "dependencies" => analyze::AnalysisType::Dependencies,
                "complexity" => analyze::AnalysisType::Complexity,
                _ => analyze::AnalysisType::Full,
            };
            handle_analyze_command(core_cfg.clone(), analysis_type).await?;
        }
        Commands::Trajectory { file, top } => {
            trajectory::handle_trajectory_command(core_cfg, file, top).await?;
        }
        Commands::CreateProject { name, features } => {
            create_project::handle_create_project_command(core_cfg, &name, &features).await?;
        }
        Commands::Revert { turn, partial } => {
            revert::handle_revert_command(core_cfg, turn, partial).await?;
        }
        Commands::Snapshots => {
            snapshots::handle_snapshots_command(core_cfg).await?;
        }
        Commands::CleanupSnapshots { max } => {
            snapshots::handle_cleanup_snapshots_command(core_cfg, Some(max)).await?;
        }
        Commands::Init => {
            init::handle_init_command(&startup.workspace, false, false).await?;
        }
        Commands::Config { output, global } => {
            config::handle_config_command(output.as_deref(), global).await?;
        }
        Commands::InitProject {
            name,
            force,
            migrate,
        } => {
            init_project::handle_init_project_command(name, force, migrate).await?;
        }
        Commands::Benchmark {
            task_file,
            task,
            output,
            max_tasks,
        } => {
            let options = benchmark::BenchmarkCommandOptions {
                task_file,
                inline_task: task,
                output,
                max_tasks,
            };
            benchmark::handle_benchmark_command(core_cfg, cfg, options, full_auto_requested)
                .await?;
        }
        Commands::Man { command, output } => {
            man::handle_man_command(command, output).await?;
        }
        Commands::ListSkills {} => {
            let skills_options = skills_options(startup);
            skills::handle_skills_list(&skills_options).await?;
        }
        Commands::Dependencies(command) => {
            dependencies::handle_dependencies_command(command).await?;
        }
        Commands::Skills(skills_cmd) => {
            let skills_options = skills_options(startup);

            match skills_cmd {
                SkillsSubcommand::List { .. } => {
                    skills::handle_skills_list(&skills_options).await?;
                }
                SkillsSubcommand::Load { name, path } => {
                    if let Some(path_val) = path {
                        skills::handle_skills_load(&skills_options, &name, Some(path_val)).await?;
                    }
                }
                SkillsSubcommand::Info { name } => {
                    skills::handle_skills_info(&skills_options, &name).await?;
                }
                SkillsSubcommand::Create { path, .. } => {
                    skills::handle_skills_create(&path).await?;
                }
                SkillsSubcommand::Validate { path, strict } => {
                    skills::handle_skills_validate(&path, strict).await?;
                }
                SkillsSubcommand::CheckCompatibility => {
                    skills::handle_skills_validate_all(&skills_options).await?;
                }
                SkillsSubcommand::Config => {
                    skills::handle_skills_config(&skills_options).await?;
                }
                SkillsSubcommand::RegenerateIndex => {
                    skills_index::handle_skills_regenerate_index(&skills_options).await?;
                }
                SkillsSubcommand::Unload { .. } => {
                    println!("Skill unload not yet implemented");
                }
                SkillsSubcommand::SkillsRef(skills_ref_cmd) => match skills_ref_cmd {
                    SkillsRefSubcommand::Validate { path } => {
                        skills_ref::handle_skills_ref_validate(&path).await?;
                    }
                    SkillsRefSubcommand::ToPrompt { paths } => {
                        skills_ref::handle_skills_ref_to_prompt(&paths).await?;
                    }
                    SkillsRefSubcommand::List { path } => {
                        skills_ref::handle_skills_ref_list(path.as_deref()).await?;
                    }
                },
            }
        }
        Commands::AnthropicApi { port, host } => {
            handle_anthropic_api_command(core_cfg.clone(), port, host).await?;
        }
        Commands::Update {
            check,
            force,
            list,
            limit,
            pin,
            unpin,
            channel,
            show_config,
        } => {
            let options = update::UpdateCommandOptions {
                check_only: check,
                force,
                list,
                limit,
                pin,
                unpin,
                channel,
                show_config,
            };
            update::handle_update_command(options).await?;
        }
    }

    Ok(())
}

fn ask_options(
    args: &Cli,
    output_format: Option<vtcode_core::cli::args::AskOutputFormat>,
    skip_confirmations: bool,
) -> AskCommandOptions {
    AskCommandOptions {
        output_format,
        allowed_tools: args.allowed_tools.clone(),
        disallowed_tools: args.disallowed_tools.clone(),
        skip_confirmations,
    }
}

fn skills_options(startup: &StartupContext) -> SkillsCommandOptions {
    SkillsCommandOptions {
        workspace: startup.workspace.clone(),
    }
}

async fn handle_ask_single_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    prompt: Option<String>,
    options: AskCommandOptions,
) -> Result<()> {
    let prompt_vec = prompt.into_iter().collect::<Vec<_>>();
    vtcode_core::commands::ask::handle_ask_command(core_cfg, prompt_vec, options).await
}

async fn handle_chat_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    vt_cfg: vtcode_core::config::loader::VTCodeConfig,
    skip_confirmations: bool,
    full_auto_requested: bool,
    plan_mode: bool,
) -> Result<()> {
    crate::agent::agents::run_single_agent_loop(
        &core_cfg,
        Some(vt_cfg),
        skip_confirmations,
        full_auto_requested,
        plan_mode,
        None,
    )
    .await
}

async fn handle_analyze_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    analysis_type: analyze::AnalysisType,
) -> Result<()> {
    let depth = match analysis_type {
        analyze::AnalysisType::Full
        | analyze::AnalysisType::Structure
        | analyze::AnalysisType::Complexity => "deep",
        analyze::AnalysisType::Security
        | analyze::AnalysisType::Performance
        | analyze::AnalysisType::Dependencies => "standard",
    };

    vtcode_core::commands::analyze::handle_analyze_command(
        core_cfg,
        depth.to_string(),
        "text".to_string(),
    )
    .await
}

async fn handle_resume_session_command(
    core_cfg: &vtcode_core::config::types::AgentConfig,
    mode: vtcode::startup::SessionResumeMode,
    show_all: bool,
    custom_session_id: Option<String>,
    skip_confirmations: bool,
) -> Result<()> {
    sessions::handle_resume_session_command(
        core_cfg,
        mode,
        show_all,
        custom_session_id,
        skip_confirmations,
    )
    .await
}

pub fn set_workspace_env(workspace: &Path) {
    unsafe {
        std::env::set_var("VTCODE_WORKSPACE", workspace);
    }
}

pub fn set_additional_dirs_env(additional_dirs: &[PathBuf]) {
    let dirs_str = additional_dirs
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(":");
    unsafe {
        std::env::set_var("VTCODE_ADDITIONAL_DIRS", dirs_str);
    }
}

#[cfg(feature = "anthropic-api")]
async fn handle_anthropic_api_command(
    core_cfg: vtcode_core::config::types::AgentConfig,
    port: u16,
    host: String,
) -> Result<()> {
    use std::net::SocketAddr;
    use vtcode_core::anthropic_api::server::{AnthropicApiServerState, create_router};

    let provider = vtcode_core::llm::factory::create_provider_for_model(
        &core_cfg.model,
        core_cfg.api_key.clone(),
        None,
    )
    .map_err(|e| anyhow::anyhow!("Failed to create LLM provider: {}", e))?;

    let state =
        AnthropicApiServerState::new(std::sync::Arc::from(provider), core_cfg.model.clone());
    let app = create_router(state);

    let addr = format!("{}:{}", host, port)
        .parse::<SocketAddr>()
        .map_err(|e| anyhow::anyhow!("Invalid address {}:{}: {}", host, port, e))?;

    println!("Anthropic API server starting on http://{}", addr);
    println!("Compatible with Anthropic Messages API at /v1/messages");
    println!("Press Ctrl+C to stop the server");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind to address {}: {}", addr, e))?;

    ::axum::serve(listener, app)
        .with_graceful_shutdown(vtcode_core::shutdown::shutdown_signal_logged("server"))
        .await
        .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;

    Ok(())
}

#[cfg(not(feature = "anthropic-api"))]
async fn handle_anthropic_api_command(
    _core_cfg: vtcode_core::config::types::AgentConfig,
    _port: u16,
    _host: String,
) -> Result<()> {
    Err(anyhow::anyhow!(
        "Anthropic API server is not enabled. Recompile with --features anthropic-api"
    ))
}

#[cfg(test)]
mod tests {
    use super::{ResolvedCliAction, handle_resume_session_command, resolve_action};
    use clap::Parser;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use vtcode::startup::{SessionResumeMode, StartupContext};
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
