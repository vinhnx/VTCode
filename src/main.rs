//! VTCode - Research-preview Rust coding agent
//!
//! Thin binary entry point that delegates to modular CLI handlers.

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use colorchoice::ColorChoice as GlobalColorChoice;
use std::io::IsTerminal;
use std::io::{self, Read};
use std::path::PathBuf;
use vtcode::startup::StartupContext;
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::api_keys::load_dotenv;
use vtcode_core::ui::tui::log::make_tui_log_layer;
use vtcode_core::ui::tui::panic_hook;
// FullTui import removed – not used in this binary.

mod agent;
mod cli; // local CLI handlers in src/cli // agent runloops (single-agent only)
mod hooks;
mod ide_context;
mod process_hardening;
mod workspace_trust;

#[tokio::main]
async fn main() -> Result<()> {
    panic_hook::init_panic_hook();

    // Load .env (non-fatal if missing)
    if let Err(err) = load_dotenv() {
        eprintln!("warning: failed to load .env: {err}");
    }

    process_hardening::apply_process_hardening()
        .context("failed to apply process hardening safeguards")?;

    // If user asked for help, augment the help output with dynamic model list
    // and print the help with the additional CLI details.
    if std::env::args().any(|a| a == "-h" || a == "--help") {
        let mut cmd = Cli::command();
        let help_extra = vtcode_core::cli::help::openai_responses_models_help();
        let help_box: Box<str> = help_extra.into_boxed_str();
        let help_static: &'static str = Box::leak(help_box);
        cmd = cmd.after_help(help_static);
        cmd.print_help().ok();
        println!();
        return Ok(());
    }

    let args = Cli::parse();

    // Initialize tracing based on both RUST_LOG env var and config
    let env_tracing_initialized = match initialize_tracing(&args).await {
        Ok(initialized) => initialized,
        Err(err) => {
            eprintln!("warning: failed to initialize tracing from environment: {err}");
            false
        }
    };

    if args.print.is_some() && args.command.is_some() {
        anyhow::bail!(
            "The --print/-p flag cannot be combined with subcommands. Use print mode without a subcommand."
        );
    }

    let print_mode = args.print.clone();
    args.color.write_global();
    if args.no_color {
        GlobalColorChoice::Never.write_global();
    }

    // Check if the workspace_path is actually a prompt (not a directory)
    // This happens when a user runs `vtcode "some prompt"` - clap treats it as workspace_path
    let (startup, potential_prompt) = if let Some(workspace_path) = &args.workspace_path {
        if !workspace_path.exists() || !workspace_path.is_dir() {
            // This looks like a prompt rather than a workspace directory
            let prompt_text = workspace_path.to_string_lossy().to_string();
            // Create startup context with current directory as workspace
            let mut modified_args = args.clone();
            modified_args.workspace_path =
                Some(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let startup = StartupContext::from_cli_args(&modified_args)
                .await
                .context("failed to initialize VTCode startup context")?;
            (startup, Some(prompt_text))
        } else {
            // Valid workspace directory, proceed normally
            let startup = StartupContext::from_cli_args(&args)
                .await
                .context("failed to initialize VTCode startup context")?;
            (startup, None)
        }
    } else {
        let startup = StartupContext::from_cli_args(&args)
            .await
            .context("failed to initialize VTCode startup context")?;
        (startup, None)
    };

    cli::set_workspace_env(&startup.workspace);

    // Initialize tracing based on config if enabled
    if startup.config.debug.enable_tracing
        && !env_tracing_initialized
        && let Err(err) = initialize_tracing_from_config(&startup.config)
    {
        eprintln!("warning: failed to initialize tracing from config: {err}");
        tracing::warn!(error = %err, "failed to initialize tracing from config");
    }

    let cfg = &startup.config;
    let core_cfg = &startup.agent_config;
    let skip_confirmations = startup.skip_confirmations;
    let full_auto_requested = startup.full_auto_requested;

    if let Some(print_value) = print_mode {
        let prompt = build_print_prompt(print_value)?;
        cli::handle_ask_single_command(core_cfg, &prompt, cli::AskCommandOptions::default())
            .await?;
        return Ok(());
    }

    // Handle potential prompt from workspace_path argument (when user runs `vtcode "prompt"`)
    if let Some(prompt) = potential_prompt {
        cli::handle_ask_single_command(core_cfg, &prompt, cli::AskCommandOptions::default())
            .await?;
        return Ok(());
    }

    if let Some(prompt) = startup.automation_prompt.as_ref() {
        cli::handle_auto_task_command(core_cfg, cfg, prompt).await?;
        return Ok(());
    }

    if let Some(resume_mode) = &startup.session_resume {
        cli::handle_resume_session_command(core_cfg, resume_mode.clone(), skip_confirmations)
            .await?;
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
        Some(Commands::Trajectory { file, top }) => {
            cli::handle_trajectory_logs_command(core_cfg, file.clone(), *top).await?;
        }
        Some(Commands::CreateProject { name, features }) => {
            cli::handle_create_project_command(core_cfg, name, features).await?;
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
        Some(Commands::Tokens { command }) => {
            vtcode_core::cli::token_commands::handle_token_command(command)
                .await
                .map_err(|e| anyhow::anyhow!("{:?}", e))?;
        }
        Some(Commands::ListSkills {}) => {
            let skills_options = cli::SkillsCommandOptions {
                workspace: startup.workspace.clone(),
            };
            cli::handle_skills_list(&skills_options).await?;
        }
        Some(Commands::Skills(skills_cmd)) => {
            let skills_options = cli::SkillsCommandOptions {
                workspace: startup.workspace.clone(),
            };
            use vtcode_core::cli::args::SkillsSubcommand;
            match skills_cmd {
                SkillsSubcommand::List { .. } => {
                    cli::handle_skills_list(&skills_options).await?;
                }
                SkillsSubcommand::Load { name, path } => {
                    cli::handle_skills_load(&skills_options, name, path.clone()).await?;
                }
                SkillsSubcommand::Info { name } => {
                    cli::handle_skills_info(&skills_options, name).await?;
                }
                SkillsSubcommand::Create { path, .. } => {
                    cli::handle_skills_create(path).await?;
                }
                SkillsSubcommand::Validate { path } => {
                    cli::handle_skills_validate(path).await?;
                }
                SkillsSubcommand::CheckCompatibility => {
                    cli::handle_skills_validate_all(&skills_options).await?;
                }
                SkillsSubcommand::Config => {
                    cli::handle_skills_config(&skills_options).await?;
                }
                SkillsSubcommand::Unload { .. } => {
                    println!("Skill unload not yet implemented");
                }
            }
        }
        _ => {
            // Default to chat
            cli::handle_chat_command(core_cfg, skip_confirmations, full_auto_requested).await?;
        }
    }

    Ok(())
}

fn build_print_prompt(print_value: String) -> Result<String> {
    let piped_input = collect_piped_stdin()?;
    let inline_prompt = if print_value.trim().is_empty() {
        None
    } else {
        Some(print_value)
    };

    match (piped_input, inline_prompt) {
        (Some(piped), Some(prompt)) => {
            let mut combined = piped;
            if !combined.ends_with("\n\n") {
                if combined.ends_with('\n') {
                    combined.push('\n');
                } else {
                    combined.push_str("\n\n");
                }
            }
            combined.push_str(&prompt);
            Ok(combined)
        }
        (Some(piped), None) => Ok(piped),
        (None, Some(prompt)) => Ok(prompt),
        (None, None) => Err(anyhow::anyhow!(
            "No prompt provided. Pass text to -p/--print or pipe input via stdin."
        )),
    }
}

fn collect_piped_stdin() -> Result<Option<String>> {
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }

    let mut buffer = String::new();
    stdin
        .read_to_string(&mut buffer)
        .context("Failed to read prompt from stdin")?;

    if buffer.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(buffer))
    }
}

