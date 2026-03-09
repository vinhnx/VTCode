use anyhow::Result;
use vtcode::startup::StartupContext;
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::mcp::cli::handle_mcp_command;

use super::run::{handle_analyze_command, handle_ask_single_command, handle_chat_command};
use super::skills::dispatch_skills_command;
use crate::cli::acp::handle_acp_command;
use crate::cli::adapters::{ask_options, skills_options};
use crate::cli::anthropic_api::handle_anthropic_api_command;
use crate::cli::{
    analyze, benchmark, config, create_project, dependencies, exec, init, init_project, man,
    revert, review, schema, skills, snapshots, trajectory, update,
};

pub(crate) async fn dispatch_command(
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
            handle_analyze_command(
                core_cfg.clone(),
                analyze::AnalysisType::from_cli_arg(&analysis_type),
            )
            .await?;
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
