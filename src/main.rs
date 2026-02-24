//! VT Code - Research-preview Rust coding agent
//!
//! Thin binary entry point that delegates to modular CLI handlers.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use anyhow::{Context, Result};
use clap::{CommandFactory, FromArgMatches};
use colorchoice::ColorChoice as GlobalColorChoice;
use std::path::PathBuf;
use vtcode::startup::StartupContext;
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::api_keys::load_dotenv;
use vtcode_core::ui::tui::panic_hook;

mod agent;
mod cli; // local CLI handlers in src/cli // agent runloops (single-agent only)
mod hooks;
mod ide_context;
mod main_helpers;
mod workspace_trust;

use main_helpers::{
    build_print_prompt, detect_available_ide, initialize_tracing, initialize_tracing_from_config,
};

fn main() -> std::process::ExitCode {
    const MAIN_THREAD_STACK_BYTES: usize = 16 * 1024 * 1024;

    let handle = match std::thread::Builder::new()
        .name("vtcode-main".to_string())
        .stack_size(MAIN_THREAD_STACK_BYTES)
        .spawn(|| -> Result<()> {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("failed to build Tokio runtime")?;
            runtime.block_on(run())
        }) {
        Ok(handle) => handle,
        Err(err) => {
            eprintln!("Error: failed to spawn vtcode main thread: {err}");
            return std::process::ExitCode::FAILURE;
        }
    };

    match handle.join() {
        Ok(Ok(_)) => std::process::ExitCode::SUCCESS,
        Ok(Err(err)) => {
            eprintln!("Error: {err:?}");
            std::process::ExitCode::FAILURE
        }
        Err(_) => {
            eprintln!("Error: vtcode main thread panicked");
            std::process::ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<()> {
    // Suppress macOS malloc warnings that appear as stderr output
    // IMPORTANT: Remove the variables rather than setting to "0"
    // Setting to "0" triggers macOS to output "can't turn off malloc stack logging"
    // which corrupts the TUI display
    #[cfg(target_os = "macos")]
    {
        // Remove malloc debugging environment variables to prevent system warnings
        // This is safe to do at startup as we're not in a multi-threaded context yet
        unsafe {
            std::env::remove_var("MallocStackLogging");
            std::env::remove_var("MallocStackLoggingDirectory");
            std::env::remove_var("MallocScribble");
            std::env::remove_var("MallocGuardEdges");
            std::env::remove_var("MallocCheckHeapStart");
            std::env::remove_var("MallocCheckHeapEach");
            std::env::remove_var("MallocCheckHeapAbort");
            std::env::remove_var("MallocCheckHeapSleep");
            std::env::remove_var("MallocErrorAbort");
            std::env::remove_var("MallocCorruptionAbort");
            std::env::remove_var("MallocStackLoggingNoCompact");
            std::env::remove_var("MallocDoNotProtectSentinel");
            std::env::remove_var("MallocQuiet");
        }
    }

    panic_hook::init_panic_hook();

    // Build the CLI command with dynamic augmentations
    let mut cmd = Cli::command();

    // Inject quick start guidance for first-time users
    cmd = cmd.before_help("Quick start:\n  1. Set your API key: export ANTHROPIC_API_KEY=\"your_key\"\n  2. Run: vtcode chat\n  3. First-time setup will run automatically\n\nFor help: vtcode --help");

    // Inject dynamic version info (XDG directories)
    let version_info = vtcode_core::cli::args::long_version();
    // We leak the string to get a 'static lifetime which clap often expects or handles better
    // for runtime constructed strings passed to builder methods.
    let version_leak: &'static str = Box::leak(version_info.into_boxed_str());
    cmd = cmd.long_version(version_leak);

    // Inject extra help info
    let help_extra = vtcode_core::cli::help::openai_responses_models_help();
    let help_leak: &'static str = Box::leak(help_extra.into_boxed_str());
    cmd = cmd.after_help(help_leak);

    // Add note about slash commands - use static string directly
    cmd = cmd.after_help(
        "\n\nSlash commands (type / in chat):\n  /init     - Reconfigure provider, model, and settings\n  /status   - Show current configuration\n  /doctor   - Diagnose setup issues\n  /plan     - Toggle read-only planning mode\n  /theme    - Switch UI theme\n  /help     - Show all slash commands",
    );

    // Parse arguments using the augmented command
    let matches = cmd.get_matches();
    let args = Cli::from_arg_matches(&matches)?;
    panic_hook::set_debug_mode(args.debug);

    // Load .env (non-fatal if missing)
    if let Err(_err) = load_dotenv()
        && !args.quiet
    {}

    // Initialize tracing based on both RUST_LOG env var and config
    let env_tracing_initialized = initialize_tracing(&args).await.unwrap_or_default();

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
                .context("failed to initialize VT Code startup context")?;
            (startup, Some(prompt_text))
        } else {
            // Valid workspace directory, proceed normally
            let startup = StartupContext::from_cli_args(&args)
                .await
                .context("failed to initialize VT Code startup context")?;
            (startup, None)
        }
    } else {
        let startup = StartupContext::from_cli_args(&args)
            .await
            .context("failed to initialize VT Code startup context")?;
        (startup, None)
    };

    cli::set_workspace_env(&startup.workspace);
    cli::set_additional_dirs_env(&startup.additional_dirs);

    if startup.config.debug.enable_tracing
        && !env_tracing_initialized
        && let Err(err) = initialize_tracing_from_config(&startup.config)
    {
        tracing::warn!(error = %err, "failed to initialize tracing from config");
    }

    let cfg = &startup.config;
    let core_cfg = &startup.agent_config;
    let skip_confirmations = startup.skip_confirmations;
    let full_auto_requested = startup.full_auto_requested;

    // Handle --ide flag for automatic IDE integration
    if args.ide && args.command.is_none() {
        // Try to auto-detect and connect to available IDE
        if let Some(ide_target) = detect_available_ide()? {
            cli::handle_acp_command(core_cfg, cfg, ide_target).await?;
            return Ok(());
        }
    }

    if let Some(print_value) = print_mode {
        let prompt = build_print_prompt(print_value)?;
        let options = cli::AskCommandOptions {
            output_format: None, // This will be set when we handle the Ask subcommand
            allowed_tools: args.allowed_tools.clone(),
            disallowed_tools: args.disallowed_tools.clone(),
            skip_confirmations: startup.skip_confirmations,
        };
        cli::handle_ask_single_command(core_cfg.clone(), Some(prompt), options).await?;
        return Ok(());
    }

    // Handle potential prompt from workspace_path argument (when user runs `vtcode "prompt"`)
    if let Some(prompt) = potential_prompt {
        let options = cli::AskCommandOptions {
            output_format: None,
            allowed_tools: args.allowed_tools.clone(),
            disallowed_tools: args.disallowed_tools.clone(),
            skip_confirmations: startup.skip_confirmations,
        };
        cli::handle_ask_single_command(core_cfg.clone(), Some(prompt), options).await?;
        return Ok(());
    }

    if let Some(prompt) = startup.automation_prompt.as_ref() {
        cli::handle_auto_task_command(core_cfg, cfg, prompt).await?;
        return Ok(());
    }

    if let Some(_resume_mode) = &startup.session_resume {
        cli::handle_resume_session_command(
            core_cfg,
            None, // resume session ID - we're using custom_session_id instead
            startup.custom_session_id.clone(),
            skip_confirmations,
        )
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
        Some(Commands::A2a { command }) => {
            vtcode_core::cli::a2a::execute_a2a_command(command.clone()).await?;
        }
        Some(Commands::Models { command }) => {
            vtcode_core::cli::models_commands::handle_models_command(&args, command).await?;
        }
        Some(Commands::Chat) => {
            cli::handle_chat_command(
                core_cfg.clone(),
                skip_confirmations,
                full_auto_requested,
                startup.plan_mode_requested,
                startup.team_context.clone(),
            )
            .await?;
        }
        Some(Commands::Ask {
            prompt,
            output_format,
        }) => {
            let options = cli::AskCommandOptions {
                output_format: *output_format,
                allowed_tools: args.allowed_tools.clone(),
                disallowed_tools: args.disallowed_tools.clone(),
                skip_confirmations,
            };
            cli::handle_ask_single_command(core_cfg.clone(), prompt.clone(), options).await?;
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
            cli::handle_exec_command(core_cfg.clone(), cfg, options, prompt.clone()).await?;
        }
        Some(Commands::ChatVerbose) => {
            // Reuse chat path; verbose behavior is handled in the module if applicable
            cli::handle_chat_command(
                core_cfg.clone(),
                skip_confirmations,
                full_auto_requested,
                startup.plan_mode_requested,
                startup.team_context.clone(),
            )
            .await?;
        }
        Some(Commands::Analyze { analysis_type }) => {
            let analysis_type = match analysis_type.as_str() {
                "full" => cli::analyze::AnalysisType::Full,
                "structure" => cli::analyze::AnalysisType::Structure,
                "security" => cli::analyze::AnalysisType::Security,
                "performance" => cli::analyze::AnalysisType::Performance,
                "dependencies" => cli::analyze::AnalysisType::Dependencies,
                "complexity" => cli::analyze::AnalysisType::Complexity,
                _ => cli::analyze::AnalysisType::Full,
            };
            cli::handle_analyze_command(core_cfg.clone(), analysis_type).await?;
        }
        Some(Commands::Trajectory { file, top }) => {
            cli::handle_trajectory_logs_command(core_cfg.clone(), file.clone(), Some(*top)).await?;
        }
        Some(Commands::CreateProject { name, features }) => {
            cli::handle_create_project_command(core_cfg.clone(), name, features).await?;
        }

        Some(Commands::Revert { turn, partial }) => {
            cli::handle_revert_command(core_cfg.clone(), *turn, partial.clone()).await?;
        }
        Some(Commands::Snapshots) => {
            cli::handle_snapshots_command(core_cfg.clone()).await?;
        }
        Some(Commands::CleanupSnapshots { max }) => {
            cli::handle_cleanup_snapshots_command(core_cfg.clone(), Some(*max)).await?;
        }
        Some(Commands::Init) => {
            cli::handle_init_command(&startup.workspace, false, false).await?;
        }
        Some(Commands::Config { output, global }) => {
            cli::handle_config_command(output.clone(), *global).await?;
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
            cli::handle_benchmark_command(core_cfg.clone(), cfg, options, full_auto_requested)
                .await?;
        }
        Some(Commands::Man { command, output }) => {
            cli::handle_man_command(command.clone(), output.clone()).await?;
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
                    if let Some(path_val) = path {
                        cli::handle_skills_load(&skills_options, name, path_val.to_path_buf())
                            .await?;
                    }
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
                SkillsSubcommand::RegenerateIndex => {
                    cli::handle_skills_regenerate_index(&skills_options).await?;
                }
                SkillsSubcommand::Unload { .. } => {
                    println!("Skill unload not yet implemented");
                }
                SkillsSubcommand::SkillsRef(skills_ref_cmd) => {
                    use vtcode_core::cli::args::SkillsRefSubcommand;
                    match skills_ref_cmd {
                        SkillsRefSubcommand::Validate { path } => {
                            cli::skills_ref::handle_skills_ref_validate(path).await?;
                        }
                        SkillsRefSubcommand::ToPrompt { paths } => {
                            cli::skills_ref::handle_skills_ref_to_prompt(paths).await?;
                        }
                        SkillsRefSubcommand::List { path } => {
                            cli::skills_ref::handle_skills_ref_list(path.as_deref()).await?;
                        }
                    }
                }
            }
        }

        Some(Commands::AnthropicApi { port, host }) => {
            cli::handle_anthropic_api_command(core_cfg.clone(), *port, host.clone()).await?;
        }
        Some(Commands::SelfUpdate { check, force: _ }) => {
            let options = cli::update::UpdateCommandOptions { check_only: *check };
            cli::update::handle_update_command(options).await?;
        }
        _ => {
            // Default to chat
            cli::handle_chat_command(
                core_cfg.clone(),
                skip_confirmations,
                full_auto_requested,
                startup.plan_mode_requested,
                startup.team_context.clone(),
            )
            .await?;
        }
    }

    Ok(())
}
