//! VTCode - Research-preview Rust coding agent
//!
//! Thin binary entry point that delegates to modular CLI handlers.

use anyhow::Result;
use clap::Parser;
use colorchoice::ColorChoice as GlobalColorChoice;
use vtcode::startup::StartupContext;
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::api_keys::load_dotenv;

mod acp;
mod agent;
mod cli; // local CLI handlers in src/cli // agent runloops (single-agent only)
mod workspace_trust;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    if std::env::var("RUST_LOG").is_ok() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    }

    // Load .env (non-fatal if missing)
    load_dotenv().ok();

    let args = Cli::parse();
    args.color.write_global();
    if args.no_color {
        GlobalColorChoice::Never.write_global();
    }

    let startup = StartupContext::from_cli_args(&args)?;
    cli::set_workspace_env(&startup.workspace);

    let cfg = &startup.config;
    let core_cfg = &startup.agent_config;
    let skip_confirmations = startup.skip_confirmations;
    let full_auto_requested = startup.full_auto_requested;

    if let Some(prompt) = startup.automation_prompt.as_ref() {
        cli::handle_auto_task_command(core_cfg, cfg, prompt).await?;
        return Ok(());
    }

    if let Some(resume_mode) = startup.session_resume.clone() {
        cli::handle_resume_session_command(core_cfg, resume_mode, skip_confirmations).await?;
        return Ok(());
    }

    match &args.command {
        Some(Commands::AgentClientProtocol { target }) => {
            cli::handle_acp_command(core_cfg, cfg, *target).await?;
        }
        Some(Commands::ToolPolicy { command }) => {
            vtcode_core::cli::tool_policy_commands::handle_tool_policy_command(command.clone())
                .await?;
        }
        Some(Commands::Mcp { command }) => {
            cli::handle_mcp_command(command.clone()).await?;
        }
        Some(Commands::Models { command }) => {
            vtcode_core::cli::models_commands::handle_models_command(&args, command).await?;
        }
        Some(Commands::Chat) => {
            cli::handle_chat_command(core_cfg, skip_confirmations, full_auto_requested).await?;
        }
        Some(Commands::Ask {
            prompt,
            output_format,
        }) => {
            let options = cli::AskCommandOptions {
                output_format: *output_format,
            };
            cli::handle_ask_single_command(core_cfg, prompt, options).await?;
        }
        Some(Commands::Exec {
            json,
            events,
            last_message_file,
            prompt,
        }) => {
            let options = cli::ExecCommandOptions {
                json: *json,
                events_path: events.clone(),
                last_message_file: last_message_file.clone(),
            };
            cli::handle_exec_command(core_cfg, cfg, options, prompt.clone()).await?;
        }
        Some(Commands::ChatVerbose) => {
            // Reuse chat path; verbose behavior is handled in the module if applicable
            cli::handle_chat_command(core_cfg, skip_confirmations, full_auto_requested).await?;
        }
        Some(Commands::Analyze) => {
            cli::handle_analyze_command(core_cfg).await?;
        }
        Some(Commands::Performance) => {
            cli::handle_performance_command().await?;
        }
        Some(Commands::Trajectory { file, top }) => {
            cli::handle_trajectory_logs_command(core_cfg, file.clone(), *top).await?;
        }
        Some(Commands::CreateProject { name, features }) => {
            cli::handle_create_project_command(core_cfg, name, features).await?;
        }
        Some(Commands::CompressContext) => {
            cli::handle_compress_context_command(core_cfg).await?;
        }
        Some(Commands::Revert { turn, partial }) => {
            cli::handle_revert_command(core_cfg, *turn, partial.clone()).await?;
        }
        Some(Commands::Snapshots) => {
            cli::handle_snapshots_command(core_cfg).await?;
        }
        Some(Commands::CleanupSnapshots { max }) => {
            cli::handle_cleanup_snapshots_command(core_cfg, Some(*max)).await?;
        }
        Some(Commands::Init) => {
            cli::handle_init_command(&startup.workspace, false, false).await?;
        }
        Some(Commands::Config { output, global }) => {
            cli::handle_config_command(output.as_deref(), *global).await?;
        }
        Some(Commands::InitProject {
            name,
            force,
            migrate,
        }) => {
            cli::handle_init_project_command(name.clone(), *force, *migrate).await?;
        }
        Some(Commands::Benchmark {
            task_file,
            task,
            output,
            max_tasks,
        }) => {
            let options = cli::BenchmarkCommandOptions {
                task_file: task_file.clone(),
                inline_task: task.clone(),
                output: output.clone(),
                max_tasks: *max_tasks,
            };
            cli::handle_benchmark_command(core_cfg, cfg, options, full_auto_requested).await?;
        }
        Some(Commands::Man { command, output }) => {
            cli::handle_man_command(command.clone(), output.clone()).await?;
        }
        _ => {
            // Default to chat
            cli::handle_chat_command(core_cfg, skip_confirmations, full_auto_requested).await?;
        }
    }

    Ok(())
}
