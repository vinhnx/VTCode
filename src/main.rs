//! VT Code - Research-preview Rust coding agent
//!
//! Thin binary entry point that delegates to modular CLI handlers.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use anyhow::{Context, Result};
use clap::{ColorChoice as CliColorChoice, CommandFactory, FromArgMatches};
use colorchoice::ColorChoice as GlobalColorChoice;
use std::path::PathBuf;
use vtcode::startup::StartupContext;
use vtcode_commons::color_policy::{self, ColorOutputPolicy, ColorOutputPolicySource};
use vtcode_core::cli::args::{Cli, Commands};
use vtcode_core::config::api_keys::load_dotenv;
use vtcode_core::core::threads::{SessionQueryScope, list_recent_sessions_in_scope};
use vtcode_core::utils::session_archive::reserve_session_archive_identifier;
use vtcode_core::utils::terminal_color_probe::probe_and_cache_terminal_palette_harmony;
use vtcode_tui::panic_hook;

mod agent;
mod cli; // local CLI handlers in src/cli // agent runloops (single-agent only)
mod hooks;
mod ide_context;
mod main_helpers;
mod workspace_trust;

use main_helpers::{
    build_command_debug_session_id, configure_runtime_debug_context,
    initialize_default_error_tracing, initialize_tracing, initialize_tracing_from_config,
};

fn env_flag_enabled(var_name: &str) -> bool {
    std::env::var(var_name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on" | "debug"
            )
        })
        .unwrap_or(false)
}

fn debug_runtime_flag_enabled(debug_arg_enabled: bool, env_var: &str) -> bool {
    cfg!(debug_assertions) && (debug_arg_enabled || env_flag_enabled(env_var))
}

fn resolve_runtime_color_policy(args: &Cli) -> ColorOutputPolicy {
    if args.no_color {
        return ColorOutputPolicy {
            enabled: false,
            source: ColorOutputPolicySource::CliNoColor,
        };
    }

    match args.color.color {
        CliColorChoice::Always => ColorOutputPolicy {
            enabled: true,
            source: ColorOutputPolicySource::CliColorAlways,
        },
        CliColorChoice::Never => ColorOutputPolicy {
            enabled: false,
            source: ColorOutputPolicySource::CliColorNever,
        },
        CliColorChoice::Auto => {
            if color_policy::no_color_env_active() {
                ColorOutputPolicy {
                    enabled: false,
                    source: ColorOutputPolicySource::NoColorEnv,
                }
            } else {
                ColorOutputPolicy {
                    enabled: true,
                    source: ColorOutputPolicySource::DefaultAuto,
                }
            }
        }
    }
}

