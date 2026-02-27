use anyhow::{Context, Result};
use std::io::{self, Read};

use vtcode_core::cli::args::AgentClientProtocolTarget;
use vtcode_core::cli::args::Cli;
use vtcode_core::utils::tty::TtyExt;
use vtcode_tui::log::make_tui_log_layer;

/// Detect available IDE for automatic connection when --ide flag is used.
pub(crate) fn detect_available_ide() -> Result<Option<AgentClientProtocolTarget>> {
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

pub(crate) fn build_print_prompt(print_value: String) -> Result<String> {
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
    // Use crossterm's IsTty trait for consistent TTY detection
    if stdin.is_tty_ext() {
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

pub(crate) async fn initialize_tracing(args: &Cli) -> Result<bool> {
    use tracing_subscriber::{fmt::format::FmtSpan, prelude::*};

    // Check if RUST_LOG env var is set (takes precedence)
    if std::env::var("RUST_LOG").is_ok() {
        let env_filter = tracing_subscriber::EnvFilter::from_default_env();

        // When running in interactive TUI mode, redirect logs to a file to avoid corrupting the display
        // Use crossterm's IsTty trait for consistent TTY detection
        let is_interactive_tui = args.command.is_none() && io::stdin().is_tty_ext();

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

pub(crate) fn initialize_tracing_from_config(
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
