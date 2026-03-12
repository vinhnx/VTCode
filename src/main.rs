//! VT Code - Research-preview Rust coding agent
//!
//! Thin binary entry point that delegates to modular CLI handlers.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use anyhow::{Context, Result};
use clap::FromArgMatches;
use colorchoice::ColorChoice as GlobalColorChoice;
use vtcode_commons::color_policy;
use vtcode_core::cli::args::Cli;
use vtcode_core::config::api_keys::load_dotenv;
use vtcode_core::utils::terminal_color_probe::probe_and_cache_terminal_palette_harmony;
use vtcode_tui::panic_hook;

mod agent;
mod cli; // local CLI handlers in src/cli // agent runloops (single-agent only)
mod main_helpers;
mod startup;
mod updater;

use main_helpers::{
    build_augmented_cli_command, configure_debug_session_routing,
    configure_runtime_relaunch_context, debug_runtime_flag_enabled,
    initialize_default_error_tracing, initialize_tracing, initialize_tracing_from_config,
    perform_queued_runtime_relaunch, resolve_runtime_color_policy, resolve_startup_context,
};

struct PreparedRun {
    args: Cli,
    startup: startup::StartupContext,
    print_mode: Option<String>,
    potential_prompt: Option<String>,
}

enum BootstrapOutcome {
    ExitEarly,
    Ready(Box<PreparedRun>),
}

fn main() -> std::process::ExitCode {
    const MAIN_THREAD_STACK_BYTES: usize = 16 * 1024 * 1024;

    let handle = match std::thread::Builder::new()
        .name("vtcode-main".to_string())
        .stack_size(MAIN_THREAD_STACK_BYTES)
        .spawn(|| -> Result<()> {
            match bootstrap_main()? {
                BootstrapOutcome::ExitEarly => Ok(()),
                BootstrapOutcome::Ready(prepared) => {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .context("failed to build Tokio runtime")?;
                    runtime.block_on(run(*prepared))
                }
            }
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

fn bootstrap_main() -> Result<BootstrapOutcome> {
    let launch_argv = std::env::args_os().collect::<Vec<_>>();
    let launch_cwd = std::env::current_dir().context("failed to resolve current directory")?;
    configure_runtime_relaunch_context(launch_argv, launch_cwd);

    // Suppress macOS malloc warnings that appear as stderr output
    // IMPORTANT: Remove the variables rather than setting to "0"
    // Setting to "0" triggers macOS to output "can't turn off malloc stack logging"
    // which corrupts the TUI display
    #[cfg(target_os = "macos")]
    {
        // SAFETY: this runs on the dedicated main thread before any Tokio runtime or
        // worker threads exist, so there is no concurrent environment access yet.
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
        return Ok(BootstrapOutcome::ExitEarly);
    }

    let matches = build_augmented_cli_command().get_matches();
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

    let startup_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to build startup Tokio runtime")?;
    let (startup, potential_prompt) = startup_runtime.block_on(resolve_startup_context(&args))?;

    Ok(BootstrapOutcome::Ready(Box::new(PreparedRun {
        args,
        startup,
        print_mode,
        potential_prompt,
    })))
}

async fn run(prepared: PreparedRun) -> Result<()> {
    let PreparedRun {
        args,
        startup,
        print_mode,
        potential_prompt,
    } = prepared;

    configure_debug_session_routing(&args, &startup, &print_mode, &potential_prompt).await;

    // Initialize tracing based on both RUST_LOG env var and config
    let env_tracing_initialized = initialize_tracing().await.unwrap_or_default();

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

    let dispatch_result = cli::dispatch(&args, &startup, print_mode, potential_prompt).await;
    perform_queued_runtime_relaunch();
    dispatch_result?;

    Ok(())
}
