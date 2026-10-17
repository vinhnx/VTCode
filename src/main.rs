//! VT Code - Research-preview Rust coding agent
//!
//! Thin binary entry point that delegates to modular CLI handlers.
#![allow(
    clippy::blocks_in_conditions,
    clippy::expect_used,
    clippy::filter_next,
    clippy::large_futures,
    clippy::uninlined_format_args,
    clippy::unwrap_used,
    reason = "The CLI preserves compatibility with existing diagnostics while the workspace migration addresses actionable lint families."
)]
#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
#![expect(
    unused_results,
    clippy::let_underscore_must_use,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "CLI startup and compatibility modules retain established builder, parser, and best-effort I/O patterns while they are migrated incrementally."
)]
// Suppress the cosmetic ld64 warning on macOS: the dev/test `__eh_frame`
// section exceeds ld64's 16 MiB compact-unwind limit, so ld falls back to
// DWARF unwind tables and warns (rust-lang/rust#159105). This lint can only
// be controlled at the root of a linked crate — enabling it here covers both
// the `vtcode` binary and its unit-test harness. Do NOT instead pass
// `-Wl,-no_compact_unwind`: that flag breaks panic unwinding binary-wide
// (`failed to initiate panic, error 5` / SIGABRT, rust-lang/rust#156812).
#![cfg_attr(
    target_os = "macos",
    allow(
        linker_messages,
        reason = "ld64's __eh_frame-too-large warning is informational in dev builds; release builds use panic=abort + strip. See build.rs."
    )
)]
#![recursion_limit = "256"]

use anyhow::{Context, Result};

mod allocator;

use clap::FromArgMatches;
use colorchoice::ColorChoice as GlobalColorChoice;
use vtcode_commons::color_policy;
use vtcode_commons::{VtCodePaths, env_lock};
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
    build_augmented_cli_command, configure_debug_session_routing, configure_runtime_relaunch_context,
    debug_runtime_flag_enabled, initialize_default_error_tracing, initialize_tracing, initialize_tracing_from_config,
    perform_queued_runtime_relaunch, resolve_runtime_color_policy, resolve_startup_context, try_enhance_clap_error,
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

