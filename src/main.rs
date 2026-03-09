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
mod hooks;
mod ide_context;
mod main_helpers;
mod workspace_trust;

use main_helpers::{
    build_augmented_cli_command, configure_debug_session_routing, debug_runtime_flag_enabled,
    initialize_default_error_tracing, initialize_tracing, initialize_tracing_from_config,
    resolve_runtime_color_policy, resolve_startup_context,
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

    let (startup, potential_prompt) = resolve_startup_context(&args).await?;

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