async fn configure_debug_session_routing(
    args: &Cli,
    startup: &StartupContext,
    print_mode: &Option<String>,
    potential_prompt: &Option<String>,
) {
    let mode_hint = if startup.session_resume.is_some() {
        "resume"
    } else if print_mode.is_some() || potential_prompt.is_some() {
        "ask"
    } else if startup.automation_prompt.is_some() {
        "auto"
    } else {
        match args.command {
            Some(Commands::Chat) => "chat",
            Some(Commands::ChatVerbose) => "chat-verbose",
            Some(Commands::Ask { .. }) => "ask",
            Some(Commands::Exec { .. }) => "exec",
            Some(Commands::Review(_)) => "review",
            Some(Commands::Schema { .. }) => "schema",
            Some(Commands::Benchmark { .. }) => "benchmark",
            Some(Commands::Analyze { .. }) => "analyze",
            Some(Commands::AgentClientProtocol { .. }) => "acp",
            Some(_) => "command",
            None => "chat",
        }
    };
    let archive_backed_session = startup.session_resume.is_some()
        || matches!(
            args.command,
            Some(Commands::Chat) | Some(Commands::ChatVerbose)
        )
        || (args.command.is_none()
            && print_mode.is_none()
            && potential_prompt.is_none()
            && startup.automation_prompt.is_none());
    let command_debug_session_id = build_command_debug_session_id(mode_hint);
    if !archive_backed_session {
        configure_runtime_debug_context(command_debug_session_id, None);
        return;
    }

    if let Some(mode) = startup.session_resume.as_ref() {
        match mode {
            vtcode::startup::SessionResumeMode::Specific(identifier)
                if startup.custom_session_id.is_none() =>
            {
                configure_runtime_debug_context(identifier.clone(), Some(identifier.clone()));
                return;
            }
            vtcode::startup::SessionResumeMode::Latest if startup.custom_session_id.is_none() => {
                let scope = if startup.resume_show_all {
                    SessionQueryScope::All
                } else {
                    SessionQueryScope::CurrentWorkspace(startup.workspace.clone())
                };
                if let Ok(listings) = list_recent_sessions_in_scope(1, &scope).await
                    && let Some(listing) = listings.first()
                {
                    let session_id = listing.identifier();
                    configure_runtime_debug_context(session_id.clone(), Some(session_id));
                    return;
                }
                configure_runtime_debug_context(command_debug_session_id, None);
                return;
            }
            vtcode::startup::SessionResumeMode::Interactive
                if startup.custom_session_id.is_none() =>
            {
                configure_runtime_debug_context(command_debug_session_id, None);
                return;
            }
            _ => {}
        }
    }

    let workspace_label = startup
        .workspace
        .file_name()
        .and_then(|component| component.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| "workspace".to_string());
    match reserve_session_archive_identifier(&workspace_label, startup.custom_session_id.clone())
        .await
    {
        Ok(session_id) => {
            configure_runtime_debug_context(session_id.clone(), Some(session_id));
        }
        Err(_) => {
            configure_runtime_debug_context(command_debug_session_id, None);
        }
    }
}

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
            panic_hook::print_error_report(err);
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

    panic_hook::set_app_metadata(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_AUTHORS"),
        Some(env!("CARGO_PKG_REPOSITORY")),
    );
    panic_hook::init_panic_hook();

    if vtcode_core::maybe_run_zsh_exec_wrapper_mode()? {
        return Ok(());
    }

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
        "\n\nSlash commands (type / in chat):\n  /init     - Reconfigure provider, model, and settings\n  /config   - Open interactive settings manager\n  /status   - Show current configuration\n  /doctor   - Diagnose setup issues (inline picker, or use --quick/--full)\n  /update   - Check for VT Code updates (use --list, --pin, --channel)\n  /plan     - Toggle read-only planning mode\n  /theme    - Switch UI theme\n  /history  - Open command history picker\n  /help     - Show all slash commands",
    );

    // Parse arguments using the augmented command
    let matches = cmd.get_matches();
    let args = Cli::from_arg_matches(&matches)?;
    panic_hook::set_debug_mode(args.debug);
    let color_eyre_enabled = debug_runtime_flag_enabled(args.debug, "VTCODE_COLOR_EYRE");
    panic_hook::set_color_eyre_enabled(color_eyre_enabled);
    let tui_log_capture_enabled = debug_runtime_flag_enabled(args.debug, "VTCODE_TUI_LOGS");
    vtcode_tui::log::set_tui_log_capture_enabled(tui_log_capture_enabled);

    // Load .env (non-fatal if missing)
    if let Err(_err) = load_dotenv()
        && !args.quiet
    {}

    // Probe terminal color semantics once and cache for theme-aware ANSI256 mapping.
    probe_and_cache_terminal_palette_harmony();

    if args.print.is_some() && args.command.is_some() {
        anyhow::bail!(
            "The --print/-p flag cannot be combined with subcommands. Use print mode without a subcommand."
        );
    }

    let print_mode = args.print.clone();
    let color_policy = resolve_runtime_color_policy(&args);
    color_policy::set_color_output_policy(color_policy);

    args.color.write_global();
    if !color_policy.enabled {
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

    configure_debug_session_routing(&args, &startup, &print_mode, &potential_prompt).await;

    // Initialize tracing based on both RUST_LOG env var and config
    let env_tracing_initialized = initialize_tracing().await.unwrap_or_default();

    cli::set_workspace_env(&startup.workspace);
    cli::set_additional_dirs_env(&startup.additional_dirs);

    if startup.config.debug.enable_tracing
        && !env_tracing_initialized
        && let Err(err) = initialize_tracing_from_config(&startup.config)
    {
        tracing::warn!(error = %err, "failed to initialize tracing from config");
    } else if !env_tracing_initialized && !startup.config.debug.enable_tracing {
        // Always collect ERROR-level logs into the session archive for post-mortem debugging
        if let Err(err) = initialize_default_error_tracing() {
            eprintln!("warning: failed to initialize default error tracing: {err}");
        }
    }

    // Sync global diagnostics flag so TuiLogLayer respects ui.show_diagnostics_in_transcript
    panic_hook::set_show_diagnostics(startup.config.ui.show_diagnostics_in_transcript);

    cli::dispatch(&args, &startup, print_mode, potential_prompt).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{LazyLock, Mutex};

    use vtcode_config::core::PromptCachingConfig;
    use vtcode_config::types::{
        AgentConfig as StartupAgentConfig, ModelSelectionSource, ReasoningEffortLevel,
        UiSurfacePreference,
    };
    use vtcode_core::config::loader::VTCodeConfig;

    static DEBUG_ROUTING_TEST_GUARD: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    fn startup_agent_config() -> StartupAgentConfig {
        StartupAgentConfig {
            model: vtcode_core::config::constants::models::openai::GPT_5.to_string(),
            api_key: "test-key".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            workspace: PathBuf::from("."),
            verbose: false,
            quiet: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
            checkpointing_enabled: true,
            checkpointing_storage_dir: None,
            checkpointing_max_snapshots: 50,
            checkpointing_max_age_days: Some(30),
            max_conversation_turns: 1000,
            model_behavior: None,
        }
    }

    #[test]
    fn configure_debug_session_routing_reuses_specific_resume_identifier() {
        let _guard = DEBUG_ROUTING_TEST_GUARD
            .lock()
            .expect("debug routing guard");

        let args = Cli::default();
        let config = VTCodeConfig::default();
        let startup = StartupContext {
            workspace: PathBuf::from("."),
            additional_dirs: Vec::new(),
            agent_config: startup_agent_config(),
            config,
            skip_confirmations: false,
            full_auto_requested: false,
            automation_prompt: None,
            session_resume: Some(vtcode::startup::SessionResumeMode::Specific(
                "session-123".to_string(),
            )),
            resume_show_all: false,
            custom_session_id: None,
            plan_mode_requested: false,
        };

        configure_runtime_debug_context("seed".to_string(), Some("seed".to_string()));
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(configure_debug_session_routing(
            &args, &startup, &None, &None,
        ));

        assert_eq!(
            crate::main_helpers::runtime_archive_session_id().as_deref(),
            Some("session-123")
        );
    }
}