#[cfg_attr(feature = "profiling", hotpath::main)]
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
                    let result = runtime.block_on(run(prepared));
                    // Bound runtime teardown. Dropping a multi-thread runtime
                    // joins every worker and waits indefinitely for in-flight
                    // `spawn_blocking` tasks (Tokio's `BlockingPool::drop`
                    // calls `shutdown(None)`), so a stuck git snapshot, child
                    // reap, or provider read can park the process after the
                    // terminal is already restored. A short budget caps that
                    // tail; normal teardown exits well within it.
                    let runtime_shutdown_started = std::time::Instant::now();
                    tracing::debug!(target: "vtcode.shutdown", boundary = "runtime_shutdown_started", elapsed_ms = 0u64);
                    runtime.shutdown_timeout(std::time::Duration::from_millis(500));
                    tracing::debug!(target: "vtcode.shutdown", boundary = "process_return", elapsed_ms = runtime_shutdown_started.elapsed().as_millis() as u64);
                    result
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
            // Fallback if print_error_report was a no-op (non-TUI run that
            // still touched raw mode via palette probe).
            let _ = panic_hook::restore_tui();
            let _ = std::io::Write::flush(&mut std::io::stderr());
            std::process::ExitCode::FAILURE
        }
        Err(_) => {
            let _ = panic_hook::restore_tui();
            eprintln!("Error: vtcode main thread panicked");
            let _ = std::io::Write::flush(&mut std::io::stderr());
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
    // This must run before tracing initialization so early CLI/config phases
    // can be captured when VTCODE_STARTUP_TRACE=1.
    vtcode_commons::startup_trace::initialize();
    let bootstrap_start = std::time::Instant::now();
    let bootstrap_phase = vtcode_commons::startup_trace::phase_started();
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
    vtcode_commons::startup_trace::record_milestone("hardening_ready");

    if vtcode_core::maybe_run_zsh_exec_wrapper_mode()? {
        return Ok(BootstrapOutcome::ExitEarly);
    }

    // The hidden `sandbox-exec` subcommand turns the binary into the Linux
    // sandbox helper. It must short-circuit before any startup machinery so
    // sandboxed spawns stay fast and independent of config loading.
    #[cfg(target_os = "linux")]
    {
        if cli::sandbox_exec::try_run_sandbox_exec_mode() {
            return Ok(BootstrapOutcome::ExitEarly);
        }
    }

    let cli_start = std::time::Instant::now();
    let cli_phase = vtcode_commons::startup_trace::phase_started();
    let matches = match build_augmented_cli_command().try_get_matches() {
        Ok(m) => m,
        Err(err) => {
            if matches!(err.kind(), clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion) {
                vtcode_commons::startup_trace::record_phase("cli_parsing", cli_phase);
                vtcode_commons::startup_trace::record_milestone("short_lived_command_ready");
                vtcode_commons::startup_trace::emit_summary();
            }
            let err_text = err.to_string();
            if let Some(enhanced) = try_enhance_clap_error(&err_text) {
                eprintln!("{enhanced}");
                std::process::exit(1);
            }
            err.exit();
        }
    };
    tracing::debug!(target = "vtcode.startup", elapsed_ms = cli_start.elapsed().as_millis() as u64, "cli parsed");
    let args = Cli::from_arg_matches(&matches)?;
    vtcode_commons::startup_trace::record_phase("cli_parsing", cli_phase);
    let startup_policy = startup::command_startup_policy(&args);
    panic_hook::set_debug_mode(args.debug);
    let color_eyre_enabled = debug_runtime_flag_enabled(args.debug, "VTCODE_COLOR_EYRE");
    panic_hook::set_color_eyre_enabled(color_eyre_enabled);
    let tui_log_capture_enabled = debug_runtime_flag_enabled(args.debug, "VTCODE_TUI_LOGS");
    vtcode_ui::tui::log::set_tui_log_capture_enabled(tui_log_capture_enabled);

    // Load .env on the critical path. Legacy path migration is maintenance
    // (marker-checked, idempotent) and must not stall first paint — a large
    // pre-XDG `~/.vtcode` tree has been observed to add 1s+ here.
    if startup_policy.load_dotenv() {
        let environment_phase = vtcode_commons::startup_trace::phase_started();
        if let Err(_err) = load_dotenv()
            && !args.quiet
        {}
        vtcode_commons::startup_trace::record_phase("dotenv_and_migration", environment_phase);
    }
    if startup_policy.migrate_legacy_paths() && !startup_policy.run_interactive_maintenance() {
        // Non-interactive consumers still need migrated paths before dispatch.
        migrate_legacy_global_paths(args.quiet);
    }

    if args.print.is_some() && args.command.is_some() {
        anyhow::bail!("The --print/-p flag cannot be combined with subcommands. Use print mode without a subcommand.");
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
    //
    // Worker threads are named so profilers/`spawn_blocking` traces are
    // attributable to VT Code. `VTCODE_RUNTIME_WORKERS` optionally reserves
    // cores for co-located non-Tokio work; unset keeps the default
    // one-worker-per-core behaviour.
    let runtime_phase = vtcode_commons::startup_trace::phase_started();
    let mut runtime_builder = tokio::runtime::Builder::new_multi_thread();
    runtime_builder.enable_all().thread_name("vtcode-rt-worker");
    if let Some(workers) = vtcode_commons::runtime_diagnostics::configured_worker_threads() {
        runtime_builder.worker_threads(workers);
    }
    let runtime = runtime_builder.build().context("failed to build Tokio runtime")?;
    vtcode_commons::startup_trace::record_phase("runtime_creation", runtime_phase);
    tracing::debug!(
        target = "vtcode.startup",
        elapsed_ms = bootstrap_start.elapsed().as_millis() as u64,
        "runtime ready"
    );

    #[cfg(feature = "profiling")]
    hotpath::tokio_runtime!(runtime.handle());

    // Step 1 of the fast-Tokio principles: measure first. Capture a boot
    // snapshot of stable runtime counters and, when diagnostics are enabled
    // (`VTCODE_RUNTIME_METRICS=1` or `VTCODE_STARTUP_TRACE=1`), report them
    // periodically so global-queue pressure is observable without needing
    // tokio_unstable. Both calls are no-ops when diagnostics are off.
    vtcode_commons::runtime_diagnostics::log_snapshot(runtime.handle(), "boot");
    // Detached: the reporter ends when the runtime drops.
    let _metrics_reporter = vtcode_commons::runtime_diagnostics::spawn_periodic_reporter(runtime.handle());

    // Start the terminal palette probe early for interactive sessions.
    // It runs on a blocking thread and overlaps with startup-context
    // resolution (config loading, auth probing, theme determination), so
    // its 15–200 ms cost is hidden from the user-visible critical path.
    // The agent loop awaits the result before crossterm sets up the
    // terminal to avoid a termios race.
    //
    // Only pre-start for commands that actually enter the agent run loop;
    // one-shot commands (`ask`, `exec`, `schema`, `--print`, …) would
    // otherwise pay ~200 ms at exit while the runtime drop waits for the
    // unused blocking task to finish.
    if startup_policy.run_terminal_probe() {
        agent::probe::start_terminal_palette_probe(runtime.handle());
    }

    let startup = match runtime.block_on(resolve_startup_context(&args)) {
        Ok(startup) => startup,
        Err(error) => {
            // The interactive palette probe runs concurrently with startup
            // resolution. Finish it before returning an error so its raw-mode
            // guard restores the TTY and no delayed terminal replies are
            // printed by the user's shell after VT Code exits.
            if startup_policy.run_terminal_probe() {
                runtime.block_on(agent::probe::finish_terminal_palette_probe());
            }
            return Err(error);
        }
    };
    vtcode_commons::startup_trace::record_milestone("dispatch_ready");
    vtcode_commons::startup_trace::record_phase("bootstrap", bootstrap_phase);
    // For one-shot commands this is the last startup boundary before dispatch;
    // interactive sessions publish the more useful first_ui_render milestone
    // (which emits the summary itself via `record_first_render`).
    if !startup_policy.runs_interactive_session() {
        vtcode_commons::startup_trace::record_milestone("short_lived_command_ready");
        vtcode_commons::startup_trace::emit_summary();
    }
    tracing::debug!(
        target = "vtcode.startup",
        elapsed_ms = bootstrap_start.elapsed().as_millis() as u64,
        "bootstrap ready"
    );

    Ok(BootstrapOutcome::Ready(Box::new(BootstrapReady {
        prepared: PreparedRun { args, startup, print_mode },
        runtime,
    })))
}

fn migrate_legacy_global_paths(quiet: bool) {
    match VtCodePaths::resolve().and_then(|paths| paths.migrate_legacy()) {
        Ok(report) if !report.failures.is_empty() && !quiet => {
            eprintln!(
                "Warning: VT Code legacy migration is incomplete with {} diagnostic failure(s); startup will retry it. See `vtcode --version` for the migration report path.",
                report.failures.len()
            );
        }
        Ok(_) => {}
        Err(error) if !quiet => {
            eprintln!("Warning: VT Code could not start legacy migration: {error:#}. Startup will continue.");
        }
        Err(_) => {}
    }
}

async fn run(prepared: PreparedRun) -> Result<()> {
    let PreparedRun { args, startup, print_mode } = prepared;
    let startup_policy = startup::command_startup_policy(&args);

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
    // Updates are best-effort maintenance. Starting this only for interactive
    // sessions avoids spawning work for metadata and one-shot commands.
    if startup_policy.run_interactive_maintenance() {
        tokio::spawn(updater::run_preflight_check());
    }

    // Interactive legacy-path migration runs after the runtime exists so it
    // never shares the pre-dispatch critical path with dotenv/config/auth.
    // Marker-checked and idempotent; non-interactive paths already migrated.
    if startup_policy.migrate_legacy_paths() && startup_policy.run_interactive_maintenance() {
        let quiet = args.quiet;
        tokio::task::spawn_blocking(move || {
            migrate_legacy_global_paths(quiet);
        });
    }

    // Clean up old spooled large output files (>24h) at startup to prevent
    // unbounded growth. Deferred to a blocking task so a cold cache does not
    // block first user I/O on the critical startup path.
    if startup_policy.run_interactive_maintenance()
        && let Ok(paths) = VtCodePaths::resolve()
        && let Ok(tmp_dir) = paths.cache_path("large-output")
    {
        tokio::task::spawn_blocking(move || {
            if let Err(err) = agent::runloop::tool_output::large_output::cleanup_old_temp_spools(
                &tmp_dir, 86400, // 24 hours
            ) {
                tracing::debug!(error = %err, "Failed to clean old temp spool dirs");
            }
        });
    }

    // First-run iTerm2 tab icon: best-effort background install. The notice
    // is non-critical, so it must not sit on the pre-TUI critical path.
    if startup_policy.run_interactive_maintenance() {
        let quiet = args.quiet;
        tokio::task::spawn_blocking(
            move || match vtcode_core::terminal_setup::terminals::iterm2::ensure_profile_icon() {
                Ok(Some(report)) => {
                    if !quiet {
                        tracing::info!(
                            profile = %report.profile_path.display(),
                            "Installed VT Code iTerm2 tab icon profile"
                        );
                    }
                }
                Ok(None) => {}
                Err(error) => tracing::debug!(error = %error, "iTerm2 tab icon install skipped"),
            },
        );
    }

    let dispatch_result = cli::dispatch(&args, &startup, print_mode).await;
    // The interactive palette probe runs concurrently with startup and
    // dispatch. Paths that never enter the agent loop (e.g. `continue` with
    // no archived sessions) would otherwise exit while the probe is still
    // waiting on `/dev/tty`, leaving the terminal's OSC/DA1 replies to be
    // printed by the shell as visible escape-code garbage after exit.
    // Finish it here so TTY replies are consumed before returning; when the
    // agent loop already awaited the probe this returns immediately.
    // Bounded so a stuck `/dev/tty` read cannot park process exit.
    if startup_policy.run_terminal_probe() {
        if tokio::time::timeout(std::time::Duration::from_millis(500), agent::probe::finish_terminal_palette_probe())
            .await
            .is_err()
        {
            tracing::debug!("terminal palette probe finish timed out during exit; continuing teardown");
        }
    }
    perform_queued_runtime_relaunch();
    vtcode_core::utils::trace_writer::flush_trace_log();
    dispatch_result?;

    Ok(())
}
