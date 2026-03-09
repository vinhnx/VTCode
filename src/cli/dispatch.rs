use anyhow::Result;
use vtcode::startup::{SessionResumeMode, StartupContext};
use vtcode_core::cli::args::{Cli, Commands, SkillsRefSubcommand, SkillsSubcommand};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::mcp::cli::handle_mcp_command;

use super::acp::handle_acp_command;
use super::adapters::{ask_options, skills_options};
use super::anthropic_api::handle_anthropic_api_command;
use super::{
    analyze, benchmark, config, create_project, dependencies, exec, init, init_project, man,
    revert, review, schema, sessions, skills, skills_index, skills_ref, snapshots, trajectory,
    update,
};

pub(super) async fn dispatch_command(
    args: &Cli,
    startup: &StartupContext,
    command: Commands,
) -> Result<()> {
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
            dispatch_skills_command(startup, skills_cmd).await?;
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

async fn dispatch_skills_command(
    startup: &StartupContext,
    skills_cmd: SkillsSubcommand,
) -> Result<()> {
    let skills_options = skills_options(startup);

    match skills_cmd {
        SkillsSubcommand::List { .. } => {
            skills::handle_skills_list(&skills_options).await?;
        }
        SkillsSubcommand::Load { name, path } => {
            skills::handle_skills_load(&skills_options, &name, path).await?;
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

    Ok(())
}

pub(crate) async fn handle_ask_single_command(
    core_cfg: CoreAgentConfig,
    prompt: Option<String>,
    options: vtcode_core::cli::args::AskCommandOptions,
) -> Result<()> {
    let prompt_vec = prompt.into_iter().collect::<Vec<_>>();
    vtcode_core::commands::ask::handle_ask_command(core_cfg, prompt_vec, options).await
}

pub(crate) async fn handle_chat_command(
    core_cfg: CoreAgentConfig,
    vt_cfg: VTCodeConfig,
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
    core_cfg: CoreAgentConfig,
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

pub(crate) async fn handle_resume_session_command(
    core_cfg: &CoreAgentConfig,
    mode: SessionResumeMode,
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