async fn initialize_tracing(_args: &Cli) -> Result<bool> {
    use tracing_subscriber::{fmt::format::FmtSpan, prelude::*};

    // Check if RUST_LOG env var is set (takes precedence)
    if std::env::var("RUST_LOG").is_ok() {
        let env_filter = tracing_subscriber::EnvFilter::from_default_env();
        let fmt_layer = tracing_subscriber::fmt::layer().with_span_events(FmtSpan::FULL);
        let init_result = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(make_tui_log_layer())
            .try_init();
        if let Err(err) = init_result {
            tracing::warn!(error = %err, "tracing already initialized; skipping env tracing setup");
        }
        return Ok(true);
    }
    // Note: Config-based tracing initialization is handled in initialize_tracing_from_config()
    // when DebugConfig is loaded. This function just ensures RUST_LOG is respected.

    Ok(false)
}

fn initialize_tracing_from_config(
    config: &vtcode_core::config::loader::VTCodeConfig,
) -> Result<()> {
    use tracing_subscriber::{fmt::format::FmtSpan, prelude::*};

    let debug_cfg = &config.debug;
    let targets = if debug_cfg.trace_targets.is_empty() {
        "vtcode_core,vtcode".to_string()
    } else {
        debug_cfg.trace_targets.join(",")
    };

    let filter_str = format!("{}={}", targets, debug_cfg.trace_level.as_str());

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&filter_str));
    let fmt_layer = tracing_subscriber::fmt::layer().with_span_events(FmtSpan::FULL);
    let init_result = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(make_tui_log_layer())
        .try_init();

    match init_result {
        Ok(()) => {
            tracing::info!(
                "Debug tracing enabled: targets={}, level={}",
                targets,
                debug_cfg.trace_level
            );
        }
        Err(err) => {
            tracing::warn!(
                error = %err,
                "tracing already initialized; skipping config tracing setup"
            );
        }
    }

    Ok(())
}
