//! VT Code - Research-preview Rust coding agent
//!
//! Thin binary entry point that delegates to modular CLI handlers.
#![allow(
    clippy::blocks_in_conditions,
    clippy::expect_used,
    clippy::filter_next,
    clippy::large_futures,
    clippy::uninlined_format_args,
    clippy::unwrap_used
)]
#![allow(missing_docs)]

use anyhow::{Context, Result};

mod allocator;

use clap::FromArgMatches;
use colorchoice::ColorChoice as GlobalColorChoice;
use vtcode_commons::color_policy;
use vtcode_commons::env_lock;
use vtcode_core::cli::args::Cli;
use vtcode_core::config::api_keys::load_dotenv;
use vtcode_ui::tui::panic_hook;

mod agent;
mod cli; // local CLI handlers in src/cli // agent runloops (single-agent only)
mod codex_app_server;
mod main_helpers;
mod process_hardening;
mod startup;
mod updater;

use main_helpers::{
    build_augmented_cli_command, configure_debug_session_routing,
    configure_runtime_relaunch_context, debug_runtime_flag_enabled,
    initialize_default_error_tracing, initialize_tracing, initialize_tracing_from_config,
    perform_queued_runtime_relaunch, resolve_runtime_color_policy, resolve_startup_context,
    try_enhance_clap_error,
};

struct PreparedRun {
    args: Cli,
    startup: startup::StartupContext,
    print_mode: Option<String>,
}

struct BootstrapReady {
    prepared: PreparedRun,
    runtime: tokio::runtime::Runtime,
}

enum BootstrapOutcome {
    ExitEarly,
    Ready(Box<BootstrapReady>),
}

fn main() -> std::process::ExitCode {
    // Apply process hardening before any other operations.
    // This disables core dumps, caps RLIMIT_STACK (defense-in-depth complement
    // to Rust's built-in stack clash protection — see rustc exploit-mitigations
    // docs), removes dangerous env vars, and prevents ptrace attach.
    process_hardening::pre_main_hardening();

    // The hardening layer caps RLIMIT_STACK at 8 MiB (only when unlimited).
    // The spawned thread below uses 16 MiB — this is safe because
    // RLIMIT_STACK only constrains the main thread's stack on Linux/macOS;
    // thread stacks allocated via pthread_attr_setstacksize come from the
    // heap and are unaffected by the rlimit.
    const MAIN_THREAD_STACK_BYTES: usize = 16 * 1024 * 1024;

    let handle = match std::thread::Builder::new()
        .name("vtcode-main".to_string())
        .stack_size(MAIN_THREAD_STACK_BYTES)
        .spawn(|| -> Result<()> {
            match bootstrap_main()? {
                BootstrapOutcome::ExitEarly => Ok(()),
                BootstrapOutcome::Ready(ready) => {
                    // Reuse the multi-threaded runtime created during bootstrap
                    // instead of building a second one.
                    let BootstrapReady { prepared, runtime } = *ready;
                    runtime.block_on(run(prepared))
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

#[cfg(target_os = "macos")]
fn remove_runtime_env_var(key: &str) {
    // Delegates to the process-wide env lock so this mutation cannot race with any
    // other VT Code env mutator, even though it runs on the dedicated main thread
    // before any worker threads exist.
    env_lock::remove_var(key);
}

fn bootstrap_main() -> Result<BootstrapOutcome> {
    let launch_argv = std::env::args_os().collect::<Vec<_>>();
    let launch_cwd = std::env::current_dir().context("failed to resolve current directory")?;
    configure_runtime_relaunch_context(launch_argv, launch_cwd);

    // Mark this process as VTCode for HuggingFace agent harness detection.
    // `huggingface_hub` reads this to attribute Hub traffic to VTCode in the
    // public agent usage dataset.  Set early via env_lock before worker threads
    // exist so the mutex acquisition is uncontended.
    {
        let _env_guard = env_lock::lock();
        _env_guard.set_var("VTCODE", "1");
    }

    // Suppress macOS malloc warnings that appear as stderr output
    // IMPORTANT: Remove the variables rather than setting to "0"
    // Setting to "0" triggers macOS to output "can't turn off malloc stack logging"
    // which corrupts the TUI display
    #[cfg(target_os = "macos")]
    {
        for key in [
            "MallocStackLogging",
            "MallocStackLoggingDirectory",
            "MallocScribble",
            "MallocGuardEdges",
            "MallocCheckHeapStart",
            "MallocCheckHeapEach",
            "MallocCheckHeapAbort",
            "MallocCheckHeapSleep",
            "MallocErrorAbort",
            "MallocCorruptionAbort",
            "MallocStackLoggingNoCompact",
            "MallocDoNotProtectSentinel",
            "MallocQuiet",
        ] {
            remove_runtime_env_var(key);
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

    let matches = match build_augmented_cli_command().try_get_matches() {
        Ok(m) => m,
        Err(err) => {
            let err_text = err.to_string();
            if let Some(enhanced) = try_enhance_clap_error(&err_text) {
                eprintln!("{enhanced}");
                std::process::exit(1);
            }
            err.exit();
        }
    };
    let args = Cli::from_arg_matches(&matches)?;
    panic_hook::set_debug_mode(args.debug);
    let color_eyre_enabled = debug_runtime_flag_enabled(args.debug, "VTCODE_COLOR_EYRE");
    panic_hook::set_color_eyre_enabled(color_eyre_enabled);
    let tui_log_capture_enabled = debug_runtime_flag_enabled(args.debug, "VTCODE_TUI_LOGS");
    vtcode_ui::tui::log::set_tui_log_capture_enabled(tui_log_capture_enabled);

    // Load .env (non-fatal if missing)
    if let Err(_err) = load_dotenv()
        && !args.quiet
    {}

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

    // Build the multi-threaded runtime once; it serves both the startup context
    // resolution and the main agent run loop.  Previously a single-threaded
    // runtime was created here and a second multi-threaded one later in main(),
    // which duplicated thread-pool and I/O-driver setup.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build Tokio runtime")?;
    let startup = runtime.block_on(resolve_startup_context(&args))?;

    Ok(BootstrapOutcome::Ready(Box::new(BootstrapReady {
        prepared: PreparedRun {
            args,
            startup,
            print_mode,
        },
        runtime,
    })))
}

async fn run(prepared: PreparedRun) -> Result<()> {
    let PreparedRun {
        args,
        startup,
        print_mode,
    } = prepared;

    configure_debug_session_routing(&args, &startup, &print_mode).await;

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

    // Preflight update check — always fetches from GitHub (force fetch).
    // Spawned as a background task so network I/O never blocks startup.
    // The result is consumed later (after dispatch) via get_preflight_notice().
    tokio::spawn(updater::run_preflight_check());

    // Clean up old spooled large output temp files (>24h) at startup to prevent
    // unbounded growth. Deferred to a blocking task so a cold ~/.vtcode/tmp never
    // blocks first user I/O on the critical startup path.
    if let Ok(home) = std::env::var("HOME") {
        let tmp_dir = std::path::Path::new(&home).join(".vtcode").join("tmp");
        tokio::task::spawn_blocking(move || {
            if let Err(err) = agent::runloop::tool_output::large_output::cleanup_old_temp_spools(
                &tmp_dir, 86400, // 24 hours
            ) {
                tracing::debug!(error = %err, "Failed to clean old temp spool dirs");
            }
        });
    }

    let dispatch_result = cli::dispatch(&args, &startup, print_mode).await;
    perform_queued_runtime_relaunch();
    vtcode_core::utils::trace_writer::flush_trace_log();
    dispatch_result?;

    Ok(())
}
