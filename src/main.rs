//! VT Code - Research-preview Rust coding agent
//!
//! Thin binary entry point that delegates to modular CLI handlers.

use anyhow::{Context, Result};
use clap::{CommandFactory, FromArgMatches};
use colorchoice::ColorChoice as GlobalColorChoice;
use std::io::IsTerminal;
use std::io::{self, Read};
use std::path::PathBuf;
use vtcode::startup::StartupContext;
use vtcode_core::cli::args::AgentClientProtocolTarget;
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::api_keys::load_dotenv;
use vtcode_core::ui::tui::log::make_tui_log_layer;
use vtcode_core::ui::tui::panic_hook;
// FullTui import removed – not used in this binary.

mod agent;
mod cli; // local CLI handlers in src/cli // agent runloops (single-agent only)
mod hooks;
mod ide_context;
mod workspace_trust;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match run().await {
        Ok(_) => std::process::ExitCode::SUCCESS,
        Err(_) => std::process::ExitCode::FAILURE,
    }
}

async fn run() -> Result<()> {
    // Suppress macOS malloc warnings that appear as stderr output
    #[cfg(target_os = "macos")]
    {
        // Set environment variables to explicitly disable malloc debugging
        // This is safe to do at startup as we're not in a multi-threaded context yet
        unsafe {
            std::env::set_var("MallocStackLogging", "0");
            std::env::set_var("MallocStackLoggingDirectory", "");
            std::env::set_var("MallocScribble", "0");
            std::env::set_var("MallocGuardEdges", "0");
            std::env::set_var("MallocCheckHeapStart", "0");
            std::env::set_var("MallocCheckHeapEach", "0");
            std::env::set_var("MallocCheckHeapAbort", "0");
            std::env::set_var("MallocCheckHeapSleep", "0");
            std::env::set_var("MallocErrorAbort", "0");
            std::env::set_var("MallocCorruptionAbort", "0");
            std::env::set_var("MallocCheckHeapNoCompact", "0");
        }
    }

    panic_hook::init_panic_hook();

    // Build the CLI command with dynamic augmentations
    let mut cmd = Cli::command();

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

    // Parse arguments using the augmented command
    let matches = cmd.get_matches();
    let args = Cli::from_arg_matches(&matches)?;
    panic_hook::set_debug_mode(args.debug);

    // Load .env (non-fatal if missing)
    if let Err(_err) = load_dotenv()
        && !args.quiet
    {}

    // Initialize tracing based on both RUST_LOG env var and config
    let env_tracing_initialized = match initialize_tracing(&args).await {
        Ok(initialized) => initialized,
        Err(_err) => false,
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
            }
        }
        Some(Commands::Marketplace(marketplace_cmd)) => {
            use vtcode_core::cli::args::MarketplaceSubcommand;
            match marketplace_cmd {
                MarketplaceSubcommand::Add { source, id } => {
                    cli::handle_marketplace_add(source.clone(), id.clone()).await?;
                }
                MarketplaceSubcommand::List => {
                    cli::handle_marketplace_list().await?;
                }
                MarketplaceSubcommand::Remove { id } => {
                    cli::handle_marketplace_remove(id.clone()).await?;
                }
            }
        }
        Some(Commands::Plugin(plugin_cmd)) => {
            use vtcode_core::cli::args::PluginSubcommand;
            match plugin_cmd {
                PluginSubcommand::Install { name, marketplace } => {
                    cli::handle_plugin_install(name.clone(), marketplace.clone()).await?;
                }
                PluginSubcommand::List => {
                    cli::handle_plugin_list().await?;
                }
                PluginSubcommand::Uninstall { name } => {
                    cli::handle_plugin_uninstall(name.clone()).await?;
                }
                PluginSubcommand::Enable { name } => {
                    cli::handle_plugin_enable(name.clone()).await?;
                }
                PluginSubcommand::Disable { name } => {
                    cli::handle_plugin_disable(name.clone()).await?;
                }
                PluginSubcommand::Validate { path } => {
                    cli::handle_plugin_validate(path).await?;
                }
            }
        }
        Some(Commands::SelfUpdate { check, force }) => {
            let options = cli::update::UpdateCommandOptions {
                check_only: *check,
                force: *force,
            };
            cli::update::handle_update_command(options).await?;
        }
        _ => {
            // Default to chat
            cli::handle_chat_command(
                core_cfg.clone(),
                skip_confirmations,
                full_auto_requested,
                startup.plan_mode_requested,
            )
            .await?;
        }
    }

    Ok(())
}

/// Detect available IDE for automatic connection when --ide flag is used
fn detect_available_ide() -> Result<Option<AgentClientProtocolTarget>> {
    use std::env;

    let mut available_ides = Vec::new();

    // Check for Zed (currently the only supported IDE)
    // Zed sets VIMRUNTIME or ZED_CLI when running with ACP
    if env::var("ZED_CLI").is_ok() || env::var("VIMRUNTIME").is_ok() {
        available_ides.push(AgentClientProtocolTarget::Zed);
    }

    // In the future, we could check for other IDEs here:
    // - VS Code: Check for VSCODE_IPC_HOOK_CLI
    // - Others: Add detection logic as needed

    match available_ides.len() {
        0 => Ok(None),
        1 => Ok(Some(available_ides[0])),
        _ => {
            // Multiple IDEs detected, be explicit and don't auto-connect
            Ok(None)
        }
    }
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

async fn initialize_tracing(args: &Cli) -> Result<bool> {
    use tracing_subscriber::{fmt::format::FmtSpan, prelude::*};

    // Check if RUST_LOG env var is set (takes precedence)
    if std::env::var("RUST_LOG").is_ok() {
        let env_filter = tracing_subscriber::EnvFilter::from_default_env();

        // When running in interactive TUI mode, redirect logs to a file to avoid corrupting the display
        // Only write to stderr for non-interactive commands (print, ask with piped input, etc.)
        let is_interactive_tui = args.command.is_none() && std::io::stdin().is_terminal();

        if is_interactive_tui {
            // Redirect logs to a file instead of stderr to avoid TUI corruption
            let log_file = std::path::PathBuf::from("/tmp/vtcode-debug.log");
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_file)
                .context("Failed to open debug log file")?;

            let fmt_layer = tracing_subscriber::fmt::layer()
                .with_writer(std::sync::Arc::new(file))
                .with_span_events(FmtSpan::FULL)
                .with_ansi(false); // No ANSI codes in file

            let init_result = tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .with(make_tui_log_layer())
                .try_init();

            if let Err(err) = init_result {
                tracing::warn!(error = %err, "tracing already initialized; skipping env tracing setup");
            }
        } else {
            // Non-interactive mode: write to stderr as normal
            let fmt_layer = tracing_subscriber::fmt::layer().with_span_events(FmtSpan::FULL);
            let init_result = tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt_layer)
                .with(make_tui_log_layer())
                .try_init();

            if let Err(err) = init_result {
                tracing::warn!(error = %err, "tracing already initialized; skipping env tracing setup");
            }
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

    // Always redirect config-based tracing to a file to avoid TUI corruption
    let log_file = std::path::PathBuf::from("/tmp/vtcode-debug.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .context("Failed to open debug log file")?;

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::sync::Arc::new(file))
        .with_span_events(FmtSpan::FULL)
        .with_ansi(false);

    let init_result = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(make_tui_log_layer())
        .try_init();

    match init_result {
        Ok(()) => {
            tracing::info!(
                "Debug tracing enabled: targets={}, level={}, log_file={}",
                targets,
                debug_cfg.trace_level,
                log_file.display()
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
